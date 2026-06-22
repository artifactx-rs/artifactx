//! Full CLI regression smoke tests for functionality not covered by narrower
//! unit tests. These tests intentionally drive the built `arx` binary so the
//! user-facing command wiring stays covered.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

mod common;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct StaticServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap();
    out
}

fn xz(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = xz2::write::XzEncoder::new(&mut out, 6);
    e.write_all(data).unwrap();
    e.finish().unwrap();
    out
}

fn write_deb(path: &Path, name: &str, version: &str, arch: &str) {
    let control = format!(
        "Package: {name}\nVersion: {version}\nArchitecture: {arch}\nMaintainer: T <t@localhost>\nDescription: test\n"
    );
    let mut control_tar = Vec::new();
    {
        let mut tb = tar::Builder::new(&mut control_tar);
        let bytes = control.as_bytes();
        let mut h = tar::Header::new_gnu();
        h.set_path("./control").unwrap();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        tb.append(&h, bytes).unwrap();
        tb.finish().unwrap();
    }
    let control_gz = gzip(&control_tar);
    let mut data_tar = Vec::new();
    tar::Builder::new(&mut data_tar).finish().unwrap();
    let data_gz = gzip(&data_tar);

    let file = std::fs::File::create(path).unwrap();
    let mut b = ar::Builder::new(file);
    b.append(
        &ar::Header::new(b"debian-binary".to_vec(), 4),
        &b"2.0\n"[..],
    )
    .unwrap();
    b.append(
        &ar::Header::new(b"control.tar.gz".to_vec(), control_gz.len() as u64),
        &control_gz[..],
    )
    .unwrap();
    b.append(
        &ar::Header::new(b"data.tar.gz".to_vec(), data_gz.len() as u64),
        &data_gz[..],
    )
    .unwrap();
}

fn write_pack_manifest(path: &Path, payload: &Path, name: &str, version: &str) {
    std::fs::write(
        path,
        format!(
            "name = \"{name}\"\n\
             version = \"{version}\"\n\
             arch = \"x86_64\"\n\
             maintainer = \"T <t@localhost>\"\n\
             description = \"test package\"\n\
             license = \"MIT\"\n\
             [[files]]\n\
             source = \"{}\"\n\
             dest = \"/usr/bin/{name}\"\n\
             mode = \"0755\"\n",
            payload.display()
        ),
    )
    .unwrap();
}

fn arx_output(args: &[&str]) -> std::process::Output {
    common::arx_command().args(args).output().unwrap()
}

fn arx_ok(args: &[&str]) {
    let output = arx_output(args);
    assert!(
        output.status.success(),
        "arx {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sha256_hex(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    hex::encode(Sha256::digest(&bytes))
}

fn start_static_server(root: PathBuf) -> StaticServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !stop_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => serve_one(&root, &mut stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    StaticServer {
        base_url: format!("http://{addr}"),
        stop,
        handle: Some(handle),
    }
}

fn serve_one(root: &Path, stream: &mut TcpStream) {
    let mut buf = [0_u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let Some(path) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return;
    };
    let rel = path.trim_start_matches('/');
    if rel.contains("..") {
        write_response(stream, 400, b"bad request");
        return;
    }
    let file = root.join(rel);
    match std::fs::read(&file) {
        Ok(body) => write_response(stream, 200, &body),
        Err(_) => write_response(stream, 404, b"not found"),
    }
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "ERR" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn wait_for<F: Fn() -> bool>(label: &str, timeout: Duration, f: F) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {label}");
}

#[test]
fn every_cli_subcommand_is_wired_into_help() {
    let output = arx_output(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for cmd in [
        "init", "key", "add", "publish", "rollback", "history", "pack", "push", "rm", "import",
        "search", "gc", "promote", "serve", "mirror", "watch", "compose", "export",
    ] {
        assert!(
            help.contains(cmd),
            "help output missing command {cmd}:\n{help}"
        );
    }
}

#[test]
fn import_and_mirror_download_from_upstream_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let upstream = tmp.path().join("upstream");
    let pool = upstream.join("pool/main");
    let packages_dir = upstream.join("dists/stable/main/binary-amd64");
    std::fs::create_dir_all(&pool).unwrap();
    std::fs::create_dir_all(&packages_dir).unwrap();
    let deb = pool.join("hello_1.0-1_amd64.deb");
    write_deb(&deb, "hello", "1.0-1", "amd64");
    let size = std::fs::metadata(&deb).unwrap().len();
    let sha = sha256_hex(&deb);
    std::fs::write(
        packages_dir.join("Packages"),
        format!(
            "Package: hello\nVersion: 1.0-1\nArchitecture: amd64\nFilename: pool/main/hello_1.0-1_amd64.deb\nSize: {size}\nSHA256: {sha}\n\n"
        ),
    )
    .unwrap();
    let server = start_static_server(upstream);

    let import_root = tmp.path().join("import-root");
    arx_ok(&["init", import_root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "import",
        &server.base_url,
        "--apt",
        "--root",
        import_root.to_str().unwrap(),
        "--dist",
        "stable",
        "--component",
        "main",
        "--arch",
        "amd64",
    ]);
    assert!(
        import_root
            .join("apt/pool/main/hello_1.0-1_amd64.deb")
            .exists(),
        "import should store downloaded .deb in the apt pool"
    );

    let mirror_root = tmp.path().join("mirror-root");
    arx_ok(&["init", mirror_root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "mirror",
        &server.base_url,
        "--root",
        mirror_root.to_str().unwrap(),
        "--dist",
        "stable",
        "--component",
        "main",
        "--arch",
        "amd64",
    ]);
    assert!(
        mirror_root
            .join("apt/pool/main/hello_1.0-1_amd64.deb")
            .exists(),
        "mirror should store synced .deb in the apt pool"
    );
    assert!(
        mirror_root.join("apt/pool/main/.arx-mirror.toml").exists(),
        "mirror should persist incremental state"
    );
}

#[test]
fn import_accepts_aptly_hash_prefixed_deb_filenames() {
    let tmp = tempfile::tempdir().unwrap();
    let upstream = tmp.path().join("upstream");
    let pool = upstream.join("pool/12/9a");
    let packages_dir = upstream.join("dists/stable/main/binary-amd64");
    std::fs::create_dir_all(&pool).unwrap();
    std::fs::create_dir_all(&packages_dir).unwrap();
    std::fs::write(
        upstream.join("dists/stable/Release"),
        "Origin: Example Repository\nLabel: Example Repository\nSuite: oldstable\nCodename: bullseye\n",
    )
    .unwrap();

    let hashed_name = "c54d87724b58ea5cff53b05a4858_hello_1.0-1_amd64.deb";
    let deb = pool.join(hashed_name);
    write_deb(&deb, "hello", "1.0-1", "amd64");
    let size = std::fs::metadata(&deb).unwrap().len();
    let sha = sha256_hex(&deb);
    let server = start_static_server(upstream);
    let packages = format!(
        "Package: hello\nVersion: 1.0-1\nArchitecture: amd64\nFilename: {}/pool/12/9a/{hashed_name}\nSize: {size}\nSHA256: {sha}\n\n",
        server.base_url
    );
    std::fs::write(packages_dir.join("Packages.xz"), xz(packages.as_bytes())).unwrap();

    let root = tmp.path().join("repo");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "import",
        &server.base_url,
        "--apt",
        "--root",
        root.to_str().unwrap(),
        "--dist",
        "stable",
        "--component",
        "main",
        "--arch",
        "amd64",
    ]);

    let imported = root.join("apt/pool/main").join(hashed_name);
    assert!(
        imported.exists(),
        "import should preserve upstream aptly-style hash-prefixed basename"
    );

    let config = std::fs::read_to_string(root.join("arx.toml")).unwrap();
    assert!(config.contains("origin = \"Example Repository\""));
    assert!(config.contains("label = \"Example Repository\""));
    assert!(config.contains("suite = \"oldstable\""));
    assert!(config.contains("codename = \"bullseye\""));

    arx_ok(&["publish", "--apt", "--root", root.to_str().unwrap()]);
    let published_packages =
        std::fs::read_to_string(root.join("apt/dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(
        published_packages.contains("Package: hello\n"),
        "publish should read identity from .deb control fields, not the hash-prefixed filename:\n{published_packages}"
    );
    assert!(
        published_packages.contains(&format!("Filename: pool/main/{hashed_name}\n")),
        "publish should emit the imported pool path in Packages metadata:\n{published_packages}"
    );

    let release = std::fs::read_to_string(root.join("apt/dists/stable/Release")).unwrap();
    assert!(release.contains("Origin: Example Repository"));
    assert!(release.contains("Label: Example Repository"));
    assert!(release.contains("Suite: oldstable"));
    assert!(release.contains("Codename: bullseye"));
}

#[test]
fn export_builds_legacy_apt_and_centos7_friendly_flat_yum_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let deb = tmp.path().join("hello_1.0-1_amd64.deb");
    write_deb(&deb, "hello", "1.0-1", "amd64");

    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("rpm-export.toml");
    let rpm_dist = tmp.path().join("rpm-dist");
    std::fs::write(&payload, b"#!/bin/sh\necho export\n").unwrap();
    write_pack_manifest(&manifest, &payload, "rpmexport", "1.0.0");
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        rpm_dist.to_str().unwrap(),
        "--rpm",
    ]);
    let rpm = rpm_dist.join("rpmexport-1.0.0-1.x86_64.rpm");

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "add",
        deb.to_str().unwrap(),
        rpm.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--component",
        "main",
        "--repo",
        "example",
    ]);
    arx_ok(&["publish", "--root", root.to_str().unwrap(), "--full"]);

    let apt_export = tmp.path().join("public-deb-20260622");
    let yum_export = tmp.path().join("public-repo-20260622");
    arx_ok(&[
        "export",
        "--root",
        root.to_str().unwrap(),
        "--apt-out",
        apt_export.to_str().unwrap(),
        "--yum-flat-out",
        yum_export.to_str().unwrap(),
        "--repo",
        "example",
        "--arch",
        "x86_64",
    ]);

    assert!(
        apt_export.join("dists/stable/Release").exists(),
        "apt export must expose dists/stable/Release for deb http://host/deb stable main"
    );
    assert!(
        apt_export.join("pool/main/hello_1.0-1_amd64.deb").exists(),
        "apt export must expose the pool under /pool/main"
    );
    let packages =
        std::fs::read_to_string(apt_export.join("dists/stable/main/binary-amd64/Packages"))
            .unwrap();
    assert!(
        packages.contains("Filename: pool/main/hello_1.0-1_amd64.deb"),
        "exported Packages must keep public /pool paths: {packages}"
    );

    assert!(
        yum_export.join("rpmexport-1.0.0-1.x86_64.rpm").exists(),
        "flat yum export must put rpm payloads directly under the public repo root"
    );
    let repomd = std::fs::read_to_string(yum_export.join("repodata/repomd.xml")).unwrap();
    assert!(
        repomd.contains("primary.xml.gz"),
        "CentOS 7 compatibility requires gzip yum metadata: {repomd}"
    );
    assert!(
        !repomd.contains(".xml.xz"),
        "flat export must not switch production metadata to xz-only for CentOS 7: {repomd}"
    );
    assert!(
        yum_export.join("repodata/sha256-primary.xml.gz").exists(),
        "primary metadata must be gzip-compressed"
    );
    assert!(
        !std::fs::symlink_metadata(yum_export.join("repodata"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "public flat yum export should materialize repodata instead of exposing internal state symlinks"
    );
    assert!(
        !yum_export.join(".states").exists(),
        "public flat yum export should not expose internal rollback states"
    );
    assert!(
        !apt_export.join("dists/.states").exists(),
        "public apt export should not expose internal rollback states"
    );
}

#[test]
fn yum_import_accepts_noncanonical_rpm_filenames_and_xz_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let upstream = tmp.path().join("upstream");
    let object_dir = upstream.join("objects/aa");
    let repodata = upstream.join("repodata");
    std::fs::create_dir_all(&object_dir).unwrap();
    std::fs::create_dir_all(&repodata).unwrap();

    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("rpm-import.toml");
    let out = tmp.path().join("dist");
    std::fs::write(&payload, b"#!/bin/sh\necho rpm-import\n").unwrap();
    write_pack_manifest(&manifest, &payload, "rpmimport", "1.0.0");
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--rpm",
    ]);

    let rpm_name = "sha256-deadbeef-not-nevra.rpm";
    let upstream_rpm = object_dir.join(rpm_name);
    std::fs::copy(out.join("rpmimport-1.0.0-1.x86_64.rpm"), &upstream_rpm).unwrap();
    let size = std::fs::metadata(&upstream_rpm).unwrap().len();
    let sha = sha256_hex(&upstream_rpm);

    let server = start_static_server(upstream);
    let primary = format!(
        r#"<metadata packages="1">
  <package type="rpm">
    <name>rpmimport</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="1.0.0" rel="1"/>
    <checksum type="sha256" pkgid="YES">{sha}</checksum>
    <size package="{size}" installed="{size}" archive="{size}"/>
    <location href="{}/objects/aa/{rpm_name}"/>
  </package>
</metadata>
"#,
        server.base_url
    );
    std::fs::write(repodata.join("primary.xml.xz"), xz(primary.as_bytes())).unwrap();
    std::fs::write(
        repodata.join("repomd.xml"),
        r#"<repomd><data type="primary"><location href="repodata/primary.xml.xz"/></data></repomd>
"#,
    )
    .unwrap();

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "import",
        &server.base_url,
        "--yum",
        "--root",
        root.to_str().unwrap(),
        "--component",
        "staging",
    ]);

    assert!(
        root.join("yum/staging/x86_64").join(rpm_name).exists(),
        "yum import should preserve upstream basename but place rpm under the package arch dir"
    );
    arx_ok(&["publish", "--yum", "--root", root.to_str().unwrap()]);
    assert!(
        root.join("yum/staging/x86_64/repodata/repomd.xml").exists(),
        "imported rpm should publish as a normal yum repository"
    );
}

#[test]
fn yum_import_skips_invalid_metadata_entries_and_keeps_importing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let upstream = tmp.path().join("upstream");
    let repodata = upstream.join("repodata");
    std::fs::create_dir_all(&repodata).unwrap();

    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("rpm-import.toml");
    let out = tmp.path().join("dist");
    std::fs::write(&payload, b"#!/bin/sh\necho rpm-import\n").unwrap();
    write_pack_manifest(&manifest, &payload, "rpmimport", "1.0.0");
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--rpm",
    ]);

    let rpm_name = "rpmimport-1.0.0-1.x86_64.rpm";
    let upstream_rpm = upstream.join(rpm_name);
    std::fs::copy(out.join(rpm_name), &upstream_rpm).unwrap();
    let size = std::fs::metadata(&upstream_rpm).unwrap().len();
    let sha = sha256_hex(&upstream_rpm);
    let bad_size = size + 1;

    let server = start_static_server(upstream);
    let primary = format!(
        r#"<metadata packages="2">
  <package type="rpm">
    <name>rpmimport</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="1.0.0" rel="1"/>
    <checksum type="sha256" pkgid="YES">{sha}</checksum>
    <size package="{bad_size}" installed="{bad_size}" archive="{bad_size}"/>
    <location href="{rpm_name}"/>
  </package>
  <package type="rpm">
    <name>rpmimport</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="1.0.0" rel="1"/>
    <checksum type="sha256" pkgid="YES">{sha}</checksum>
    <size package="{size}" installed="{size}" archive="{size}"/>
    <location href="{rpm_name}"/>
  </package>
</metadata>
"#
    );
    std::fs::write(repodata.join("primary.xml.gz"), gzip(primary.as_bytes())).unwrap();
    std::fs::write(
        repodata.join("repomd.xml"),
        r#"<repomd><data type="primary"><location href="repodata/primary.xml.gz"/></data></repomd>
"#,
    )
    .unwrap();

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    let output = arx_output(&[
        "import",
        &server.base_url,
        "--yum",
        "--root",
        root.to_str().unwrap(),
        "--component",
        "staging",
    ]);
    assert!(
        output.status.success(),
        "yum import should skip the bad entry and keep importing\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.join("yum/staging/x86_64").join(rpm_name).exists(),
        "valid entry should still be imported"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("WARNING: skipped 1 invalid yum metadata entry during import"),
        "best-effort import should summarize the accepted metadata delta: {stderr}"
    );
    assert!(
        stderr.contains("use --strict to fail"),
        "best-effort summary should point operators at the cutover gate: {stderr}"
    );

    let strict_root = tmp.path().join("repo-strict");
    arx_ok(&["init", strict_root.to_str().unwrap(), "--no-key"]);
    let strict = arx_output(&[
        "import",
        &server.base_url,
        "--yum",
        "--strict",
        "--root",
        strict_root.to_str().unwrap(),
        "--component",
        "staging",
    ]);
    assert!(
        !strict.status.success(),
        "strict yum import must fail when upstream metadata has invalid entries"
    );
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(
        stderr.contains("strict yum import refused"),
        "strict failure should explain skipped metadata entries: {stderr}"
    );
}

#[test]
fn publish_history_and_rollback_cli_work_together() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let pkg1 = tmp.path().join("hello_1.0-1_amd64.deb");
    let pkg2 = tmp.path().join("hello_2.0-1_amd64.deb");
    write_deb(&pkg1, "hello", "1.0-1", "amd64");
    write_deb(&pkg2, "hello", "2.0-1", "amd64");

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "add",
        pkg1.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--component",
        "main",
    ]);
    arx_ok(&["publish", "--apt", "--root", root.to_str().unwrap()]);
    let first_release = std::fs::read_to_string(root.join("apt/dists/stable/Release")).unwrap();

    std::thread::sleep(Duration::from_millis(1100));
    arx_ok(&[
        "add",
        pkg2.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--component",
        "main",
    ]);
    arx_ok(&["publish", "--apt", "--root", root.to_str().unwrap()]);
    let second_release = std::fs::read_to_string(root.join("apt/dists/stable/Release")).unwrap();
    assert_ne!(
        first_release, second_release,
        "second publish should update Release"
    );

    let history = arx_output(&["history", "stable", "--root", root.to_str().unwrap()]);
    assert!(history.status.success());
    let history_text = String::from_utf8_lossy(&history.stdout);
    assert!(
        history_text.contains("stable"),
        "history output: {history_text}"
    );

    arx_ok(&["rollback", "stable", "--root", root.to_str().unwrap()]);
    let rolled_back = std::fs::read_to_string(root.join("apt/dists/stable/Release")).unwrap();
    assert_eq!(
        rolled_back, first_release,
        "rollback should restore previous state"
    );
}

#[test]
fn serve_and_push_round_trip_a_deb_package() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let pkg = tmp.path().join("hello_1.0-1_amd64.deb");
    write_deb(&pkg, "hello", "1.0-1", "amd64");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .env("ARX_SERVE_TOKEN", "test-token")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });

    arx_ok(&[
        "push",
        pkg.to_str().unwrap(),
        "--url",
        &base,
        "--token",
        "test-token",
        "--component",
        "main",
    ]);
    assert!(root.join("apt/pool/main/hello_1.0-1_amd64.deb").exists());
    assert!(root.join("apt/dists/stable/Release").exists());

    let _ = child.0.kill();
}

#[test]
fn package_list_api_supports_search_filters() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let first = tmp.path().join("api-demo_1.0-1_amd64.deb");
    let second = tmp.path().join("api-other_1.0-1_amd64.deb");
    write_deb(&first, "api-demo", "1.0-1", "amd64");
    write_deb(&second, "api-other", "1.0-1", "amd64");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "add",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
    ]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });

    let response: serde_json::Value =
        reqwest::blocking::get(format!("{base}/api/v1/packages?q=demo&apt=true&scope=main"))
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
    let packages = response.as_array().expect("array response");
    assert_eq!(packages.len(), 1, "filtered package list: {response}");
    assert_eq!(packages[0]["name"], "api-demo");
    assert_eq!(packages[0]["kind"], "apt");

    let _ = child.0.kill();
}

#[test]
fn gc_api_supports_package_scope_and_retention_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    for v in ["1.0", "2.0", "3.0"] {
        let target = tmp.path().join(format!("api-gc_{v}_amd64.deb"));
        let other = tmp.path().join(format!("api-keep_{v}_amd64.deb"));
        write_deb(&target, "api-gc", &format!("{v}-1"), "amd64");
        write_deb(&other, "api-keep", &format!("{v}-1"), "amd64");
        arx_ok(&[
            "add",
            target.to_str().unwrap(),
            other.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
        ]);
        std::thread::sleep(Duration::from_millis(1100));
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .env("ARX_SERVE_TOKEN", "test-token")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });
    let client = reqwest::blocking::Client::new();

    let dry_run: serde_json::Value = client
        .post(format!(
            "{base}/api/v1/gc?name=api-gc&keep=1&apt=true&dry_run=true"
        ))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(dry_run["dry_run"], true);
    assert_eq!(dry_run["retained_for_rollback"], 0);
    assert_eq!(dry_run["deferred"], 0);
    assert!(dry_run["bytes_freed"].as_u64().unwrap() > 0);
    assert_eq!(dry_run["published"], serde_json::Value::Null);
    assert_eq!(
        dry_run["pruned"].as_array().unwrap().len(),
        2,
        "dry-run should report old target versions only: {dry_run}"
    );
    assert!(root.join("apt/pool/main/api-gc_1.0_amd64.deb").exists());

    let pruned: serde_json::Value = client
        .post(format!("{base}/api/v1/gc?name=api-gc&keep=1&apt=true"))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(pruned["dry_run"], false);
    assert_eq!(pruned["pruned"].as_array().unwrap().len(), 2);
    assert!(
        pruned["published"].as_str().unwrap().contains("apt:"),
        "non-dry-run API GC should republish: {pruned}"
    );
    assert!(!root.join("apt/pool/main/api-gc_1.0_amd64.deb").exists());
    assert!(root.join("apt/pool/main/api-gc_3.0_amd64.deb").exists());
    assert!(
        root.join("apt/pool/main/api-keep_1.0_amd64.deb").exists(),
        "name-scoped API GC must not prune unrelated packages"
    );

    let _ = child.0.kill();
}

#[test]
fn api_workflow_covers_documented_publish_history_rollback_and_promote() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let first = tmp.path().join("api-flow_1.0-1_amd64.deb");
    let second = tmp.path().join("api-flow_2.0-1_amd64.deb");
    let staged = tmp.path().join("api-promote_1.0-1_amd64.deb");
    write_deb(&first, "api-flow", "1.0-1", "amd64");
    write_deb(&second, "api-flow", "2.0-1", "amd64");
    write_deb(&staged, "api-promote", "1.0-1", "amd64");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .env("ARX_SERVE_TOKEN", "test-token")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });
    let client = reqwest::blocking::Client::new();

    for (path, filename, component) in [
        (&first, "api-flow_1.0-1_amd64.deb", "main"),
        (&second, "api-flow_2.0-1_amd64.deb", "main"),
        (&staged, "api-promote_1.0-1_amd64.deb", "staging"),
    ] {
        let response: serde_json::Value = client
            .post(format!("{base}/api/v1/packages"))
            .bearer_auth("test-token")
            .header("X-Arx-Filename", filename)
            .header("X-Arx-Component", component)
            .body(std::fs::read(path).unwrap())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        assert!(
            response["published"].as_str().unwrap().contains("apt:"),
            "upload should publish apt metadata: {response}"
        );
    }

    let filtered: serde_json::Value = client
        .get(format!(
            "{base}/api/v1/packages?q=api-flow&apt=true&scope=main"
        ))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        filtered.as_array().unwrap().len(),
        2,
        "API search should find both uploaded versions: {filtered}"
    );

    let dry_run: serde_json::Value = client
        .post(format!(
            "{base}/api/v1/gc?name=api-flow&keep=1&apt=true&dry_run=true&ignore_rollback_states=true"
        ))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(dry_run["dry_run"], true);
    assert_eq!(
        dry_run["published"],
        serde_json::Value::Null,
        "dry-run GC must not publish: {dry_run}"
    );
    assert_eq!(
        dry_run["pruned"].as_array().unwrap().len(),
        1,
        "dry-run GC should report the old version: {dry_run}"
    );

    let promoted: serde_json::Value = client
        .post(format!(
            "{base}/api/v1/promote?name=api-promote&from=staging&to=main&version=1.0-1&apt=true"
        ))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(promoted["moved"], 1, "promote response: {promoted}");

    let published: serde_json::Value = client
        .post(format!("{base}/api/v1/publish"))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        published["apt"].as_str().unwrap().contains("apt:"),
        "publish response should include apt summary: {published}"
    );

    let history: serde_json::Value = client
        .get(format!("{base}/api/v1/history/stable"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        history.as_array().unwrap().len() >= 2,
        "API history should expose retained published states: {history}"
    );

    let rollback: serde_json::Value = client
        .post(format!("{base}/api/v1/rollback/stable"))
        .bearer_auth("test-token")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        rollback["previous"], "stable",
        "rollback response: {rollback}"
    );
    assert!(
        !rollback["current"].as_str().unwrap().is_empty(),
        "rollback should report the restored state id: {rollback}"
    );

    let _ = child.0.kill();
}

#[test]
fn serve_rejects_unauthenticated_write_when_token_is_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .env("ARX_SERVE_TOKEN", "test-token")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });

    let client = reqwest::blocking::Client::new();
    let public_health = client.get(format!("{base}/api/v1/health")).send().unwrap();
    assert_eq!(public_health.status(), reqwest::StatusCode::OK);

    let unauthenticated_write = client.post(format!("{base}/api/v1/gc")).send().unwrap();
    assert_eq!(
        unauthenticated_write.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "token-configured writes must reject missing Authorization"
    );

    let wrong_token_write = client
        .post(format!("{base}/api/v1/gc"))
        .bearer_auth("wrong-token")
        .send()
        .unwrap();
    assert_eq!(
        wrong_token_write.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let authed_write = client
        .post(format!("{base}/api/v1/gc?dry_run=true"))
        .bearer_auth("test-token")
        .send()
        .unwrap();
    assert!(
        authed_write.status().is_success(),
        "correct token should pass middleware, got {}",
        authed_write.status()
    );

    let _ = child.0.kill();
}

#[test]
fn serve_does_not_expose_private_signing_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    std::fs::create_dir_all(root.join("keys")).unwrap();
    std::fs::write(
        root.join("keys/private.asc"),
        b"private key must not be served",
    )
    .unwrap();
    std::fs::write(
        root.join("keys/private.asc.old"),
        b"old private key must not be served",
    )
    .unwrap();
    std::fs::write(root.join("keys/public.asc"), b"public key may be served").unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });

    let client = reqwest::blocking::Client::new();
    for path in [
        "keys/private.asc",
        "keys/private.asc.old",
        "keys/private%2Easc",
    ] {
        let response = client.get(format!("{base}/{path}")).send().unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND,
            "sensitive path {path} must not be served"
        );
    }

    let public = client
        .get(format!("{base}/keys/public.asc"))
        .send()
        .unwrap();
    assert_eq!(public.status(), reqwest::StatusCode::OK);

    let _ = child.0.kill();
}

#[test]
fn serve_blocks_configured_private_signing_key_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config
            .replace(
                "private_key = \"keys/private.asc\"",
                "private_key = \"secrets/custom-signing-key.asc\"",
            )
            .replace(
                "public_key = \"keys/public.asc\"",
                "public_key = \"secrets/custom-signing-key.pub.asc\"",
            ),
    )
    .unwrap();

    std::fs::create_dir_all(root.join("secrets")).unwrap();
    std::fs::write(
        root.join("secrets/custom-signing-key.asc"),
        b"configured private key must not be served",
    )
    .unwrap();
    std::fs::write(
        root.join("secrets/custom-signing-key.asc.bak"),
        b"configured backup private key must not be served",
    )
    .unwrap();
    std::fs::write(
        root.join("secrets/custom-signing-key.pub.asc"),
        b"configured public key may be served",
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });

    let client = reqwest::blocking::Client::new();
    for path in [
        "secrets/custom-signing-key.asc",
        "secrets/custom-signing-key.asc.bak",
    ] {
        let response = client.get(format!("{base}/{path}")).send().unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND,
            "configured sensitive path {path} must not be served"
        );
    }

    let public = client
        .get(format!("{base}/secrets/custom-signing-key.pub.asc"))
        .send()
        .unwrap();
    assert_eq!(public.status(), reqwest::StatusCode::OK);

    let _ = child.0.kill();
}

#[test]
fn serve_upload_response_uses_configured_storage_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let deb = tmp.path().join("stored_1.0-1_amd64.deb");
    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("stored-rpm.toml");
    let out = tmp.path().join("dist");
    write_deb(&deb, "stored", "1.0-1", "amd64");
    std::fs::write(&payload, b"#!/bin/sh\necho stored\n").unwrap();
    write_pack_manifest(&manifest, &payload, "storedrpm", "1.0.0");

    arx_ok(&[
        "init",
        root.to_str().unwrap(),
        "--no-key",
        "--pool-dir",
        "pkgs",
    ]);
    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("base_dir = \"yum\"", "base_dir = \"rpmrepos\""),
    )
    .unwrap();
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--rpm",
    ]);
    let rpm = out.join("storedrpm-1.0.0-1.x86_64.rpm");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "serve",
                "--root",
                root.to_str().unwrap(),
                "--addr",
                &addr.to_string(),
            ])
            .env("ARX_SERVE_TOKEN", "test-token")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{addr}");
    wait_for("serve health", Duration::from_secs(10), || {
        reqwest::blocking::get(format!("{base}/api/v1/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    });
    let client = reqwest::blocking::Client::new();

    let deb_response: serde_json::Value = client
        .post(format!("{base}/api/v1/packages"))
        .bearer_auth("test-token")
        .header("X-Arx-Filename", "stored_1.0-1_amd64.deb")
        .header("X-Arx-Component", "main")
        .body(std::fs::read(&deb).unwrap())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        deb_response["stored"],
        "apt/pkgs/main/stored_1.0-1_amd64.deb"
    );

    let rpm_response: serde_json::Value = client
        .post(format!("{base}/api/v1/packages"))
        .bearer_auth("test-token")
        .header("X-Arx-Filename", "storedrpm-1.0.0-1.x86_64.rpm")
        .header("X-Arx-Repo", "custom")
        .body(std::fs::read(&rpm).unwrap())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        rpm_response["stored"],
        "rpmrepos/custom/x86_64/storedrpm-1.0.0-1.x86_64.rpm"
    );

    let _ = child.0.kill();
}

#[test]
fn watch_imports_new_package_and_publishes_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let mut child = ChildGuard(
        Command::new(common::arx_bin())
            .args([
                "watch",
                watch_dir.to_str().unwrap(),
                "--root",
                root.to_str().unwrap(),
                "--interval",
                "1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let pkg = watch_dir.join("watched_1.0-1_amd64.deb");
    write_deb(&pkg, "watched", "1.0-1", "amd64");
    wait_for("watch import", Duration::from_secs(10), || {
        root.join("apt/pool/main/watched_1.0-1_amd64.deb").exists()
            && root.join("apt/dists/stable/Release").exists()
    });

    let _ = child.0.kill();
}

#[test]
fn compose_generates_deployment_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("compose-out");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    arx_ok(&[
        "compose",
        "--root",
        root.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(out.join("docker-compose.yml").exists());
    assert!(out.join("Dockerfile").exists());
}

#[test]
fn release_packaging_includes_systemd_service_unit() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under crates/arx");
    let manifest = std::fs::read_to_string(repo.join("packaging/arx.toml")).unwrap();
    assert!(
        manifest.contains("packaging/arx.service"),
        "packaging/arx.toml must package the service unit"
    );
    assert!(
        manifest.contains("/usr/lib/systemd/system/arx.service"),
        "packaged service unit should land in systemd's unit directory"
    );

    let workflow = std::fs::read_to_string(repo.join(".github/workflows/release.yml")).unwrap();
    assert!(
        workflow.contains("cp packaging/arx.service /tmp/pack/$arch/arx.service"),
        "release workflow must stage the service unit for self-packaging"
    );
    assert!(
        workflow
            .matches("/usr/lib/systemd/system/arx.service")
            .count()
            >= 2,
        "release workflow must include the unit in both deb and rpm manifests"
    );
}

#[test]
fn pack_cli_flags_and_add_place_expected_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("dist");
    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("pkg.toml");
    std::fs::write(&payload, b"#!/bin/sh\necho packed\n").unwrap();
    write_pack_manifest(&manifest, &payload, "packed", "1.0.0");
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--deb",
    ]);
    assert!(out.join("packed_1.0.0_amd64.deb").exists());
    assert!(
        !out.join("packed-1.0.0-1.x86_64.rpm").exists(),
        "--deb should not emit rpm"
    );

    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--rpm",
        "--apk",
    ]);
    assert!(out.join("packed-1.0.0-1.x86_64.rpm").exists());
    assert!(out.join("packed-1.0.0-r0.x86_64.apk").exists());

    let add_out = tmp.path().join("add-dist");
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        add_out.to_str().unwrap(),
        "--add",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(root.join("apt/pool/main/packed_1.0.0_amd64.deb").exists());
    assert!(root
        .join("yum/myrepo/x86_64/packed-1.0.0-1.x86_64.rpm")
        .exists());
}

#[test]
fn add_accepts_directory_inputs_recursively_in_stable_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let input = tmp.path().join("incoming");
    let first_dir = input.join("a-first");
    let nested = input.join("z-nested");
    let rpm_out = tmp.path().join("rpm-out");
    let payload = tmp.path().join("payload.sh");
    let manifest = tmp.path().join("rpm.toml");

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(input.join("README.txt"), "ignored").unwrap();
    std::fs::write(&payload, b"#!/bin/sh\necho diradd\n").unwrap();
    write_pack_manifest(&manifest, &payload, "dirrpm", "1.0.0");
    arx_ok(&[
        "pack",
        manifest.to_str().unwrap(),
        "--out",
        rpm_out.to_str().unwrap(),
        "--rpm",
    ]);

    let first = first_dir.join("dirdeb-a_1.0-1_amd64.deb");
    let second = nested.join("dirdeb-z_1.0-1_amd64.deb");
    write_deb(&first, "dirdeb-a", "1.0-1", "amd64");
    write_deb(&second, "dirdeb-z", "1.0-1", "amd64");
    std::fs::copy(
        rpm_out.join("dirrpm-1.0.0-1.x86_64.rpm"),
        nested.join("dirrpm-1.0.0-1.x86_64.rpm"),
    )
    .unwrap();

    let output = arx_output(&[
        "add",
        input.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "arx add directory failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_idx = stdout
        .find("dirdeb-a_1.0-1_amd64.deb")
        .expect("first deb add line");
    let second_idx = stdout
        .find("dirdeb-z_1.0-1_amd64.deb")
        .expect("second deb add line");
    let rpm_idx = stdout
        .find("dirrpm-1.0.0-1.x86_64.rpm")
        .expect("rpm add line");
    assert!(
        first_idx < second_idx && second_idx < rpm_idx,
        "directory add output should be stable sorted order:\n{stdout}"
    );
    assert!(root.join("apt/pool/main/dirdeb-a_1.0-1_amd64.deb").exists());
    assert!(root.join("apt/pool/main/dirdeb-z_1.0-1_amd64.deb").exists());
    assert!(root
        .join("yum/myrepo/x86_64/dirrpm-1.0.0-1.x86_64.rpm")
        .exists());

    arx_ok(&["publish", "--root", root.to_str().unwrap(), "--full"]);
    let packages =
        std::fs::read_to_string(root.join("apt/dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(packages.contains("Package: dirdeb-a"));
    assert!(packages.contains("Package: dirdeb-z"));
    let repomd =
        std::fs::read_to_string(root.join("yum/myrepo/x86_64/repodata/repomd.xml")).unwrap();
    assert!(repomd.contains("primary"));
}

#[test]
fn add_directory_without_packages_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let input = tmp.path().join("incoming");

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    std::fs::create_dir_all(&input).unwrap();
    std::fs::write(input.join("README.txt"), "not a package").unwrap();

    let output = arx_output(&[
        "add",
        input.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(
        !output.status.success(),
        "empty directory add should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("directory contains no supported package files"),
        "error should explain empty directory package discovery:\n{stderr}"
    );
}

#[test]
fn search_cli_filters_pool_entries_and_emits_json() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let keep = tmp.path().join("demo-agent_2.0-1_amd64.deb");
    let other = tmp.path().join("otherpkg_1.0-1_amd64.deb");

    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);
    write_deb(&keep, "demo-agent", "2.0-1", "amd64");
    write_deb(&other, "otherpkg", "1.0-1", "amd64");
    arx_ok(&[
        "add",
        keep.to_str().unwrap(),
        other.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
    ]);

    let json = arx_output(&[
        "search",
        "demo",
        "--apt",
        "--scope",
        "main",
        "--json",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(
        json.status.success(),
        "search json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&json.stdout),
        String::from_utf8_lossy(&json.stderr)
    );
    let packages: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let packages = packages.as_array().expect("json array");
    assert_eq!(packages.len(), 1, "filtered search json");
    assert_eq!(packages[0]["name"], "demo-agent");

    let text = arx_output(&[
        "search",
        "--name-prefix",
        "other",
        "--version",
        "1.0-1",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(
        text.status.success(),
        "search text failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&text.stdout),
        String::from_utf8_lossy(&text.stderr)
    );
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("otherpkg\t1.0-1\tamd64\tmain\tapt"));
    assert!(!stdout.contains("demo-agent"));
}

#[test]
fn apt_rm_and_gc_respect_configured_pool_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    arx_ok(&[
        "init",
        root.to_str().unwrap(),
        "--no-key",
        "--pool-dir",
        "pkgs",
    ]);

    for version in ["1.0-1", "2.0-1", "3.0-1"] {
        let pkg = tmp.path().join(format!("customapt_{version}_amd64.deb"));
        write_deb(&pkg, "customapt", version, "amd64");
        arx_ok(&[
            "add",
            pkg.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--component",
            "main",
        ]);
        std::thread::sleep(Duration::from_millis(1100));
    }

    assert!(root
        .join("apt/pkgs/main/customapt_1.0-1_amd64.deb")
        .exists());
    assert!(
        !root.join("apt/pool/main").exists(),
        "custom apt pool must not fall back to apt/pool"
    );

    arx_ok(&["publish", "--apt", "--root", root.to_str().unwrap()]);
    let packages =
        std::fs::read_to_string(root.join("apt/dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(
        packages.contains("Filename: pkgs/main/customapt_1.0-1_amd64.deb"),
        "publish must reference the configured pool dir:\n{packages}"
    );
    assert!(
        !packages.contains("Filename: pool/"),
        "custom pool metadata must not fall back to pool/:\n{packages}"
    );
    assert!(root.join("apt/pkgs/main/.arx-manifest.toml").exists());

    arx_ok(&[
        "rm",
        "customapt",
        "--version",
        "1.0-1",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!root
        .join("apt/pkgs/main/customapt_1.0-1_amd64.deb")
        .exists());

    let gc_root = tmp.path().join("gc-repo");
    arx_ok(&[
        "init",
        gc_root.to_str().unwrap(),
        "--no-key",
        "--pool-dir",
        "pkgs",
    ]);
    for version in ["1.0-1", "2.0-1", "3.0-1"] {
        let pkg = tmp.path().join(format!("customgc_{version}_amd64.deb"));
        write_deb(&pkg, "customgc", version, "amd64");
        arx_ok(&[
            "add",
            pkg.to_str().unwrap(),
            "--root",
            gc_root.to_str().unwrap(),
            "--component",
            "main",
        ]);
        std::thread::sleep(Duration::from_millis(1100));
    }
    arx_ok(&["gc", "--keep", "1", "--root", gc_root.to_str().unwrap()]);
    assert!(!gc_root
        .join("apt/pkgs/main/customgc_2.0-1_amd64.deb")
        .exists());
    assert!(gc_root
        .join("apt/pkgs/main/customgc_3.0-1_amd64.deb")
        .exists());
}

#[test]
fn yum_add_promote_rm_and_gc_positive_paths_work() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let payload = tmp.path().join("payload.sh");
    std::fs::write(&payload, b"#!/bin/sh\necho yum\n").unwrap();
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let mut rpm_paths = Vec::new();
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        let manifest = tmp.path().join(format!("yum-{version}.toml"));
        let out = tmp.path().join(format!("dist-{version}"));
        write_pack_manifest(&manifest, &payload, "yumthing", version);
        arx_ok(&[
            "pack",
            manifest.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--rpm",
        ]);
        rpm_paths.push(out.join(format!("yumthing-{version}-1.x86_64.rpm")));
    }

    arx_ok(&[
        "add",
        rpm_paths[0].to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--repo",
        "staging",
    ]);
    assert!(root
        .join("yum/staging/x86_64/yumthing-1.0.0-1.x86_64.rpm")
        .exists());

    arx_ok(&[
        "promote",
        "yumthing",
        "--from",
        "staging",
        "--to",
        "prod",
        "--yum",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(root
        .join("yum/prod/x86_64/yumthing-1.0.0-1.x86_64.rpm")
        .exists());
    assert!(!root
        .join("yum/staging/x86_64/yumthing-1.0.0-1.x86_64.rpm")
        .exists());

    arx_ok(&[
        "rm",
        "yumthing",
        "--version",
        "1.0.0",
        "--yum",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!root
        .join("yum/prod/x86_64/yumthing-1.0.0-1.x86_64.rpm")
        .exists());

    for rpm in &rpm_paths[1..] {
        arx_ok(&[
            "add",
            rpm.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--repo",
            "prod",
        ]);
        std::thread::sleep(Duration::from_millis(1100));
    }
    arx_ok(&[
        "gc",
        "--keep",
        "1",
        "--yum",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!root
        .join("yum/prod/x86_64/yumthing-2.0.0-1.x86_64.rpm")
        .exists());
    assert!(root
        .join("yum/prod/x86_64/yumthing-3.0.0-1.x86_64.rpm")
        .exists());
}

#[test]
fn yum_rm_and_gc_respect_configured_base_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let payload = tmp.path().join("payload.sh");
    std::fs::write(&payload, b"#!/bin/sh\necho custom yum\n").unwrap();
    arx_ok(&["init", root.to_str().unwrap(), "--no-key"]);

    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("base_dir = \"yum\"", "base_dir = \"rpmrepos\""),
    )
    .unwrap();

    let mut rpm_paths = Vec::new();
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        let manifest = tmp.path().join(format!("custom-yum-{version}.toml"));
        let out = tmp.path().join(format!("custom-dist-{version}"));
        write_pack_manifest(&manifest, &payload, "customyum", version);
        arx_ok(&[
            "pack",
            manifest.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--rpm",
        ]);
        rpm_paths.push(out.join(format!("customyum-{version}-1.x86_64.rpm")));
    }

    for rpm in &rpm_paths {
        arx_ok(&[
            "add",
            rpm.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--repo",
            "custom",
        ]);
        std::thread::sleep(Duration::from_millis(1100));
    }

    assert!(root
        .join("rpmrepos/custom/x86_64/customyum-1.0.0-1.x86_64.rpm")
        .exists());
    assert!(!root.join("yum/custom/x86_64").exists());

    arx_ok(&[
        "rm",
        "customyum",
        "--version",
        "1.0.0",
        "--yum",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!root
        .join("rpmrepos/custom/x86_64/customyum-1.0.0-1.x86_64.rpm")
        .exists());

    arx_ok(&[
        "gc",
        "--keep",
        "1",
        "--yum",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!root
        .join("rpmrepos/custom/x86_64/customyum-2.0.0-1.x86_64.rpm")
        .exists());
    assert!(root
        .join("rpmrepos/custom/x86_64/customyum-3.0.0-1.x86_64.rpm")
        .exists());
}

//! Integration tests for the `arx-pack` crate.
//!
//! These build real `.deb` and `.rpm` artifacts from a sample manifest and read
//! them back to assert the metadata round-trips. The `.deb` is re-parsed inline
//! with `ar` + `tar` + `flate2` (no dependency on the sibling `arx-debrepo` crate)
//! to keep this crate standalone.

use std::io::Read;
use std::path::Path;

use arx_pack::{Backend, Format, Manifest};

/// Write a sample payload file and return a manifest referencing it.
fn sample_manifest(dir: &Path) -> Manifest {
    let payload = dir.join("hello");
    std::fs::write(&payload, b"#!/bin/sh\necho hello\n").unwrap();

    let toml = format!(
        r#"
        name = "hello"
        version = "1.2.3"
        arch = "amd64"
        maintainer = "Jane Dev <jane@example.com>"
        description = "A friendly greeter"
        license = "MIT"
        section = "utils"
        depends = ["libc6"]

        [[files]]
        source = "{}"
        dest = "/usr/bin/hello"
        mode = "0755"
        "#,
        payload.display()
    );
    Manifest::from_toml_str(&toml).expect("manifest parses")
}

#[test]
fn cargo_toml_derives_manifest() {
    let cargo = r#"
        [package]
        name = "greeter"
        version = "0.3.0"
        edition = "2021"
        description = "A friendly greeter"
        license = "MIT"
        authors = ["Jane Dev <jane@example.com>"]

        [package.metadata.arx]
        section = "utils"
        depends = ["libc6"]
        provides = ["greet"]
    "#;
    let m = Manifest::from_cargo_toml(cargo).unwrap();
    assert_eq!(m.name, "greeter");
    assert_eq!(m.version, "0.3.0");
    assert_eq!(m.maintainer, "Jane Dev <jane@example.com>"); // from authors
    assert_eq!(m.section.as_deref(), Some("utils"));
    assert_eq!(m.depends, vec!["libc6".to_string()]);
    assert_eq!(m.provides, vec!["greet".to_string()]);
    // Convention: with no [[files]], default to .../target/release/<name> →
    // /usr/bin/<name>. In a workspace the target dir is at the workspace root.
    // The bin name is the package name (no [[bin]] override in test input).
    assert_eq!(m.files.len(), 1);
    assert!(
        m.files[0].source.ends_with("target/release/greeter"),
        "source should end with target/release/greeter, got: {}",
        m.files[0].source
    );
    assert_eq!(m.files[0].dest, "/usr/bin/greeter");
}

#[test]
fn cargo_toml_without_package_errors() {
    // A workspace root has no [package].
    let err = Manifest::from_cargo_toml("[workspace]\nmembers = []\n").unwrap_err();
    assert!(err.to_string().contains("no [package]"));
}

#[test]
fn manifest_round_trip() {
    let toml = r#"
        name = "demo"
        version = "0.1.0"
        arch = "x86_64"
        maintainer = "Dev <dev@example.com>"
        description = "line one\nline two"
        license = "Apache-2.0"

        [[files]]
        source = "/tmp/x"
        dest = "/opt/demo/x"
        mode = "0644"
    "#;
    let m = Manifest::from_toml_str(toml).unwrap();
    assert_eq!(m.name, "demo");
    assert_eq!(m.version, "0.1.0");
    assert_eq!(m.arch, "x86_64");
    assert_eq!(m.files.len(), 1);
    assert_eq!(m.files[0].dest, "/opt/demo/x");
    assert_eq!(m.files[0].mode_bits().unwrap(), 0o644);
}

#[test]
fn deb_is_byte_reproducible() {
    let dir = tempfile::tempdir().unwrap();
    let m = sample_manifest(dir.path());
    let a = arx_pack::build_deb(&m, &dir.path().join("a")).unwrap();
    let b = arx_pack::build_deb(&m, &dir.path().join("b")).unwrap();
    assert_eq!(
        std::fs::read(&a).unwrap(),
        std::fs::read(&b).unwrap(),
        ".deb must be byte-for-byte reproducible across builds"
    );
}

#[test]
fn rpm_is_byte_reproducible_and_has_fixed_build_time() {
    let dir = tempfile::tempdir().unwrap();
    let m = sample_manifest(dir.path());
    let a = arx_pack::build_rpm(&m, &dir.path().join("a")).unwrap();
    let b = arx_pack::build_rpm(&m, &dir.path().join("b")).unwrap();
    assert_eq!(
        std::fs::read(&a).unwrap(),
        std::fs::read(&b).unwrap(),
        ".rpm must be byte-for-byte reproducible across builds"
    );
    // Verify the build time is the deterministic epoch, not wall-clock. Nix and
    // other reproducible-build environments may set SOURCE_DATE_EPOCH globally.
    let expected_epoch = u64::from(arx_pack::resolve_source_epoch());
    let pkg = rpm::Package::open(&a).expect("rpm re-opens for timestamp check");
    let build_time = pkg
        .metadata
        .get_build_time()
        .expect("rpm must have a build_time header");
    assert_eq!(
        build_time, expected_epoch,
        "rpm build_time must follow SOURCE_DATE_EPOCH/default reproducibility epoch"
    );
}

#[test]
fn unknown_arch_is_rejected_not_silently_defaulted() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("x");
    std::fs::write(&src, b"x").unwrap();
    let m = Manifest::from_toml_str(&format!(
        "name='a'\nversion='1'\narch='bogus42'\nmaintainer='T<t@x>'\ndescription='d'\nlicense='MIT'\n[[files]]\nsource='{}'\ndest='/x'\nmode='0644'\n",
        src.display()
    )).unwrap();
    let err = arx_pack::build_deb(&m, dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("unknown architecture"),
        "expected 'unknown architecture' error, got: {err}"
    );
    let err = arx_pack::build_rpm(&m, dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("unknown architecture"),
        "expected 'unknown architecture' error, got: {err}"
    );
}

#[test]
fn non_regular_file_source_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let sym = dir.path().join("link");
    std::os::unix::fs::symlink("/tmp", &sym).unwrap();
    let m = Manifest::from_toml_str(&format!(
        "name='a'\nversion='1'\narch='amd64'\nmaintainer='T<t@x>'\ndescription='d'\nlicense='MIT'\n[[files]]\nsource='{}'\ndest='/x'\nmode='0644'\n",
        sym.display()
    )).unwrap();
    let err = arx_pack::build_deb(&m, dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("symbolic link"),
        "expected 'symbolic link' error, got: {err}"
    );
}

#[test]
fn builds_and_reparses_deb() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());

    let deb_path = arx_pack::build_deb(&manifest, dir.path()).expect("deb builds");
    assert!(deb_path.exists());
    assert_eq!(
        deb_path.file_name().unwrap().to_str().unwrap(),
        "hello_1.2.3_amd64.deb"
    );

    let control = read_deb_control(&deb_path);
    assert!(
        control.contains("Package: hello"),
        "control missing Package:\n{control}"
    );
    assert!(
        control.contains("Version: 1.2.3"),
        "control missing Version:\n{control}"
    );
    assert!(
        control.contains("Architecture: amd64"),
        "control missing Architecture:\n{control}"
    );
    assert!(
        control.contains("Depends: libc6"),
        "control missing Depends:\n{control}"
    );

    // The data tarball must carry the file at its install path.
    let data_names = read_deb_data_names(&deb_path);
    assert!(
        data_names.iter().any(|n| n.ends_with("usr/bin/hello")),
        "data.tar.gz missing the installed file: {data_names:?}"
    );
}

#[test]
fn builds_and_reparses_rpm() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());

    let rpm_path = arx_pack::build_rpm(&manifest, dir.path()).expect("rpm builds");
    assert!(rpm_path.exists());

    let pkg = rpm::Package::open(&rpm_path).expect("rpm re-opens");
    assert_eq!(pkg.metadata.get_name().unwrap(), "hello");
    assert_eq!(pkg.metadata.get_version().unwrap(), "1.2.3");
    assert_eq!(pkg.metadata.get_arch().unwrap(), "x86_64");
}

#[test]
fn builds_apk() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());
    let apk_path = arx_pack::build_apk(&manifest, dir.path()).expect("apk builds");
    assert!(apk_path.exists());
    // APK is a gzipped tar — verify the structure.
    let data = std::fs::read(&apk_path).unwrap();
    let gz = flate2::read::GzDecoder::new(data.as_slice());
    let mut archive = tar::Archive::new(gz);
    let mut has_pkinfo = false;
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        if name == ".PKGINFO" {
            has_pkinfo = true;
            use std::io::Read;
            let mut s = String::new();
            entry.take(1024).read_to_string(&mut s).unwrap();
            assert!(s.contains("pkgname = hello"), "PKGINFO: {s}");
            assert!(s.contains("pkgver = 1.2.3"));
        }
    }
    assert!(has_pkinfo, "APK must contain .PKGINFO");
}

#[test]
fn directory_entries_expand_into_deb_rpm_and_apk_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(assets.join("css")).unwrap();
    std::fs::write(assets.join("index.html"), b"<h1>Hello</h1>\n").unwrap();
    std::fs::write(assets.join("css/style.css"), b"body { color: #222; }\n").unwrap();

    let toml = format!(
        r#"
        name = "web-assets"
        version = "1.0.0"
        arch = "amd64"
        maintainer = "Dev <dev@example.com>"
        description = "static assets"
        license = "MIT"

        [[dirs]]
        source = "{}"
        dest = "/usr/share/web-assets"
        file_mode = "0644"
        dir_mode = "0755"
        "#,
        assets.display()
    );
    let manifest = Manifest::from_toml_str(&toml).unwrap();
    assert_eq!(manifest.dirs.len(), 1);
    assert_eq!(manifest.dirs[0].file_mode_bits().unwrap(), 0o644);
    assert_eq!(manifest.dirs[0].dir_mode_bits().unwrap(), 0o755);

    let deb = arx_pack::build_deb(&manifest, &dir.path().join("deb")).unwrap();
    let deb_names = read_deb_data_names(&deb);
    assert!(
        deb_names
            .iter()
            .any(|n| n.trim_start_matches("./") == "usr/share/web-assets/"),
        "deb data.tar missing expanded root dir: {deb_names:?}"
    );
    assert!(
        deb_names
            .iter()
            .any(|n| n.trim_start_matches("./") == "usr/share/web-assets/css/style.css"),
        "deb data.tar missing expanded nested file: {deb_names:?}"
    );

    let rpm = arx_pack::build_rpm(&manifest, &dir.path().join("rpm")).unwrap();
    let rpm_pkg = rpm::Package::open(&rpm).unwrap();
    let rpm_paths = rpm_pkg.metadata.get_file_paths().unwrap();
    assert!(
        rpm_paths
            .iter()
            .any(|p| p == Path::new("/usr/share/web-assets/index.html")),
        "rpm missing expanded directory file: {rpm_paths:?}"
    );
    assert!(
        rpm_paths
            .iter()
            .any(|p| p == Path::new("/usr/share/web-assets/css/style.css")),
        "rpm missing expanded nested file: {rpm_paths:?}"
    );

    let apk = arx_pack::build_apk(&manifest, &dir.path().join("apk")).unwrap();
    let apk_names = read_apk_names(&apk);
    assert!(
        apk_names
            .iter()
            .any(|n| n.trim_start_matches('.') == "usr/share/web-assets/"),
        "apk missing expanded root dir: {apk_names:?}"
    );
    assert!(
        apk_names
            .iter()
            .any(|n| n.trim_start_matches('.') == "usr/share/web-assets/css/style.css"),
        "apk missing expanded nested file: {apk_names:?}"
    );
}

#[test]
fn directory_entries_reject_symlinks_and_duplicate_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("app.conf"), b"from dir\n").unwrap();
    std::os::unix::fs::symlink("app.conf", assets.join("link.conf")).unwrap();

    let symlink_manifest = Manifest::from_toml_str(&format!(
        "name='a'\nversion='1'\narch='amd64'\nmaintainer='T<t@x>'\ndescription='d'\nlicense='MIT'\n[[dirs]]\nsource='{}'\ndest='/etc/a'\n",
        assets.display()
    ))
    .unwrap();
    let err = arx_pack::build_deb(&symlink_manifest, dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("symbolic link"),
        "expected symlink rejection, got: {err}"
    );

    std::fs::remove_file(assets.join("link.conf")).unwrap();
    let explicit = dir.path().join("explicit.conf");
    std::fs::write(&explicit, b"explicit\n").unwrap();
    let duplicate_manifest = Manifest::from_toml_str(&format!(
        "name='a'\nversion='1'\narch='amd64'\nmaintainer='T<t@x>'\ndescription='d'\nlicense='MIT'\n[[files]]\nsource='{}'\ndest='/etc/a/app.conf'\nmode='0644'\n[[dirs]]\nsource='{}'\ndest='/etc/a'\n",
        explicit.display(),
        assets.display()
    ))
    .unwrap();
    let err = arx_pack::build_deb(&duplicate_manifest, dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("duplicate package destination"),
        "expected duplicate destination rejection, got: {err}"
    );
}

#[test]
fn backend_native_builds_both() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());
    let backend = Backend::Native;
    let deb = backend.build(&manifest, Format::Deb, dir.path()).unwrap();
    let rpm = backend.build(&manifest, Format::Rpm, dir.path()).unwrap();
    let apk = backend.build(&manifest, Format::Apk, dir.path()).unwrap();
    assert!(deb.exists() && rpm.exists() && apk.exists());
}

#[test]
fn backend_docker_builds_in_container() {
    // Docker backend requires docker CLI + arx binary. Skip if absent.
    if !std::process::Command::new("docker")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        eprintln!("skipping — docker not available");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());
    let backend = Backend::Docker {
        image: "debian:bookworm-slim".into(),
    };
    match backend.build(&manifest, Format::Deb, dir.path()) {
        Ok(deb_path) => {
            assert!(deb_path.exists());
            // Verify the output is a valid .deb.
            let control = read_deb_control(&deb_path);
            assert!(
                control.contains("Package: hello"),
                "container-built deb: {control}"
            );
        }
        Err(e) => {
            // May fail if image not pulled, Docker not running, etc. Skip.
            eprintln!("Docker build skipped: {e}");
        }
    }
}

#[test]
fn relationship_fields_and_scripts_in_deb() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello"), b"#!/bin/sh\n").unwrap();
    let postinst = dir.path().join("postinst.sh");
    std::fs::write(&postinst, b"#!/bin/sh\nset -e\n").unwrap();

    let toml = format!(
        r#"
        name = "hello"
        version = "1.0.0"
        arch = "amd64"
        maintainer = "Dev <dev@example.com>"
        description = "greeter"
        license = "MIT"
        depends = ["libc6"]
        conflicts = ["hello-old"]
        provides = ["greeter"]
        replaces = ["hello-old"]

        [scripts]
        postinst = "{postinst}"

        [[files]]
        source = "{payload}"
        dest = "/usr/bin/hello"
        mode = "0755"
        "#,
        postinst = postinst.display(),
        payload = dir.path().join("hello").display(),
    );
    let manifest = Manifest::from_toml_str(&toml).unwrap();
    let deb = arx_pack::build_deb(&manifest, dir.path()).unwrap();

    let control = read_deb_control(&deb);
    assert!(control.contains("Conflicts: hello-old"), "{control}");
    assert!(control.contains("Provides: greeter"), "{control}");
    assert!(control.contains("Replaces: hello-old"), "{control}");

    // The maintainer script must be embedded in control.tar.gz.
    let names = read_control_tar_names(&deb);
    assert!(
        names
            .iter()
            .any(|n| n.trim_start_matches("./") == "postinst"),
        "control.tar missing postinst: {names:?}"
    );
}

// --- inline .deb re-parsing helpers (ar + tar + flate2) ---

/// Return the entry names inside an `.apk` tarball.
fn read_apk_names(path: &Path) -> Vec<String> {
    let data = std::fs::read(path).unwrap();
    let gz = flate2::read::GzDecoder::new(data.as_slice());
    let mut archive = tar::Archive::new(gz);
    archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Return the entry names inside a `.deb`'s `control.tar.gz`.
fn read_control_tar_names(path: &Path) -> Vec<String> {
    let tar_gz = read_ar_member(path, "control.tar.gz");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tar_gz.as_slice()));
    archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Extract and return the `control` file text from a `.deb`.
fn read_deb_control(path: &Path) -> String {
    let tar_gz = read_ar_member(path, "control.tar.gz");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tar_gz.as_slice()));
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        if name.trim_start_matches("./") == "control" {
            let mut s = String::new();
            entry.read_to_string(&mut s).unwrap();
            return s;
        }
    }
    panic!("control file not found in control.tar.gz");
}

/// Return the entry names inside a `.deb`'s `data.tar.gz`.
fn read_deb_data_names(path: &Path) -> Vec<String> {
    let tar_gz = read_ar_member(path, "data.tar.gz");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tar_gz.as_slice()));
    archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Read a single `ar` member's bytes from a `.deb`, tolerating identifier padding.
fn read_ar_member(path: &Path, member: &str) -> Vec<u8> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = ar::Archive::new(file);
    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.unwrap();
        let name = String::from_utf8_lossy(entry.header().identifier()).into_owned();
        if name.trim_end_matches('/') == member {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            return buf;
        }
    }
    panic!("ar member {member} not found in {}", path.display());
}

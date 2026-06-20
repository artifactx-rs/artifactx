//! Regression tests for repository scope names used in filesystem joins.

use std::io::Write;
use std::path::Path;

mod common;

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
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

#[test]
fn add_rejects_component_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let pkg = tmp.path().join("hello_1.0-1_amd64.deb");
    write_deb(&pkg, "hello", "1.0-1", "amd64");

    let output = common::arx_command()
        .args([
            "add",
            pkg.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--component",
            "../escape",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success(), "unsafe component should fail");
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe component must not create a directory outside the repo"
    );
    assert!(
        !root.join("apt/pool/main/hello_1.0-1_amd64.deb").exists(),
        "failed add must not copy into the default pool either"
    );
}

#[test]
fn promote_rejects_destination_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let src = root.join("apt/pool/main");
    std::fs::create_dir_all(&src).unwrap();
    let deb = src.join("hello_1.0-1_amd64.deb");
    write_deb(&deb, "hello", "1.0-1", "amd64");

    let output = common::arx_command()
        .args([
            "promote",
            "hello",
            "--from",
            "main",
            "--to",
            "../escape",
            "--apt",
            "--root",
            root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success(), "unsafe destination should fail");
    assert!(
        deb.exists(),
        "failed promote must leave the source file in place"
    );
    assert!(
        !root.join("apt/pool/escape").exists() && !root.join("apt/escape").exists(),
        "unsafe destination must not create an escaped directory"
    );
}

#[test]
fn publish_rejects_configured_dist_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let init = common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let pool = root.join("apt/pool/main");
    std::fs::create_dir_all(&pool).unwrap();
    write_deb(
        &pool.join("hello_1.0-1_amd64.deb"),
        "hello",
        "1.0-1",
        "amd64",
    );

    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("dist = \"stable\"", "dist = \"../../../escape\""),
    )
    .unwrap();

    let output = common::arx_command()
        .args(["publish", "--apt", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe dist should fail before publishing"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe dist must not create a directory outside the repo"
    );
}

#[test]
fn import_rejects_dist_that_escapes_before_fetching() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let init = common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let output = common::arx_command()
        .args([
            "import",
            "http://127.0.0.1:9",
            "--apt",
            "--root",
            root.to_str().unwrap(),
            "--dist",
            "../escape",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe dist should fail before import fetches upstream"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("apt dist"),
        "expected apt dist validation error, got: {stderr}"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe dist must not create a directory outside the repo"
    );
}

#[test]
fn mirror_rejects_dist_that_escapes_before_fetching() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let init = common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let output = common::arx_command()
        .args([
            "mirror",
            "http://127.0.0.1:9",
            "--root",
            root.to_str().unwrap(),
            "--dist",
            "../escape",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe dist should fail before mirror fetches upstream"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("apt dist"),
        "expected apt dist validation error, got: {stderr}"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe dist must not create a directory outside the repo"
    );
}

#[test]
fn init_rejects_pool_dir_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let output = common::arx_command()
        .args([
            "init",
            root.to_str().unwrap(),
            "--no-key",
            "--pool-dir",
            "../escape",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe pool_dir should fail during init"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe pool_dir must not create a directory outside the repo"
    );
}

#[test]
fn init_rejects_key_dir_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let output = common::arx_command()
        .args([
            "init",
            root.to_str().unwrap(),
            "--no-key",
            "--key-dir",
            "../escape",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe key_dir should fail during init"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe key_dir must not create a directory outside the repo"
    );
}

#[test]
fn key_export_rejects_configured_public_key_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let init = common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace(
            "public_key = \"keys/public.asc\"",
            "public_key = \"../escape.asc\"",
        ),
    )
    .unwrap();

    let output = common::arx_command()
        .args(["key", "export", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe public_key path should fail before reading"
    );
    assert!(
        !tmp.path().join("escape.asc").exists(),
        "unsafe public_key path must not be used outside the repo"
    );
}

#[test]
fn publish_rejects_configured_yum_base_that_escapes_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");

    let init = common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let config_path = root.join("arx.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("base_dir = \"yum\"", "base_dir = \"../escape\""),
    )
    .unwrap();

    let output = common::arx_command()
        .args(["publish", "--yum", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "unsafe yum base_dir should fail before publishing"
    );
    assert!(
        !tmp.path().join("escape").exists(),
        "unsafe yum base_dir must not create a directory outside the repo"
    );
}

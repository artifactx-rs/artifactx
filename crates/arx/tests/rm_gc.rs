//! CLI integration tests for `arx rm` and `arx gc` against the built binary.

use std::io::Write;
use std::path::Path;
mod common;

/// Build a minimal `.deb` (ar + control.tar.gz + empty data.tar.gz).
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

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap();
    out
}

fn arx(root: &Path, args: &[&str]) {
    let status = common::arx_command()
        .args(args)
        .arg("--root")
        .arg(root)
        .status()
        .unwrap();
    assert!(status.success(), "arx {args:?} failed");
}

/// Add three versions of one package, each newer by mtime, into the apt pool.
fn seed_three_versions(pool: &Path) {
    std::fs::create_dir_all(pool).unwrap();
    for v in ["1.0", "2.0", "3.0"] {
        let p = pool.join(format!("hello_{v}_amd64.deb"));
        write_deb(&p, "hello", &format!("{v}-1"), "amd64");
        // Stagger mtimes so retention order is deterministic.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        filetime_now(&p);
    }
}

/// Touch the file so its mtime reflects insertion order.
fn filetime_now(p: &Path) {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(std::time::SystemTime::now()).unwrap();
}

#[test]
fn gc_keeps_newest_n() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pool = root.join("apt/pool/main");
    seed_three_versions(&pool);

    arx(root, &["gc", "--keep", "2", "--apt"]);

    let remaining: Vec<String> = std::fs::read_dir(&pool)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(remaining.len(), 2, "got {remaining:?}");
    assert!(
        !remaining.iter().any(|f| f.contains("1.0")),
        "oldest should be pruned: {remaining:?}"
    );
    assert!(remaining.iter().any(|f| f.contains("3.0")));
}

#[test]
fn gc_dry_run_deletes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pool = root.join("apt/pool/main");
    seed_three_versions(&pool);

    arx(root, &["gc", "--keep", "1", "--dry-run", "--apt"]);
    let count = std::fs::read_dir(&pool).unwrap().count();
    assert_eq!(count, 3, "dry-run must not delete");
}

#[test]
fn rm_by_name_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pool = root.join("apt/pool/main");
    std::fs::create_dir_all(&pool).unwrap();
    write_deb(
        &pool.join("hello_1.0-1_amd64.deb"),
        "hello",
        "1.0-1",
        "amd64",
    );
    write_deb(
        &pool.join("hello_2.0-1_amd64.deb"),
        "hello",
        "2.0-1",
        "amd64",
    );

    // Remove only version 1.0-1.
    arx(root, &["rm", "hello", "--version", "1.0-1", "--apt"]);
    let remaining: Vec<String> = std::fs::read_dir(&pool)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(remaining, vec!["hello_2.0-1_amd64.deb".to_string()]);

    // Remove all remaining by name.
    arx(root, &["rm", "hello", "--apt"]);
    assert_eq!(std::fs::read_dir(&pool).unwrap().count(), 0);
}

#[test]
fn promote_moves_between_components() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let src = root.join("apt/pool/main");
    let dst = root.join("apt/pool/stable");
    std::fs::create_dir_all(&src).unwrap();
    write_deb(
        &src.join("hello_1.0-1_amd64.deb"),
        "hello",
        "1.0-1",
        "amd64",
    );

    let output = common::arx_command()
        .args([
            "promote",
            "hello",
            "--from",
            "main",
            "--to",
            "stable",
            "--apt",
            "--root",
            root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "promote failed: {stdout}");
    assert!(stdout.contains("Promoted"));
    // File moved.
    assert!(dst.join("hello_1.0-1_amd64.deb").exists());
    assert!(!src.join("hello_1.0-1_amd64.deb").exists());
}

#[test]
fn key_rotate_backs_up_and_replaces() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // init with a key.
    let status = common::arx_command()
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(root.join("keys/private.asc").exists());

    // Rotate.
    let output = common::arx_command()
        .args(["key", "rotate", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "rotate failed: {stdout}");
    assert!(
        root.join("keys/private.asc.old").exists(),
        "old key must be backed up"
    );
    assert!(root.join("keys/private.asc").exists(), "new key must exist");

    // Revoke.
    let output = common::arx_command()
        .args(["key", "revoke", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !root.join("keys/private.asc.old").exists(),
        "old key must be deleted"
    );
}

#[test]
fn default_rm_and_gc_keep_legacy_apt_only_repo_without_config_working() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pool = root.join("apt/pool/main");
    seed_three_versions(&pool);

    arx(root, &["rm", "hello", "--version", "1.0-1"]);
    assert!(!pool.join("hello_1.0_amd64.deb").exists());

    arx(root, &["gc", "--keep", "1"]);
    assert!(!pool.join("hello_2.0_amd64.deb").exists());
    assert!(pool.join("hello_3.0_amd64.deb").exists());
}

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

fn arx_output(root: &Path, args: &[&str]) -> String {
    let output = common::arx_command()
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "arx {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
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
fn gc_can_prune_only_a_named_package() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pool = root.join("apt/pool/main");
    std::fs::create_dir_all(&pool).unwrap();
    for v in ["1.0", "2.0", "3.0"] {
        write_deb(
            &pool.join(format!("hello_{v}_amd64.deb")),
            "hello",
            &format!("{v}-1"),
            "amd64",
        );
        write_deb(
            &pool.join(format!("other_{v}_amd64.deb")),
            "other",
            &format!("{v}-1"),
            "amd64",
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }

    arx(root, &["gc", "hello", "--keep", "1", "--apt"]);

    let remaining: Vec<String> = std::fs::read_dir(&pool)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !remaining.iter().any(|f| f.starts_with("hello_1.0")),
        "old hello should be pruned: {remaining:?}"
    );
    assert!(
        remaining.iter().any(|f| f.starts_with("hello_3.0")),
        "new hello should remain: {remaining:?}"
    );
    assert_eq!(
        remaining.iter().filter(|f| f.starts_with("other_")).count(),
        3,
        "non-target package versions must be untouched: {remaining:?}"
    );
}

#[test]
fn gc_keeps_rollback_pins_by_default_and_can_ignore_them_explicitly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let pool = root.join("apt/pool/main");
    common::arx_command()
        .args(["init", root.to_str().unwrap(), "--no-key"])
        .status()
        .unwrap();

    for v in ["1.0", "2.0", "3.0"] {
        let pkg = tmp.path().join(format!("rollbackpin_{v}_amd64.deb"));
        write_deb(&pkg, "rollbackpin", &format!("{v}-1"), "amd64");
        arx(&root, &["add", pkg.to_str().unwrap()]);
        arx(&root, &["publish", "--apt"]);
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }

    let output = arx_output(&root, &["gc", "rollbackpin", "--keep", "1", "--apt"]);
    assert!(output.contains("pinned by retained rollback states"));
    assert!(output.contains("--ignore-rollback-states"));
    assert!(output.contains("rollback states may no longer be valid"));
    assert!(
        pool.join("rollbackpin_1.0_amd64.deb").exists(),
        "default gc must keep packages pinned by rollback states"
    );

    arx(
        &root,
        &[
            "gc",
            "rollbackpin",
            "--keep",
            "1",
            "--apt",
            "--ignore-rollback-states",
        ],
    );
    assert!(
        !pool.join("rollbackpin_1.0_amd64.deb").exists(),
        "explicit ignore should allow old rollback-pinned package to be pruned"
    );
    assert!(pool.join("rollbackpin_3.0_amd64.deb").exists());
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

#[cfg(unix)]
#[test]
fn private_key_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let imported = tmp.path().join("imported");

    let status = common::arx_command()
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::metadata(root.join("keys/private.asc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let status = common::arx_command()
        .args(["init", imported.to_str().unwrap(), "--no-key"])
        .status()
        .unwrap();
    assert!(status.success());
    let output = common::arx_command()
        .args([
            "key",
            "import",
            "--root",
            imported.to_str().unwrap(),
            root.join("keys/private.asc").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "key import failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::metadata(imported.join("keys/private.asc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let output = common::arx_command()
        .args(["key", "rotate", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "key rotate failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::metadata(root.join("keys/private.asc.old"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(root.join("keys/private.asc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
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

//! Integration tests for `debrepo::build_apt`.
//!
//! Synthesizes minimal `.deb` files (ar + control.tar.gz) on disk, runs the
//! generator, and asserts the produced `Packages`/`Release` content.

use std::io::Write;
use std::path::Path;

use debrepo::{build_dist, ReleaseMeta};

/// Write a minimal but valid `.deb` (ar archive: debian-binary + control.tar.gz
/// + empty data.tar.gz) whose control paragraph is `control_text`.
fn write_deb(path: &Path, control_text: &str) {
    // control.tar.gz containing ./control
    let mut control_tar = Vec::new();
    {
        let mut tb = tar::Builder::new(&mut control_tar);
        let bytes = control_text.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_path("./control").unwrap();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tb.append(&header, bytes).unwrap();
        tb.finish().unwrap();
    }
    let control_gz = gzip(&control_tar);

    // empty data.tar.gz
    let mut data_tar = Vec::new();
    {
        let mut tb = tar::Builder::new(&mut data_tar);
        tb.finish().unwrap();
    }
    let data_gz = gzip(&data_tar);

    let file = std::fs::File::create(path).unwrap();
    let mut builder = ar::Builder::new(file);
    builder
        .append(&ar::Header::new(b"debian-binary".to_vec(), 4), &b"2.0\n"[..])
        .unwrap();
    builder
        .append(
            &ar::Header::new(b"control.tar.gz".to_vec(), control_gz.len() as u64),
            &control_gz[..],
        )
        .unwrap();
    builder
        .append(
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

fn control(name: &str, version: &str, arch: &str) -> String {
    format!(
        "Package: {name}\nVersion: {version}\nArchitecture: {arch}\nMaintainer: Test <t@localhost>\nDescription: test package {name}\n a multi-line\n description\n"
    )
}

#[test]
fn builds_packages_and_release() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    write_deb(&pool.join("foo_1.0_amd64.deb"), &control("foo", "1.0", "amd64"));
    write_deb(&pool.join("bar_2.0_amd64.deb"), &control("bar", "2.0", "amd64"));

    let meta = ReleaseMeta::new("TestOrigin", "TestLabel", "Test repo", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();

    assert_eq!(build.packages, 2);
    assert_eq!(build.architectures, vec!["amd64".to_string()]);

    // Packages index content
    let packages =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(packages.contains("Package: foo"));
    assert!(packages.contains("Package: bar"));
    assert!(packages.contains("Filename: pool/main/foo_1.0_amd64.deb"));
    assert!(packages.contains("SHA256:"));
    assert!(packages.contains("MD5sum:"));
    // Folded multi-line description preserved.
    assert!(packages.contains("\n a multi-line"));

    // Packages.gz exists and is non-empty.
    let gz = std::fs::metadata(apt.join("dists/stable/main/binary-amd64/Packages.gz")).unwrap();
    assert!(gz.len() > 0);

    // Release index content
    let release = std::fs::read_to_string(apt.join("dists/stable/Release")).unwrap();
    assert!(release.contains("Origin: TestOrigin"));
    assert!(release.contains("Suite: stable"));
    assert!(release.contains("Codename: stable"));
    assert!(release.contains("Components: main"));
    assert!(release.contains("Architectures: amd64"));
    assert!(release.contains("SHA256:"));
    assert!(release.contains("main/binary-amd64/Packages"));
    assert!(release.contains("main/binary-amd64/Packages.gz"));
}

#[test]
fn architecture_all_lands_in_each_concrete_arch() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    write_deb(&pool.join("native_1_amd64.deb"), &control("native", "1", "amd64"));
    write_deb(&pool.join("docs_1_all.deb"), &control("docs", "1", "all"));

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();

    assert_eq!(build.packages, 2);
    // Only the concrete arch gets an index; the `all` package is folded into it.
    assert_eq!(build.architectures, vec!["amd64".to_string()]);

    let packages =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(packages.contains("Package: native"));
    assert!(packages.contains("Package: docs")); // the Architecture: all package
}

#[test]
fn only_all_packages_produce_binary_all() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    write_deb(&pool.join("docs_1_all.deb"), &control("docs", "1", "all"));

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();

    assert_eq!(build.architectures, vec!["all".to_string()]);
    assert!(apt.join("dists/stable/main/binary-all/Packages").exists());
}

#[test]
fn empty_pool_yields_empty_build() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();
    assert_eq!(build.packages, 0);
    assert!(build.architectures.is_empty());
    // Release is still written.
    assert!(apt.join("dists/stable/Release").exists());
}

#[test]
fn multiple_components_share_one_release() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();
    std::fs::create_dir_all(apt.join("pool/contrib")).unwrap();

    write_deb(&apt.join("pool/main/foo_1_amd64.deb"), &control("foo", "1", "amd64"));
    write_deb(&apt.join("pool/contrib/bar_1_amd64.deb"), &control("bar", "1", "amd64"));

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();

    assert_eq!(build.packages, 2);
    assert_eq!(build.components, vec!["contrib".to_string(), "main".to_string()]);

    // A single Release must cover BOTH components (the P0 fix: no overwrite).
    let release = std::fs::read_to_string(apt.join("dists/stable/Release")).unwrap();
    assert!(release.contains("Components: contrib main"));
    assert!(release.contains("main/binary-amd64/Packages"));
    assert!(release.contains("contrib/binary-amd64/Packages"));

    // Both component indices exist on disk.
    assert!(apt.join("dists/stable/main/binary-amd64/Packages").exists());
    assert!(apt.join("dists/stable/contrib/binary-amd64/Packages").exists());
}

#[test]
fn writes_by_hash_and_sets_acquire_by_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();
    write_deb(&apt.join("pool/main/foo_1_amd64.deb"), &control("foo", "1", "amd64"));

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    build_dist(&apt, "stable", &meta).unwrap();

    let release = std::fs::read_to_string(apt.join("dists/stable/Release")).unwrap();
    assert!(release.contains("Acquire-By-Hash: yes"));

    // The by-hash copy of Packages must exist, named by its SHA256.
    let packages = std::fs::read(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&packages);
        hex::encode(h.finalize())
    };
    let by_hash = apt
        .join("dists/stable/main/binary-amd64/by-hash/SHA256")
        .join(&sha);
    assert!(by_hash.exists(), "missing by-hash copy at {}", by_hash.display());
}

#[test]
fn republish_atomically_replaces_previous_dist() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();

    write_deb(&apt.join("pool/main/foo_1_amd64.deb"), &control("foo", "1", "amd64"));
    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    build_dist(&apt, "stable", &meta).unwrap();

    // Add a second package and republish; the new dist must reflect both and
    // leave no staging/backup dirs behind.
    write_deb(&apt.join("pool/main/bar_1_amd64.deb"), &control("bar", "1", "amd64"));
    let build = build_dist(&apt, "stable", &meta).unwrap();
    assert_eq!(build.packages, 2);

    let packages =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(packages.contains("Package: foo"));
    assert!(packages.contains("Package: bar"));
    assert!(!apt.join("dists/.stable.staging").exists());
    assert!(!apt.join("dists/stable.old").exists());
}

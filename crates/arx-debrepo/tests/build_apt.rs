//! Integration tests for `arx_debrepo::build_apt`.
//!
//! Synthesizes minimal `.deb` files (ar + control.tar.gz) on disk, runs the
//! generator, and asserts the produced `Packages`/`Release` content.

use std::io::Write;
use std::path::Path;

use arx_debrepo::{build_dist, ReleaseMeta};

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
        .append(
            &ar::Header::new(b"debian-binary".to_vec(), 4),
            &b"2.0\n"[..],
        )
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

    write_deb(
        &pool.join("foo_1.0_amd64.deb"),
        &control("foo", "1.0", "amd64"),
    );
    write_deb(
        &pool.join("bar_2.0_amd64.deb"),
        &control("bar", "2.0", "amd64"),
    );

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
    // Default ReleaseMeta has valid_days = 0 → no expiry field.
    assert!(
        !release.contains("Valid-Until:"),
        "Valid-Until must be omitted when valid_days == 0"
    );
}

#[test]
fn valid_until_emitted_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();
    write_deb(
        &pool.join("foo_1.0_amd64.deb"),
        &control("foo", "1.0", "amd64"),
    );

    let meta = ReleaseMeta::new("O", "L", "D", "stable").with_valid_days(7);
    build_dist(&apt, "stable", &meta).unwrap();

    let release = std::fs::read_to_string(apt.join("dists/stable/Release")).unwrap();
    let date = release
        .lines()
        .find_map(|l| l.strip_prefix("Date: "))
        .expect("Release has a Date");
    let valid = release
        .lines()
        .find_map(|l| l.strip_prefix("Valid-Until: "))
        .expect("Release has Valid-Until when valid_days > 0");
    // apt requires the same RFC822 shape as Date; the window is non-empty.
    assert!(
        valid.ends_with(" UTC"),
        "Valid-Until is RFC822 UTC: {valid}"
    );
    assert_eq!(
        date.split(' ').count(),
        valid.split(' ').count(),
        "Valid-Until ({valid}) must share Date's ({date}) field shape"
    );
    assert_ne!(date, valid, "a 7-day window must move the timestamp");
}

#[test]
fn skips_unreadable_deb_and_indexes_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    write_deb(
        &pool.join("good_1.0_amd64.deb"),
        &control("good", "1.0", "amd64"),
    );
    // Not a valid ar archive → unreadable; must be skipped, not fatal.
    std::fs::write(pool.join("broken_1.0_amd64.deb"), b"this is not a .deb").unwrap();

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let staged = arx_debrepo::stage_dist(&apt, "pool", "stable", &meta, false).unwrap();

    assert_eq!(staged.packages, 1, "the good package is still indexed");
    assert_eq!(staged.skipped.len(), 1, "the broken package is skipped");
    assert!(staged.skipped[0].path.ends_with("broken_1.0_amd64.deb"));
}

#[test]
fn identical_duplicate_is_indexed_once_not_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    // Same Package/Version/Arch and identical bytes (control is deterministic):
    // an accidental double-add. Indexed once, idempotent — not a skip.
    let ctl = control("dup", "1.0", "amd64");
    write_deb(&pool.join("dup_1.0_amd64.deb"), &ctl);
    write_deb(&pool.join("dup-again_1.0_amd64.deb"), &ctl);

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let staged = arx_debrepo::stage_dist(&apt, "pool", "stable", &meta, false).unwrap();

    assert_eq!(
        staged.packages, 1,
        "identical duplicate collapses to one stanza"
    );
    assert!(
        staged.skipped.is_empty(),
        "an identical re-add is idempotent, not a skip: {:?}",
        staged.skipped
    );
}

#[test]
fn same_identity_different_content_is_a_collision() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    // Same (Package, Version, Architecture) but different bytes (Maintainer
    // differs) → a real collision; the first by sorted path wins, the other is
    // recorded as skipped rather than emitting a duplicate stanza.
    let a = "Package: clash\nVersion: 1.0\nArchitecture: amd64\nMaintainer: A <a@x>\nDescription: one\n";
    let b = "Package: clash\nVersion: 1.0\nArchitecture: amd64\nMaintainer: B <b@x>\nDescription: two\n";
    write_deb(&pool.join("clash-a_1.0_amd64.deb"), a);
    write_deb(&pool.join("clash-b_1.0_amd64.deb"), b);

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let staged = arx_debrepo::stage_dist(&apt, "pool", "stable", &meta, false).unwrap();

    assert_eq!(staged.packages, 1, "first by sorted path wins");
    assert_eq!(staged.skipped.len(), 1, "the colliding package is skipped");
    assert!(
        staged.skipped[0].reason.contains("collision"),
        "reason should explain the collision: {}",
        staged.skipped[0].reason
    );
}

#[test]
fn architecture_all_lands_in_each_concrete_arch() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();

    write_deb(
        &pool.join("native_1_amd64.deb"),
        &control("native", "1", "amd64"),
    );
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
fn states_and_rollback() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    let pool = apt.join("pool/main");
    std::fs::create_dir_all(&pool).unwrap();
    let meta = ReleaseMeta::new("O", "L", "D", "stable");

    // Publish #1 (one package) → state 000001.
    write_deb(&pool.join("foo_1_amd64.deb"), &control("foo", "1", "amd64"));
    build_dist(&apt, "stable", &meta).unwrap();
    // Publish #2 (two packages) → state 000002, now current.
    write_deb(&pool.join("bar_1_amd64.deb"), &control("bar", "1", "amd64"));
    build_dist(&apt, "stable", &meta).unwrap();

    // dists/stable is a symlink; two states exist; the newest is current.
    assert!(std::fs::symlink_metadata(apt.join("dists/stable"))
        .unwrap()
        .file_type()
        .is_symlink());
    let states = arx_debrepo::list_states(&apt, "stable").unwrap();
    assert_eq!(states.len(), 2);
    assert_eq!(states.iter().filter(|s| s.current).count(), 1);
    assert!(states.last().unwrap().current); // newest is current

    // Current Release lists both packages.
    let pkgs =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(pkgs.contains("Package: foo") && pkgs.contains("Package: bar"));

    // Roll back → previous state becomes current, Release loses bar.
    let to = arx_debrepo::rollback(&apt, "stable", None).unwrap();
    assert_eq!(to, "000001");
    let pkgs =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(pkgs.contains("Package: foo") && !pkgs.contains("Package: bar"));
    assert!(
        arx_debrepo::list_states(&apt, "stable")
            .unwrap()
            .iter()
            .find(|s| s.id == "000001")
            .unwrap()
            .current
    );
}

#[test]
fn multiple_components_share_one_release() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();
    std::fs::create_dir_all(apt.join("pool/contrib")).unwrap();

    write_deb(
        &apt.join("pool/main/foo_1_amd64.deb"),
        &control("foo", "1", "amd64"),
    );
    write_deb(
        &apt.join("pool/contrib/bar_1_amd64.deb"),
        &control("bar", "1", "amd64"),
    );

    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    let build = build_dist(&apt, "stable", &meta).unwrap();

    assert_eq!(build.packages, 2);
    assert_eq!(
        build.components,
        vec!["contrib".to_string(), "main".to_string()]
    );

    // A single Release must cover BOTH components (the P0 fix: no overwrite).
    let release = std::fs::read_to_string(apt.join("dists/stable/Release")).unwrap();
    assert!(release.contains("Components: contrib main"));
    assert!(release.contains("main/binary-amd64/Packages"));
    assert!(release.contains("contrib/binary-amd64/Packages"));

    // Both component indices exist on disk.
    assert!(apt.join("dists/stable/main/binary-amd64/Packages").exists());
    assert!(apt
        .join("dists/stable/contrib/binary-amd64/Packages")
        .exists());
}

#[test]
fn read_data_paths_extracts_installed_files() {
    let tmp = tempfile::tempdir().unwrap();
    // Build a .deb with a non-empty data.tar (one regular file).
    let deb = tmp.path().join("test.deb");
    let control = control("test", "1.0", "amd64");
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
    // Non-empty data.tar with one actual file (same pattern as write_deb).
    let mut data_tar = Vec::new();
    {
        let mut tb = tar::Builder::new(&mut data_tar);
        let body = b"bar";
        let mut h = tar::Header::new_gnu();
        h.set_path("./usr/bin/foo").unwrap();
        h.set_size(body.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tb.append(&h, body.as_slice()).unwrap();
        tb.finish().unwrap();
    }
    let control_gz = gzip(&control_tar);
    let data_gz = gzip(&data_tar);
    let file = std::fs::File::create(&deb).unwrap();
    let mut builder = ar::Builder::new(file);
    builder
        .append(
            &ar::Header::new(b"debian-binary".to_vec(), 4),
            &b"2.0\n"[..],
        )
        .unwrap();
    builder
        .append(
            &ar::Header::new(b"control.tar.gz".to_vec(), control_gz.len() as u64),
            control_gz.as_slice(),
        )
        .unwrap();
    builder
        .append(
            &ar::Header::new(b"data.tar.gz".to_vec(), data_gz.len() as u64),
            data_gz.as_slice(),
        )
        .unwrap();

    let paths = arx_debrepo::deb::read_data_paths(&deb).unwrap();
    assert!(!paths.is_empty(), "must find at least one file in data.tar");
    assert!(
        paths.iter().any(|p| p.contains("usr/bin/foo")),
        "must contain the installed file: {paths:?}"
    );
}

#[test]
fn writes_by_hash_and_sets_acquire_by_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();
    write_deb(
        &apt.join("pool/main/foo_1_amd64.deb"),
        &control("foo", "1", "amd64"),
    );

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
    assert!(
        by_hash.exists(),
        "missing by-hash copy at {}",
        by_hash.display()
    );
}

#[test]
fn republish_atomically_replaces_previous_dist() {
    let tmp = tempfile::tempdir().unwrap();
    let apt = tmp.path().join("apt");
    std::fs::create_dir_all(apt.join("pool/main")).unwrap();

    write_deb(
        &apt.join("pool/main/foo_1_amd64.deb"),
        &control("foo", "1", "amd64"),
    );
    let meta = ReleaseMeta::new("O", "L", "D", "stable");
    build_dist(&apt, "stable", &meta).unwrap();

    // Add a second package and republish; the new dist must reflect both and
    // leave no staging/backup dirs behind.
    write_deb(
        &apt.join("pool/main/bar_1_amd64.deb"),
        &control("bar", "1", "amd64"),
    );
    let build = build_dist(&apt, "stable", &meta).unwrap();
    assert_eq!(build.packages, 2);

    let packages =
        std::fs::read_to_string(apt.join("dists/stable/main/binary-amd64/Packages")).unwrap();
    assert!(packages.contains("Package: foo"));
    assert!(packages.contains("Package: bar"));
    assert!(!apt.join("dists/.stable.staging").exists());
    assert!(!apt.join("dists/stable.old").exists());
}

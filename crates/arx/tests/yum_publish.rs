//! End-to-end yum test: drive the real `arx` binary to build a `.rpm` with
//! `arx pack`, publish yum repodata, and assert the generated metadata is
//! structurally valid and signed. Closes the "yum has no integration coverage"
//! gap (ADR-0011 product-ready bar #4) without needing a `dnf` container — the
//! repodata XML + signature are verified directly.

use std::io::Read;

use arx_debrepo::FileManifest;
use std::path::Path;

mod common;

fn arx(args: &[&str]) -> bool {
    common::arx_command().args(args).status().unwrap().success()
}
fn arx_in(cwd: &Path, args: &[&str]) -> bool {
    common::arx_command()
        .current_dir(cwd)
        .args(args)
        .status()
        .unwrap()
        .success()
}

fn write_pack_manifest(path: &Path, payload: &Path, name: &str, version: &str) {
    write_pack_manifest_for_arch(path, payload, name, version, "x86_64");
}

fn write_pack_manifest_for_arch(
    path: &Path,
    payload: &Path,
    name: &str,
    version: &str,
    arch: &str,
) {
    std::fs::write(
        path,
        format!(
            "name = \"{name}\"\n\
             version = \"{version}\"\n\
             arch = \"{arch}\"\n\
             maintainer = \"T <t@localhost>\"\n\
             description = \"{name}\"\n\
             license = \"MIT\"\n\
             [[files]]\n\
             source = \"{}\"\n\
             dest = \"/usr/share/{name}/data\"\n\
             mode = \"0644\"\n",
            payload.display()
        ),
    )
    .unwrap();
}

/// Build one `.rpm` for `arch` straight into `out`.
fn pack_rpm(root: &Path, out: &Path, name: &str, version: &str, arch: &str) {
    let payload = root.join(format!("{name}.data"));
    std::fs::write(&payload, format!("{name}\n")).unwrap();
    let manifest = root.join(format!("{name}-{arch}.toml"));
    write_pack_manifest_for_arch(&manifest, &payload, name, version, arch);
    std::fs::create_dir_all(out).unwrap();
    assert!(
        arx(&[
            "pack",
            manifest.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--rpm",
        ]),
        "arx pack {name} ({arch}) failed"
    );
}

fn read_primary_xml(repodata: &Path) -> String {
    let primary_gz =
        find_with_suffix(repodata, "primary.xml.gz").expect("primary.xml.gz must exist");
    let gz = std::fs::read(&primary_gz).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn find_with_suffix(dir: &Path, suffix: &str) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().ends_with(suffix))
                .unwrap_or(false)
        })
}

#[test]
fn yum_publish_builds_valid_signed_repodata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // init with a signing key (default) so we also exercise repomd.xml.asc.
    assert!(arx(&["init", root.to_str().unwrap()]), "arx init failed");

    // Build a real .rpm with `arx pack`, dropped straight into the yum arch dir.
    let payload = root.join("payload");
    std::fs::write(&payload, b"#!/bin/sh\necho hi\n").unwrap();
    let manifest = root.join("m.toml");
    write_pack_manifest(&manifest, &payload, "greeter", "1.2.3");

    let arch_dir = root.join("yum/myrepo/x86_64");
    std::fs::create_dir_all(&arch_dir).unwrap();
    assert!(
        arx(&[
            "pack",
            manifest.to_str().unwrap(),
            "--out",
            arch_dir.to_str().unwrap(),
        ]),
        "arx pack failed"
    );
    // pack also emits a .deb here; the yum walker only collects .rpm.
    assert!(
        find_with_suffix(&arch_dir, ".rpm").is_some(),
        "pack should have produced an .rpm"
    );

    // Publish yum metadata.
    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "arx publish --yum failed"
    );

    // --- assert the repodata is structurally valid ---
    let repodata = arch_dir.join("repodata");
    let repomd = std::fs::read_to_string(repodata.join("repomd.xml"))
        .expect("repomd.xml must exist after publish");
    for record in ["primary", "filelists", "other"] {
        assert!(
            repomd.contains(record),
            "repomd.xml must reference the {record} stream:\n{repomd}"
        );
    }

    // primary.xml.gz must list the package we packed.
    let xml = read_primary_xml(&repodata);
    assert!(xml.contains("greeter"), "primary.xml must list the package");
    assert!(xml.contains("1.2.3"), "primary.xml must carry the version");

    // signing was enabled at init → repomd.xml.asc must be present.
    assert!(
        repodata.join("repomd.xml.asc").exists(),
        "repomd.xml.asc (detached signature) must be written when signing is on"
    );
}

#[test]
fn yum_incremental_publish_caches_xml_fragments_for_small_adds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert!(
        arx(&["init", root.to_str().unwrap(), "--no-key"]),
        "arx init failed"
    );

    let arch_dir = root.join("yum/myrepo/x86_64");
    std::fs::create_dir_all(&arch_dir).unwrap();
    for (name, version) in [("alpha", "1.0.0"), ("beta", "1.0.0")] {
        let payload = root.join(format!("{name}.sh"));
        std::fs::write(&payload, format!("#!/bin/sh\necho {name}\n")).unwrap();
        let manifest = root.join(format!("{name}.toml"));
        write_pack_manifest(&manifest, &payload, name, version);
        assert!(
            arx(&[
                "pack",
                manifest.to_str().unwrap(),
                "--out",
                arch_dir.to_str().unwrap(),
                "--rpm",
            ]),
            "arx pack {name} failed"
        );
        assert!(
            arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
            "arx publish --yum failed after {name}"
        );
    }

    let manifest = FileManifest::load(&arch_dir).unwrap();
    assert_eq!(
        manifest.files.len(),
        2,
        "yum manifest should retain both packages"
    );
    for (filename, cached) in &manifest.files {
        assert!(filename.ends_with(".rpm"));
        assert!(
            !cached.stanza.is_empty(),
            "primary fragment missing for {filename}"
        );
        assert!(
            !cached.contents.is_empty(),
            "filelists fragment missing for {filename}"
        );
        assert!(
            !cached.other.is_empty(),
            "other fragment missing for {filename}"
        );
    }

    // Older yum manifests may only have mtime/size and no XML fragments. They
    // must not be considered fresh, otherwise publish would skip the rebuild
    // needed to backfill reusable metadata fragments.
    let mut legacy_manifest = manifest.clone();
    for cached in legacy_manifest.files.values_mut() {
        cached.stanza.clear();
        cached.contents.clear();
        cached.other.clear();
    }
    legacy_manifest.save(&arch_dir).unwrap();
    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "arx publish --yum failed with legacy manifest"
    );
    let backfilled = FileManifest::load(&arch_dir).unwrap();
    for (filename, cached) in &backfilled.files {
        assert!(
            !cached.stanza.is_empty() && !cached.contents.is_empty() && !cached.other.is_empty(),
            "legacy yum manifest was not backfilled for {filename}"
        );
    }

    let xml = read_primary_xml(&arch_dir.join("repodata"));
    assert!(
        xml.contains("alpha"),
        "primary.xml must retain first package: {xml}"
    );
    assert!(
        xml.contains("beta"),
        "primary.xml must include new package: {xml}"
    );
}

#[cfg(unix)]
#[test]
fn noarch_packages_are_indexed_by_every_arch_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert!(
        arx(&["init", root.to_str().unwrap(), "--no-key"]),
        "arx init failed"
    );

    let repo = root.join("yum/myrepo");
    pack_rpm(root, &repo.join("x86_64"), "toolx", "1.0.0", "x86_64");
    pack_rpm(root, &repo.join("aarch64"), "toolx", "1.0.0", "aarch64");
    pack_rpm(root, &repo.join("noarch"), "shareddata", "1.0.0", "noarch");
    let shared_rpm = repo.join("noarch/shareddata-1.0.0-1.noarch.rpm");
    for arch in ["x86_64", "aarch64"] {
        std::os::unix::fs::symlink(
            "../noarch/shareddata-1.0.0-1.noarch.rpm",
            repo.join(arch).join(shared_rpm.file_name().unwrap()),
        )
        .unwrap();
    }

    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "arx publish --yum failed"
    );

    for arch in ["x86_64", "aarch64"] {
        let xml = read_primary_xml(&repo.join(arch).join("repodata"));
        assert!(
            xml.contains("<location href=\"../noarch/shareddata-1.0.0-1.noarch.rpm\"/>"),
            "{arch} index must advertise the shared noarch package:\n{xml}"
        );
        assert!(
            xml.contains("packages=\"2\""),
            "{arch} index must count its own package plus the noarch one:\n{xml}"
        );
        let legacy_link = repo.join(arch).join("shareddata-1.0.0-1.noarch.rpm");
        assert!(
            std::fs::symlink_metadata(&legacy_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "legacy noarch workaround link must remain a link in {arch}"
        );
    }

    // The noarch repo itself keeps plain local locations.
    let noarch_xml = read_primary_xml(&repo.join("noarch").join("repodata"));
    assert!(
        noarch_xml.contains("<location href=\"shareddata-1.0.0-1.noarch.rpm\"/>"),
        "the noarch index must reference its own directory:\n{noarch_xml}"
    );
    assert!(
        !noarch_xml.contains("toolx"),
        "arch-specific packages must not leak into the noarch index:\n{noarch_xml}"
    );

    // A second incremental publish must keep the folded entries.
    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "second arx publish --yum failed"
    );
    let xml = read_primary_xml(&repo.join("x86_64").join("repodata"));
    assert!(
        xml.contains("<location href=\"../noarch/shareddata-1.0.0-1.noarch.rpm\"/>"),
        "incremental publish must keep the shared noarch location:\n{xml}"
    );
}

#[test]
fn incremental_publish_updates_noarch_location_after_moving_rpm() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert!(
        arx(&["init", root.to_str().unwrap(), "--no-key"]),
        "arx init failed"
    );

    let repo = root.join("yum/myrepo");
    let arch_dir = repo.join("x86_64");
    let rpm_name = "shareddata-1.0.0-1.noarch.rpm";
    pack_rpm(root, &arch_dir, "shareddata", "1.0.0", "noarch");

    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "initial arx publish --yum failed"
    );
    let first = read_primary_xml(&arch_dir.join("repodata"));
    assert!(
        first.contains(&format!("<location href=\"{rpm_name}\"/>")),
        "initial index must use the local RPM location:\n{first}"
    );

    let noarch_dir = repo.join("noarch");
    std::fs::create_dir_all(&noarch_dir).unwrap();
    std::fs::rename(arch_dir.join(rpm_name), noarch_dir.join(rpm_name)).unwrap();

    assert!(
        arx(&["publish", "--root", root.to_str().unwrap(), "--yum"]),
        "incremental arx publish --yum after moving RPM failed"
    );
    let second = read_primary_xml(&arch_dir.join("repodata"));
    assert!(
        second.contains(&format!("<location href=\"../noarch/{rpm_name}\"/>")),
        "moved noarch RPM must switch to a relative sibling location:\n{second}"
    );
}

#[test]
fn publish_repo_scopes_yum_metadata_to_one_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert!(
        arx(&["init", root.to_str().unwrap(), "--no-key"]),
        "arx init failed"
    );

    let el9 = root.join("yum/el9/x86_64");
    let el10 = root.join("yum/el10/x86_64");
    pack_rpm(root, &el9, "alpha", "1.0.0", "x86_64");
    pack_rpm(root, &el10, "beta", "1.0.0", "x86_64");

    assert!(
        arx(&[
            "publish",
            "--root",
            root.to_str().unwrap(),
            "--yum",
            "--repo",
            "el9",
        ]),
        "arx publish --yum --repo el9 failed"
    );
    assert!(
        el9.join("repodata/repomd.xml").exists(),
        "--repo el9 must publish el9"
    );
    assert!(
        !el10.join("repodata").exists(),
        "--repo el9 must not publish other repos"
    );

    let missing = common::arx_command()
        .args([
            "publish",
            "--root",
            root.to_str().unwrap(),
            "--yum",
            "--repo",
            "el11",
        ])
        .output()
        .unwrap();
    assert!(
        !missing.status.success(),
        "--repo naming a missing directory must fail instead of publishing everything"
    );
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("does not exist"),
        "stderr should name the missing repo directory: {}",
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(
        !el10.join("repodata").exists(),
        "a failed --repo publish must not publish other repos"
    );
}

#[test]
fn gc_keeps_yum_rollback_pins_with_relative_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    assert!(
        arx(&["init", root.to_str().unwrap(), "--no-key"]),
        "arx init failed"
    );

    let noarch = root.join("yum/myrepo/noarch");
    pack_rpm(&root, &noarch, "shared", "1.0.0", "noarch");
    assert!(
        arx_in(
            tmp.path(),
            &["publish", "--root", "repo", "--yum", "--full"]
        ),
        "relative-root initial publish failed"
    );

    pack_rpm(&root, &noarch, "shared", "2.0.0", "noarch");
    assert!(
        arx_in(tmp.path(), &["publish", "--root", "repo", "--yum"]),
        "relative-root second publish failed"
    );

    let gc = common::arx_command()
        .current_dir(tmp.path())
        .args(["gc", "shared", "--keep", "1", "--yum", "--root", "repo"])
        .output()
        .unwrap();
    assert!(
        gc.status.success(),
        "relative-root yum gc failed:\n{}",
        String::from_utf8_lossy(&gc.stderr)
    );
    assert!(
        String::from_utf8_lossy(&gc.stdout).contains("pinned by retained rollback states"),
        "relative-root yum gc must report pinned package:\n{}",
        String::from_utf8_lossy(&gc.stdout)
    );
    assert!(
        noarch.join("shared-1.0.0-1.noarch.rpm").exists(),
        "relative-root gc must preserve the RPM referenced by rollback state"
    );
}

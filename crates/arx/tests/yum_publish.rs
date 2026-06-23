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

fn write_pack_manifest(path: &Path, payload: &Path, name: &str, version: &str) {
    std::fs::write(
        path,
        format!(
            "name = \"{name}\"\n\
             version = \"{version}\"\n\
             arch = \"x86_64\"\n\
             maintainer = \"T <t@localhost>\"\n\
             description = \"{name}\"\n\
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

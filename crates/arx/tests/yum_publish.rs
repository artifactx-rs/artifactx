//! End-to-end yum test: drive the real `arx` binary to build a `.rpm` with
//! `arx pack`, publish yum repodata, and assert the generated metadata is
//! structurally valid and signed. Closes the "yum has no integration coverage"
//! gap (ADR-0011 product-ready bar #4) without needing a `dnf` container — the
//! repodata XML + signature are verified directly.

use std::io::Read;
use std::path::Path;
use std::process::Command;

fn arx(args: &[&str]) -> bool {
    Command::new(env!("CARGO_BIN_EXE_arx"))
        .args(args)
        .status()
        .unwrap()
        .success()
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
    assert!(
        arx(&["init", root.to_str().unwrap()]),
        "arx init failed"
    );

    // Build a real .rpm with `arx pack`, dropped straight into the yum arch dir.
    let payload = root.join("payload");
    std::fs::write(&payload, b"#!/bin/sh\necho hi\n").unwrap();
    let manifest = root.join("m.toml");
    std::fs::write(
        &manifest,
        format!(
            "name = \"greeter\"\n\
             version = \"1.2.3\"\n\
             arch = \"x86_64\"\n\
             maintainer = \"T <t@localhost>\"\n\
             description = \"greeter\"\n\
             license = \"MIT\"\n\
             [[files]]\n\
             source = \"{}\"\n\
             dest = \"/usr/bin/greeter\"\n\
             mode = \"0755\"\n",
            payload.display()
        ),
    )
    .unwrap();

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
    let primary_gz = find_with_suffix(&repodata, "primary.xml.gz")
        .expect("primary.xml.gz must exist");
    let gz = std::fs::read(&primary_gz).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("greeter"), "primary.xml must list the package");
    assert!(xml.contains("1.2.3"), "primary.xml must carry the version");

    // signing was enabled at init → repomd.xml.asc must be present.
    assert!(
        repodata.join("repomd.xml.asc").exists(),
        "repomd.xml.asc (detached signature) must be written when signing is on"
    );
}

//! Integration tests for the `pack` crate.
//!
//! These build real `.deb` and `.rpm` artifacts from a sample manifest and read
//! them back to assert the metadata round-trips. The `.deb` is re-parsed inline
//! with `ar` + `tar` + `flate2` (no dependency on the sibling `debrepo` crate)
//! to keep this crate standalone.

use std::io::Read;
use std::path::Path;

use pack::{Backend, Format, Manifest};

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
fn builds_and_reparses_deb() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());

    let deb_path = pack::build_deb(&manifest, dir.path()).expect("deb builds");
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

    let rpm_path = pack::build_rpm(&manifest, dir.path()).expect("rpm builds");
    assert!(rpm_path.exists());

    let pkg = rpm::Package::open(&rpm_path).expect("rpm re-opens");
    assert_eq!(pkg.metadata.get_name().unwrap(), "hello");
    assert_eq!(pkg.metadata.get_version().unwrap(), "1.2.3");
    assert_eq!(pkg.metadata.get_arch().unwrap(), "x86_64");
}

#[test]
fn backend_native_builds_both() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());
    let backend = Backend::Native;

    let deb = backend.build(&manifest, Format::Deb, dir.path()).unwrap();
    let rpm = backend.build(&manifest, Format::Rpm, dir.path()).unwrap();
    assert!(deb.exists() && rpm.exists());
}

#[test]
fn backend_docker_is_stubbed() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = sample_manifest(dir.path());
    let backend = Backend::Docker {
        image: "debian:bookworm".into(),
    };
    let err = backend
        .build(&manifest, Format::Deb, dir.path())
        .expect_err("docker backend should be unimplemented");
    assert!(err.to_string().contains("not yet implemented"));
}

// --- inline .deb re-parsing helpers (ar + tar + flate2) ---

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

//! Real-tool validation: build `.deb`/`.rpm` packages and verify the output with
//! `dpkg-deb` and `rpm` — the formats' canonical validators. These tests are
//! **gated**: skipped when the tool is absent (typical macOS dev box), but
//! `PACK_REQUIRE_REAL_TOOLS=1` in CI turns "absent" into a failure so the
//! evidence gate is never silently skipped where it matters (ADR-0012 §4).
//!
//! On macOS these tools are available via Homebrew: `brew install dpkg rpm`.

use std::path::Path;
use std::process::Command;

use arx_pack::Manifest;

// ---------------------------------------------------------------------------
// tool-gating helpers
// ---------------------------------------------------------------------------

fn require_tools() -> bool {
    std::env::var("PACK_REQUIRE_REAL_TOOLS").is_ok_and(|v| !v.is_empty())
}

fn has_dpkg_deb() -> bool {
    Command::new("dpkg-deb")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn has_rpm() -> bool {
    Command::new("rpm")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Skip or fail depending on whether a tool is present. `name` is the CLI
/// binary; `found` came from `has_*()` above.
fn gate(name: &str, found: bool) {
    if found {
        return;
    }
    if require_tools() {
        panic!("PACK_REQUIRE_REAL_TOOLS is set but {name} not found on PATH");
    }
    eprintln!("skipping — {name} not on PATH (set PACK_REQUIRE_REAL_TOOLS=1 in CI)");
}

// ---------------------------------------------------------------------------
// manifest helpers
// ---------------------------------------------------------------------------

fn write_file(dir: &Path, name: &str, body: &[u8], executable: bool) {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn simple_manifest(dir: &Path) -> Manifest {
    write_file(dir, "hello.sh", b"#!/bin/sh\necho hello\n", true);
    Manifest::from_toml_str(&format!(
        "name = 'hello'\n\
         version = '1.0'\n\
         arch = 'amd64'\n\
         maintainer = 'T <t@localhost>'\n\
         description = 'A friendly greeter'\n\
         license = 'MIT'\n\
         section = 'utils'\n\
         depends = ['bash']\n\
         [[files]]\n\
         source = '{}'\n\
         dest = '/usr/bin/hello'\n\
         mode = '0755'\n",
        dir.join("hello.sh").display()
    ))
    .unwrap()
}

fn multi_file_manifest(dir: &Path) -> Manifest {
    write_file(dir, "app", b"#!/bin/sh\necho app\n", true);
    write_file(dir, "lib.so", b"shared-object-data", false);
    write_file(
        dir,
        "readme.txt",
        b"This is a readme\nBuilt by arx\n",
        false,
    );
    Manifest::from_toml_str(&format!(
        "name = 'multi'\n\
         version = '2.0'\n\
         arch = 'amd64'\n\
         maintainer = 'T <t@localhost>'\n\
         description = 'Multi-file package'\n\
         license = 'MIT'\n\
         conflicts = ['old-multi']\n\
         provides = ['multi']\n\
         [[files]]\n\
         source = '{}'\n\
         dest = '/usr/bin/app'\n\
         mode = '0755'\n\
         [[files]]\n\
         source = '{}'\n\
         dest = '/usr/lib/lib.so'\n\
         mode = '0644'\n\
         [[files]]\n\
         source = '{}'\n\
         dest = '/usr/share/doc/multi/readme.txt'\n\
         mode = '0644'\n",
        dir.join("app").display(),
        dir.join("lib.so").display(),
        dir.join("readme.txt").display(),
    ))
    .unwrap()
}

fn scripted_manifest(dir: &Path) -> Manifest {
    write_file(dir, "app", b"#!/bin/sh\necho app\n", true);
    write_file(
        dir,
        "postinst",
        b"#!/bin/sh\nset -e\necho installed\n",
        true,
    );
    write_file(dir, "prerm", b"#!/bin/sh\nset -e\necho removing\n", true);
    Manifest::from_toml_str(&format!(
        "name = 'scripted'\n\
         version = '3.0'\n\
         arch = 'amd64'\n\
         maintainer = 'T <t@localhost>'\n\
         description = 'Scripted package'\n\
         license = 'MIT'\n\
         [scripts]\n\
         postinst = '{}'\n\
         prerm = '{}'\n\
         [[files]]\n\
         source = '{}'\n\
         dest = '/usr/bin/app'\n\
         mode = '0755'\n",
        dir.join("postinst").display(),
        dir.join("prerm").display(),
        dir.join("app").display(),
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// real-tool tests
// ---------------------------------------------------------------------------

#[test]
fn dpkg_deb_validates_single_file_package() {
    gate("dpkg-deb", has_dpkg_deb());
    if !has_dpkg_deb() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let m = simple_manifest(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let deb = arx_pack::build_deb(&m, &out).unwrap();

    let info = Command::new("dpkg-deb")
        .args(["--info", deb.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(info.status.success(), "dpkg-deb --info failed");
    let info = String::from_utf8_lossy(&info.stdout);
    assert!(
        info.contains("Package: hello"),
        "--info missing Package:\n{info}"
    );
    assert!(
        info.contains("Version: 1.0"),
        "--info missing Version:\n{info}"
    );
    assert!(
        info.contains("Architecture: amd64"),
        "--info missing Arch:\n{info}"
    );
    assert!(
        info.contains("Depends: bash"),
        "--info missing Depends:\n{info}"
    );

    let contents = Command::new("dpkg-deb")
        .args(["--contents", deb.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(contents.status.success(), "dpkg-deb --contents failed");
    let c = String::from_utf8_lossy(&contents.stdout);
    // dpkg-deb --contents shows mode, owner, size, date, path.
    assert!(
        c.contains("usr/bin/hello"),
        "--contents missing the file:\n{c}"
    );
}

#[test]
fn dpkg_deb_validates_multi_file_package() {
    gate("dpkg-deb", has_dpkg_deb());
    if !has_dpkg_deb() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let m = multi_file_manifest(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let deb = arx_pack::build_deb(&m, &out).unwrap();

    let contents = Command::new("dpkg-deb")
        .args(["--contents", deb.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(contents.status.success());
    let c = String::from_utf8_lossy(&contents.stdout);
    assert!(c.contains("usr/bin/app"), "--contents missing app:\n{c}");
    assert!(
        c.contains("usr/lib/lib.so"),
        "--contents missing lib.so:\n{c}"
    );
    assert!(
        c.contains("usr/share/doc/multi/readme.txt"),
        "--contents missing readme.txt:\n{c}"
    );

    let info = Command::new("dpkg-deb")
        .args(["--info", deb.to_str().unwrap()])
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&info.stdout);
    assert!(
        info.contains("Conflicts: old-multi"),
        "--info missing Conflicts:\n{info}"
    );
    assert!(
        info.contains("Provides: multi"),
        "--info missing Provides:\n{info}"
    );
}

#[test]
fn dpkg_deb_validates_maintainer_scripts() {
    gate("dpkg-deb", has_dpkg_deb());
    if !has_dpkg_deb() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let m = scripted_manifest(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let deb = arx_pack::build_deb(&m, &out).unwrap();

    // dpkg-deb --info lists the control tarball contents, including scripts.
    let info = Command::new("dpkg-deb")
        .args(["--info", deb.to_str().unwrap()])
        .output()
        .unwrap();
    let out_text = String::from_utf8_lossy(&info.stdout);
    assert!(
        out_text.contains("postinst") || out_text.contains("control.tar"),
        "postinst script should be embedded:\n{out_text}"
    );
}

#[test]
fn rpm_validates_single_file_package() {
    gate("rpm", has_rpm());
    if !has_rpm() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let m = simple_manifest(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let rpm = arx_pack::build_rpm(&m, &out).unwrap();

    let info = Command::new("rpm")
        .args([
            "-qp",
            "--queryformat",
            "%{NAME} %{VERSION} %{ARCH} %{SUMMARY}",
            rpm.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(info.status.success(), "rpm -qp failed");
    let s = String::from_utf8_lossy(&info.stdout);
    assert!(s.contains("hello"), "rpm -qp missing name: {s}");
    assert!(s.contains("1.0"), "rpm -qp missing version: {s}");
    assert!(s.contains("x86_64"), "rpm -qp missing arch: {s}");

    let files = Command::new("rpm")
        .args(["-qlp", rpm.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(files.status.success(), "rpm -qlp failed");
    let f = String::from_utf8_lossy(&files.stdout);
    assert!(
        f.contains("/usr/bin/hello"),
        "rpm -qlp missing the file:\n{f}"
    );
}

// ---------------------------------------------------------------------------
// Nix / Guix definition validation (ADR-0030)
// ---------------------------------------------------------------------------

fn has_command(name: &str, version_flag: &str) -> bool {
    Command::new(name)
        .arg(version_flag)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Parsing a definition needs only the Nix parser: no store writes, no nixpkgs.
#[test]
fn nix_instantiate_parses_generated_expression() {
    if !has_command("nix-instantiate", "--version") {
        eprintln!("skipping — nix-instantiate not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let manifest = multi_file_manifest(dir.path());
    let out = dir.path().join("def");
    std::fs::create_dir(&out).unwrap();
    let expression = arx_pack::build_nix(&manifest, &out).unwrap();

    let parsed = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(&expression)
        .output()
        .unwrap();
    assert!(
        parsed.status.success(),
        "nix-instantiate --parse rejected the generated expression:\n{}",
        String::from_utf8_lossy(&parsed.stderr)
    );

    // A destination containing the two sequences that mean something inside a
    // Nix indented string must survive as literal shell text.
    let quoted = Manifest::from_toml_str(&format!(
        "name = \"quoted\"\nversion = \"1\"\narch = \"amd64\"\n\
         maintainer = \"T <t@localhost>\"\ndescription = \"quoted path\"\n\
         license = \"MIT\"\n[[files]]\nsource = \"{}\"\n\
         dest = \"/usr/share/it''s/${{x}}\"\nmode = \"0644\"\n",
        dir.path().join("app").display()
    ))
    .unwrap();
    let quoted_expression = arx_pack::build_nix(&quoted, &out).unwrap();
    let parsed = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(&quoted_expression)
        .output()
        .unwrap();
    assert!(
        parsed.status.success(),
        "nix-instantiate --parse rejected a quoted destination:\n{}",
        String::from_utf8_lossy(&parsed.stderr)
    );
}

/// Full Nix build: evaluates the expression against nixpkgs and checks that the
/// declared payload lands in `$out` byte for byte. `dontFixup` is what keeps the
/// bytes exact; without it stdenv rewrites shebangs and moves `sbin` into `bin`.
/// Opt-in because it writes to the Nix store and needs a usable `<nixpkgs>`; run
/// it in a disposable container.
#[test]
fn nix_build_installs_the_manifest_payload() {
    if std::env::var("PACK_NIX_BUILD_E2E").is_err() {
        eprintln!("skipping — set PACK_NIX_BUILD_E2E=1 (requires nix + nixpkgs)");
        return;
    }
    assert!(
        has_command("nix-build", "--version"),
        "PACK_NIX_BUILD_E2E is set but nix-build is not on PATH"
    );

    let dir = tempfile::tempdir().unwrap();
    let hello = dir.path().join("hello");
    std::fs::write(&hello, b"#!/bin/sh\necho hello\n").unwrap();
    let daemon = dir.path().join("daemon");
    std::fs::write(&daemon, b"#!/bin/sh\necho daemon\n").unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(assets.join("tool"), b"#!/bin/sh\necho tool\n").unwrap();
    let manifest = Manifest::from_toml_str(&format!(
        "name = 'hello'\nversion = '1.0'\narch = 'amd64'\n\
         maintainer = 'T <t@localhost>'\ndescription = 'A friendly greeter'\n\
         license = 'MIT'\n\
         [[files]]\nsource = '{}'\ndest = '/usr/bin/hello'\nmode = '0755'\n\
         [[files]]\nsource = '{}'\ndest = '/usr/sbin/daemon'\nmode = '0755'\n\
         [[dirs]]\nsource = '{}'\ndest = '/usr/libexec'\nfile_mode = '0755'\ndir_mode = '0555'\n",
        hello.display(),
        daemon.display(),
        assets.display()
    ))
    .unwrap();
    let out = dir.path().join("def");
    std::fs::create_dir(&out).unwrap();
    let expression = arx_pack::build_nix(&manifest, &out).unwrap();

    let wrapper = dir.path().join("default.nix");
    std::fs::write(
        &wrapper,
        format!(
            "let pkgs = import <nixpkgs> {{}}; in import {} {{ inherit (pkgs) lib stdenv; }}\n",
            expression.display()
        ),
    )
    .unwrap();

    let build = Command::new("nix-build")
        .args(["--no-out-link"])
        .arg(&wrapper)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "nix-build rejected the generated expression:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let store_path = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let installed = Path::new(&store_path).join("bin/hello");
    assert_eq!(
        std::fs::read(&installed).unwrap(),
        b"#!/bin/sh\necho hello\n",
        "installed payload differs for {store_path}"
    );
    let daemon_path = Path::new(&store_path).join("sbin/daemon");
    assert_eq!(
        std::fs::read(&daemon_path).unwrap(),
        b"#!/bin/sh\necho daemon\n",
        "sbin payload differs for {store_path}"
    );
    assert!(
        !Path::new(&store_path).join("bin/daemon").exists(),
        "sbin must not be moved into bin"
    );
    let tool_path = Path::new(&store_path).join("libexec/tool");
    assert_eq!(
        std::fs::read(&tool_path).unwrap(),
        b"#!/bin/sh\necho tool\n",
        "a read-only directory mode must be applied after the payload is installed"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        std::fs::metadata(&installed).unwrap().permissions().mode() & 0o111,
        0,
        "manifest executable mode did not survive the install"
    );
}

/// Guix evaluation of the generated definition; a dry run lowers it to
/// derivations without building anything.
#[test]
fn guix_build_dry_run_accepts_generated_definition() {
    if !has_command("guix", "--version") {
        eprintln!("skipping — guix not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let manifest = simple_manifest(dir.path());
    let out = dir.path().join("def");
    std::fs::create_dir(&out).unwrap();
    let definition = arx_pack::build_guix(&manifest, &out).unwrap();

    let dry = Command::new("guix")
        .args(["build", "--dry-run", "-f"])
        .arg(&definition)
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "guix build --dry-run rejected the generated definition:\n{}",
        String::from_utf8_lossy(&dry.stderr)
    );
}

/// Full Guix build: checks that the declared payload lands in the store output
/// byte for byte and keeps its paths. Opt-in because it needs a Guix store with
/// substitutes (authorized signing keys) or all inputs pre-built.
#[test]
fn guix_build_installs_the_manifest_payload() {
    if std::env::var("PACK_GUIX_BUILD_E2E").is_err() {
        eprintln!("skipping — set PACK_GUIX_BUILD_E2E=1 (requires guix + substitutes)");
        return;
    }
    assert!(
        has_command("guix", "--version"),
        "PACK_GUIX_BUILD_E2E is set but guix is not on PATH"
    );

    let dir = tempfile::tempdir().unwrap();
    let hello = dir.path().join("hello");
    std::fs::write(&hello, b"#!/bin/sh\necho hello\n").unwrap();
    let daemon = dir.path().join("daemon");
    std::fs::write(&daemon, b"#!/bin/sh\necho daemon\n").unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(assets.join("tool"), b"#!/bin/sh\necho tool\n").unwrap();
    let manifest = Manifest::from_toml_str(&format!(
        "name = 'hello'\nversion = '1.0'\narch = 'amd64'\n\
         maintainer = 'T <t@localhost>'\ndescription = 'A friendly greeter'\n\
         license = 'MIT'\n\
         [[files]]\nsource = '{}'\ndest = '/usr/bin/hello'\nmode = '0755'\n\
         [[files]]\nsource = '{}'\ndest = '/usr/sbin/daemon'\nmode = '0755'\n\
         [[dirs]]\nsource = '{}'\ndest = '/usr/libexec'\nfile_mode = '0755'\ndir_mode = '0555'\n",
        hello.display(),
        daemon.display(),
        assets.display()
    ))
    .unwrap();
    let out = dir.path().join("def");
    std::fs::create_dir(&out).unwrap();
    let definition = arx_pack::build_guix(&manifest, &out).unwrap();

    let build = Command::new("guix")
        .args(["build", "--no-grafts", "--quiet", "-f"])
        .arg(&definition)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "guix build rejected the generated definition:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let store_path = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let installed = Path::new(&store_path).join("bin/hello");
    assert_eq!(
        std::fs::read(&installed).unwrap(),
        b"#!/bin/sh\necho hello\n",
        "installed payload differs for {store_path}"
    );
    let daemon_path = Path::new(&store_path).join("sbin/daemon");
    assert_eq!(
        std::fs::read(&daemon_path).unwrap(),
        b"#!/bin/sh\necho daemon\n",
        "sbin payload differs for {store_path}"
    );
    assert!(
        !Path::new(&store_path).join("bin/daemon").exists(),
        "sbin must not be moved into bin"
    );
    let tool_path = Path::new(&store_path).join("libexec/tool");
    assert_eq!(
        std::fs::read(&tool_path).unwrap(),
        b"#!/bin/sh\necho tool\n",
        "a read-only directory mode must be applied after the payload is installed"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        std::fs::metadata(&installed).unwrap().permissions().mode() & 0o111,
        0,
        "manifest executable mode did not survive the install"
    );
}

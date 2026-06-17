//! Build script: stamp git sha / build time / rustc into the binary via vergen.
//!
//! Tolerant of a repository with no commits (or no git at all): vergen supplies
//! the build timestamp and rustc info, and we resolve the git sha ourselves so
//! the no-commit case shows a clean `nogit` instead of vergen's idempotent
//! placeholder.

use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, RustcBuilder};

fn main() {
    let mut emitter = Emitter::default();
    if let Ok(build) = BuildBuilder::all_build() {
        let _ = emitter.add_instructions(&build);
    }
    if let Ok(cargo) = CargoBuilder::all_cargo() {
        let _ = emitter.add_instructions(&cargo);
    }
    if let Ok(rustc) = RustcBuilder::all_rustc() {
        let _ = emitter.add_instructions(&rustc);
    }
    // Never fail the build over version metadata.
    let _ = emitter.emit();

    // Resolve a short git sha ourselves (vergen emits a placeholder when there
    // are no commits yet). Printed last so it is the value `env!` sees.
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nogit".to_string());
    println!("cargo:rustc-env=VERGEN_GIT_SHA={sha}");
    // Re-stamp the sha when HEAD moves.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}

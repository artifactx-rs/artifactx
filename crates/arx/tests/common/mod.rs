use std::path::PathBuf;
use std::process::Command;

/// Return the `arx` binary built for integration tests.
///
/// Cargo normally exposes this through `CARGO_BIN_EXE_arx`, but stale target
/// directories can embed an old absolute path. Fall back to the binary next to
/// the current test executable so moving/copying the checkout fails clearly
/// instead of trying to execute another worktree's path.
pub fn arx_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_arx") {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }

    let exe = std::env::current_exe().expect("current test executable path");
    let deps_dir = exe.parent().expect("test executable lives in deps dir");
    let profile_dir = deps_dir.parent().expect("deps dir lives under profile dir");
    let candidate = profile_dir.join(format!("arx{}", std::env::consts::EXE_SUFFIX));
    assert!(
        candidate.exists(),
        "arx binary not found: CARGO_BIN_EXE_arx={:?}, fallback={}",
        std::env::var("CARGO_BIN_EXE_arx").ok(),
        candidate.display()
    );
    candidate
}

pub fn arx_command() -> Command {
    Command::new(arx_bin())
}

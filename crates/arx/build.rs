//! Build script: stamp git sha / build time / rustc into the binary.
//!
//! This intentionally uses only the standard library and external tools that
//! Cargo already depends on (`git`, `rustc`) so a cold build does not compile a
//! large build-dependency graph just to produce `arx --version` metadata.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace_root = manifest_dir
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../.."));

    println!(
        "cargo:rustc-env=VERGEN_GIT_SHA={}",
        git_sha(&workspace_root)
    );
    println!(
        "cargo:rustc-env=VERGEN_BUILD_TIMESTAMP={}",
        build_timestamp()
    );
    println!("cargo:rustc-env=VERGEN_RUSTC_SEMVER={}", rustc_semver());
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    watch_git_head(&workspace_root);
}

fn git_sha(workspace_root: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nogit".to_owned())
}

fn rustc_semver() -> String {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().nth(1).map(str::to_owned))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn build_timestamp() -> String {
    let seconds = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0)
        });
    unix_seconds_to_utc(seconds)
}

fn unix_seconds_to_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    // Howard Hinnant's civil-from-days algorithm. `days_since_unix_epoch` day 0
    // is 1970-01-01 UTC.
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe as i32 + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

fn watch_git_head(workspace_root: &Path) {
    let git_dir = workspace_root.join(".git");
    let git_dir = match resolve_git_dir(&git_dir, workspace_root) {
        Some(path) => path,
        None => return,
    };

    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());
    if let Ok(head) = std::fs::read_to_string(&head_path) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            println!(
                "cargo:rerun-if-changed={}",
                git_dir.join(reference).display()
            );
        }
    }

    let packed_refs = git_dir.join("packed-refs");
    if packed_refs.exists() {
        println!("cargo:rerun-if-changed={}", packed_refs.display());
    }
}

fn resolve_git_dir(path: &Path, workspace_root: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_owned());
    }
    let contents = std::fs::read_to_string(path).ok()?;
    let gitdir = contents.trim().strip_prefix("gitdir: ")?;
    let gitdir = PathBuf::from(gitdir);
    Some(if gitdir.is_absolute() {
        gitdir
    } else {
        workspace_root.join(gitdir)
    })
}

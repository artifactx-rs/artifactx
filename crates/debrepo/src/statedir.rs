//! Generic "versioned directory via atomic symlink flip" — the mechanism behind
//! rollback, shared by the apt (`dists/<dist>`) and yum (`<arch>/repodata`) sides.
//!
//! A logical published thing lives at `link` (a relative symlink). Its immutable
//! states live at `<link_parent>/.states/<link_name>/<NNNNNN>/`. Going live is a
//! single atomic `rename` of the symlink; rollback re-points it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// One retained state of a versioned directory.
#[derive(Debug, Clone)]
pub struct StateInfo {
    pub id: String,
    pub current: bool,
}

fn states_dir(link: &Path) -> Result<PathBuf> {
    let parent = link
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent"))?;
    let name = link
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("path has no name"))?;
    Ok(parent.join(".states").join(name))
}

/// Move `staging` into a new numbered state and atomically point `link` at it.
/// Prunes states beyond `keep` (never the current one). Returns the new state id.
pub fn commit(staging: &Path, link: &Path, keep: usize) -> Result<String> {
    let states = states_dir(link)?;
    std::fs::create_dir_all(&states).with_context(|| format!("creating {}", states.display()))?;
    let id = next_id(&states)?;
    std::fs::rename(staging, states.join(&id))
        .with_context(|| format!("moving staging into state {id}"))?;

    let name = link.file_name().unwrap();
    let target = Path::new(".states").join(name).join(&id);
    swap(link, &target)?;
    prune(&states, link, keep)?;
    Ok(id)
}

/// List retained states for `link`, oldest first.
pub fn list(link: &Path) -> Result<Vec<StateInfo>> {
    let states = states_dir(link)?;
    let cur = current(link);
    Ok(ids(&states)
        .into_iter()
        .map(|id| StateInfo {
            current: Some(&id) == cur.as_ref(),
            id,
        })
        .collect())
}

/// Re-point `link` at a previous state (or `to`). Returns the new current id.
pub fn rollback(link: &Path, to: Option<&str>) -> Result<String> {
    let states = states_dir(link)?;
    let all = ids(&states);
    if all.is_empty() {
        bail!("no published states for {}", link.display());
    }
    let cur = current(link);
    let target = match to {
        Some(id) => {
            if !all.iter().any(|x| x == id) {
                bail!("state {id} does not exist");
            }
            id.to_string()
        }
        None => {
            let pos = cur.as_ref().and_then(|c| all.iter().position(|x| x == c));
            match pos {
                Some(0) | None => bail!("no earlier state to roll back to"),
                Some(p) => all[p - 1].clone(),
            }
        }
    };
    let name = link.file_name().unwrap();
    swap(link, &Path::new(".states").join(name).join(&target))?;
    Ok(target)
}

fn next_id(states: &Path) -> Result<String> {
    let mut max = 0u64;
    if states.is_dir() {
        for entry in std::fs::read_dir(states)? {
            if let Ok(n) = entry?.file_name().to_string_lossy().parse::<u64>() {
                max = max.max(n);
            }
        }
    }
    Ok(format!("{:06}", max + 1))
}

/// Atomically repoint `link` at `target`, replacing a symlink or a pre-symlink
/// real directory (migration).
fn swap(link: &Path, target: &Path) -> Result<()> {
    #[cfg(not(unix))]
    {
        let _ = (link, target);
        anyhow::bail!("symlink-based publish currently requires a Unix platform");
    }
    #[cfg(unix)]
    {
        let parent = link.parent().unwrap();
        let tmp = parent.join(format!(
            ".{}.newlink",
            link.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(target, &tmp)
            .with_context(|| format!("creating symlink {}", tmp.display()))?;
        match std::fs::symlink_metadata(link) {
            Ok(meta) if meta.file_type().is_symlink() => {}
            Ok(meta) if meta.is_dir() => {
                std::fs::remove_dir_all(link).ok();
            }
            Ok(_) => {
                std::fs::remove_file(link).ok();
            }
            Err(_) => {}
        }
        std::fs::rename(&tmp, link)
            .with_context(|| format!("swapping symlink {}", link.display()))?;
        Ok(())
    }
}

fn current(link: &Path) -> Option<String> {
    let target = std::fs::read_link(link).ok()?;
    target.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn ids(states: &Path) -> Vec<String> {
    let mut v: Vec<(u64, String)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(states) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(n) = name.parse::<u64>() {
                v.push((n, name));
            }
        }
    }
    v.sort_unstable_by_key(|(n, _)| *n);
    v.into_iter().map(|(_, s)| s).collect()
}

fn prune(states: &Path, link: &Path, keep: usize) -> Result<()> {
    let cur = current(link);
    let all = ids(states);
    if all.len() <= keep {
        return Ok(());
    }
    let keep_set: std::collections::HashSet<&String> =
        all.iter().rev().take(keep).chain(cur.iter()).collect();
    for id in &all {
        if !keep_set.contains(id) {
            std::fs::remove_dir_all(states.join(id)).ok();
        }
    }
    Ok(())
}

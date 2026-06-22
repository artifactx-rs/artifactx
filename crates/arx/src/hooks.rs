//! Configurable lifecycle hooks for client-visible repository state changes.
//!
//! Hooks deliberately use `command` + `args` rather than shell strings. This
//! keeps public examples portable and avoids adding a shell parser or invoking a
//! shell implicitly. Operators who want shell features can opt into them by
//! configuring `command = "sh"` and explicit arguments.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{Config, HookCommand};

#[derive(Debug, Clone, Copy)]
pub enum HookEvent {
    PrePublish,
    PostPublish,
    PreExport,
    PostExport,
    PreRollback,
    PostRollback,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::PrePublish => "pre_publish",
            HookEvent::PostPublish => "post_publish",
            HookEvent::PreExport => "pre_export",
            HookEvent::PostExport => "post_export",
            HookEvent::PreRollback => "pre_rollback",
            HookEvent::PostRollback => "post_rollback",
        }
    }
}

#[derive(Debug, Default)]
pub struct HookContext {
    env: BTreeMap<String, String>,
}

impl HookContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

pub fn run(root: &Path, cfg: &Config, event: HookEvent, context: &HookContext) -> Result<()> {
    for (index, hook) in hooks_for(cfg, event).iter().enumerate() {
        run_one(root, event, index, hook, context)?;
    }
    Ok(())
}

fn hooks_for(cfg: &Config, event: HookEvent) -> &[HookCommand] {
    match event {
        HookEvent::PrePublish => &cfg.hooks.pre_publish,
        HookEvent::PostPublish => &cfg.hooks.post_publish,
        HookEvent::PreExport => &cfg.hooks.pre_export,
        HookEvent::PostExport => &cfg.hooks.post_export,
        HookEvent::PreRollback => &cfg.hooks.pre_rollback,
        HookEvent::PostRollback => &cfg.hooks.post_rollback,
    }
}

fn run_one(
    root: &Path,
    event: HookEvent,
    index: usize,
    hook: &HookCommand,
    context: &HookContext,
) -> Result<()> {
    if hook.command.trim().is_empty() {
        bail!("hook {}[{}] has an empty command", event.as_str(), index);
    }

    let output = Command::new(&hook.command)
        .args(&hook.args)
        .current_dir(root)
        .env("ARX_HOOK", event.as_str())
        .env("ARX_ROOT", root)
        .envs(context.env.iter())
        .output()
        .with_context(|| {
            format!(
                "running hook {}[{}]: {}",
                event.as_str(),
                index,
                hook.command
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = first_non_empty(stderr.trim(), stdout.trim()).unwrap_or("no output");
        bail!(
            "hook {}[{}] failed with status {}: {}",
            event.as_str(),
            index,
            output.status,
            detail
        );
    }
    Ok(())
}

fn first_non_empty<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    if !a.is_empty() {
        Some(a)
    } else if !b.is_empty() {
        Some(b)
    } else {
        None
    }
}

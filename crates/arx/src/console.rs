//! A2UI console messages for `arx serve`.
//!
//! The console is a deterministic "agent": it reads the same pool and config as
//! the rest of the server, then emits A2UI v1.0 messages. It never scans the
//! repository from the browser and never performs a write.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::config::Config;
use crate::pool;

const SURFACE_ID: &str = "artifactx-console";
const BASIC_CATALOG: &str = "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json";
const MAX_ROWS: usize = 50;

/// Initial overview surface: repository summary plus the first package page.
pub fn overview(root: &Path, cfg: &Config) -> Result<Value> {
    let entries = load_entries(root, cfg, None)?;
    let (summary, packages, activity) = state(root, cfg, &entries);
    Ok(json!([surface_message(&summary, packages, activity)]))
}

/// Replace the package page after a search. The repository-wide summary stays
/// unchanged so a filter does not look like the repository shrank.
pub fn search(root: &Path, cfg: &Config, query: &str) -> Result<Value> {
    let entries = load_entries(root, cfg, Some(query))?;
    let (_, packages, _) = state(root, cfg, &entries);
    Ok(json!([
        {
            "version": "1.0",
            "updateDataModel": {
                "surfaceId": SURFACE_ID,
                "path": "/query",
                "value": query
            }
        },
        {
            "version": "1.0",
            "updateDataModel": {
                "surfaceId": SURFACE_ID,
                "path": "/packages",
                "value": packages
            }
        }
    ]))
}

fn load_entries(root: &Path, cfg: &Config, query: Option<&str>) -> Result<Vec<pool::Entry>> {
    let apt_pool_root = cfg.checked_apt_pool_root(root)?;
    let yum_base = cfg.checked_yum_base(root)?;
    pool::search(
        &apt_pool_root,
        &yum_base,
        pool::SearchOptions {
            query,
            name_prefix: None,
            version: None,
            arch: None,
            scope: None,
            apt: false,
            yum: false,
        },
    )
    .context("searching the package pool")
}

fn state(root: &Path, cfg: &Config, entries: &[pool::Entry]) -> (Value, Value, Value) {
    let repo = repo_name(cfg);
    let last_publish = latest_publish(root, cfg);
    let last_label = last_publish
        .map(relative_time)
        .unwrap_or_else(|| "never".into());
    let apt_components: BTreeSet<&str> = entries
        .iter()
        .filter(|entry| entry.kind == pool::Kind::Apt)
        .map(|entry| entry.scope.as_str())
        .collect();
    let yum_repos: BTreeSet<&str> = entries
        .iter()
        .filter(|entry| entry.kind == pool::Kind::Yum)
        .map(|entry| entry.scope.as_str())
        .collect();

    let summary = json!({
        "repo": repo,
        "packages": entries.len(),
        "apt_components": apt_components.len(),
        "yum_repos": yum_repos.len(),
        "last_publish": last_label,
        "signed": if cfg.signing.enabled { "Signing enabled" } else { "Signing disabled" },
        "healthy": "Read-only",
    });
    let packages = Value::Array(
        entries
            .iter()
            .take(MAX_ROWS)
            .map(|entry| {
                let status = match last_publish {
                    Some(published) if entry.mtime <= published => "published",
                    Some(_) => "new",
                    None => "pooled",
                };
                json!({
                    "name": entry.name,
                    "version": entry.version,
                    "arch": entry.arch,
                    "scope": entry.scope,
                    "kind": match entry.kind {
                        pool::Kind::Apt => "deb",
                        pool::Kind::Yum => "rpm",
                    },
                    "status": status,
                })
            })
            .collect(),
    );
    let activity = json!([
        {
            "title": if last_publish.is_some() { "Metadata present" } else { "No publish yet" },
            "detail": if last_publish.is_some() { "apt + yum indexes are available" } else { "Run arx publish to create indexes" },
            "time": last_label.clone(),
        }
    ]);
    (summary, packages, activity)
}

fn repo_name(cfg: &Config) -> String {
    cfg.apt_release().label.clone()
}

fn latest_publish(root: &Path, cfg: &Config) -> Option<SystemTime> {
    [
        cfg.checked_apt_dists(root).ok(),
        cfg.checked_yum_base(root).ok(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|dir| latest_metadata(&dir))
    .max()
}

fn latest_metadata(dir: &Path) -> Option<SystemTime> {
    let mut latest = None;
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry
            .path()
            .components()
            .any(|part| part.as_os_str() == ".states")
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name != "InRelease" && name != "repomd.xml" {
            continue;
        }
        if let Some(modified) = entry.metadata().ok().and_then(|meta| meta.modified().ok()) {
            latest = Some(latest.map_or(modified, |current: SystemTime| current.max(modified)));
        }
    }
    latest
}

fn relative_time(time: SystemTime) -> String {
    let seconds = Utc::now()
        .signed_duration_since(DateTime::<Utc>::from(time))
        .num_seconds()
        .max(0);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn surface_message(summary: &Value, packages: Value, activity: Value) -> Value {
    json!({
        "version": "1.0",
        "createSurface": {
            "surfaceId": SURFACE_ID,
            "catalogId": BASIC_CATALOG,
            "components": components(),
            "dataModel": {
                "query": "",
                "summary": summary,
                "packages": packages,
                "activity": activity,
            }
        }
    })
}

fn components() -> Vec<Value> {
    let mut out = vec![
        json!({"id": "root", "component": "Column", "children": ["header", "metrics", "search_row", "body"]}),
        json!({"id": "header", "component": "Row", "children": ["title", "signed", "healthy"], "justify": "spaceBetween"}),
        json!({"id": "title", "component": "Text", "text": {"@path": "/summary/repo"}, "weight": 3.0}),
        json!({"id": "signed", "component": "Text", "text": {"@path": "/summary/signed"}, "variant": "caption"}),
        json!({"id": "healthy", "component": "Text", "text": {"@path": "/summary/healthy"}, "variant": "caption"}),
        json!({"id": "metrics", "component": "Row", "children": ["metric_packages", "metric_apt", "metric_yum", "metric_publish"]}),
        json!({"id": "search_row", "component": "Row", "children": ["search", "search_button"], "justify": "spaceBetween"}),
        json!({"id": "search", "component": "TextField", "label": "Search packages", "value": {"@path": "/query"}, "placeholder": "package name…", "weight": 5.0}),
        json!({"id": "search_label", "component": "Text", "text": "Search"}),
        json!({"id": "search_button", "component": "Button", "child": "search_label", "variant": "primary", "weight": 1.0, "action": {"event": {"name": "search_packages", "context": {"query": {"@path": "/query"}}}}}),
        json!({"id": "body", "component": "Row", "children": ["package_card", "activity_card"]}),
    ];
    out.extend(metric("metric_packages", "/summary/packages", "packages"));
    out.extend(metric(
        "metric_apt",
        "/summary/apt_components",
        "apt components",
    ));
    out.extend(metric("metric_yum", "/summary/yum_repos", "yum repos"));
    out.extend(metric(
        "metric_publish",
        "/summary/last_publish",
        "last publish",
    ));
    out.extend([
        json!({"id": "package_card", "component": "Card", "child": "package_column", "weight": 3.0}),
        json!({"id": "package_column", "component": "Column", "children": ["package_heading", "package_header", "package_list"]}),
        json!({"id": "package_heading", "component": "Text", "text": "Packages", "weight": 1.0}),
        json!({"id": "package_header", "component": "Row", "children": ["h_name", "h_version", "h_arch", "h_scope", "h_kind", "h_status"]}),
        json!({"id": "h_name", "component": "Text", "text": "Package", "variant": "caption", "weight": 3.0}),
        json!({"id": "h_version", "component": "Text", "text": "Version", "variant": "caption", "weight": 2.0}),
        json!({"id": "h_arch", "component": "Text", "text": "Arch", "variant": "caption", "weight": 1.0}),
        json!({"id": "h_scope", "component": "Text", "text": "Scope", "variant": "caption", "weight": 1.5}),
        json!({"id": "h_kind", "component": "Text", "text": "Type", "variant": "caption", "weight": 1.0}),
        json!({"id": "h_status", "component": "Text", "text": "Status", "variant": "caption", "weight": 1.5}),
        json!({"id": "package_list", "component": "List", "children": {"path": "/packages", "componentId": "package_row"}}),
        json!({"id": "package_row", "component": "Row", "children": ["p_name", "p_version", "p_arch", "p_scope", "p_kind", "p_status"]}),
        json!({"id": "p_name", "component": "Text", "text": {"@path": "name"}, "weight": 3.0}),
        json!({"id": "p_version", "component": "Text", "text": {"@path": "version"}, "weight": 2.0}),
        json!({"id": "p_arch", "component": "Text", "text": {"@path": "arch"}, "weight": 1.0}),
        json!({"id": "p_scope", "component": "Text", "text": {"@path": "scope"}, "weight": 1.5}),
        json!({"id": "p_kind", "component": "Text", "text": {"@path": "kind"}, "weight": 1.0}),
        json!({"id": "p_status", "component": "Text", "text": {"@path": "status"}, "weight": 1.5}),
        json!({"id": "activity_card", "component": "Card", "child": "activity_column", "weight": 1.0}),
        json!({"id": "activity_column", "component": "Column", "children": ["activity_title", "activity_list"]}),
        json!({"id": "activity_title", "component": "Text", "text": "Publish activity"}),
        json!({"id": "activity_list", "component": "List", "children": {"path": "/activity", "componentId": "activity_row"}}),
        json!({"id": "activity_row", "component": "Column", "children": ["a_title", "a_detail", "a_time"]}),
        json!({"id": "a_title", "component": "Text", "text": {"@path": "title"}}),
        json!({"id": "a_detail", "component": "Text", "text": {"@path": "detail"}, "variant": "caption"}),
        json!({"id": "a_time", "component": "Text", "text": {"@path": "time"}, "variant": "caption"}),
    ]);
    out
}

fn metric(id: &str, path: &str, label: &str) -> Vec<Value> {
    vec![
        json!({"id": id, "component": "Card", "child": format!("{id}_column"), "weight": 1.0}),
        json!({"id": format!("{id}_column"), "component": "Column", "children": [format!("{id}_value"), format!("{id}_label")]}),
        json!({"id": format!("{id}_value"), "component": "Text", "text": {"@path": path}}),
        json!({"id": format!("{id}_label"), "component": "Text", "text": label, "variant": "caption"}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_and_search_emit_a2ui_messages() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("apt/pool")).unwrap();
        std::fs::create_dir_all(temp.path().join("yum")).unwrap();
        let cfg = Config::default();

        let overview = overview(temp.path(), &cfg).unwrap();
        assert_eq!(overview[0]["version"], "1.0");
        assert_eq!(overview[0]["createSurface"]["surfaceId"], SURFACE_ID);
        assert_eq!(
            overview[0]["createSurface"]["dataModel"]["summary"]["packages"],
            0
        );

        let search = search(temp.path(), &cfg, "curl").unwrap();
        let messages = search.as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["updateDataModel"]["path"], "/query");
        assert_eq!(messages[1]["updateDataModel"]["path"], "/packages");
    }
}

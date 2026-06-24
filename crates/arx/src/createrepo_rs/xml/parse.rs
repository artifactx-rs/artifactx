//! Parsers that read existing repodata XML back into [`Package`] structs.
//!
//! Used by `--update` mode to reuse cached metadata for unchanged packages
//! instead of re-reading every RPM from disk.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::createrepo_rs::compression::{
    bzip2_decompress, gzip_decompress, xz_decompress, zstd_decompress,
};
use crate::createrepo_rs::types::{ChangelogEntry, ChecksumType, Dependency, Package, PackageFile};

/// Read a metadata file from disk, decompressing based on extension.
pub fn read_metadata_file(path: &Path) -> Result<Vec<u8>, String> {
    let raw = std::fs::read(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext {
        "gz" => gzip_decompress(&raw).map_err(|e| format!("gzip decode: {e}")),
        "bz2" => bzip2_decompress(&raw).map_err(|e| format!("bzip2 decode: {e}")),
        "xz" => xz_decompress(&raw).map_err(|e| format!("xz decode: {e}")),
        "zst" | "zstd" => zstd_decompress(&raw).map_err(|e| format!("zstd decode: {e}")),
        _ => Ok(raw),
    }
}

fn local_name(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    match s.split_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.to_string(),
    }
}

fn attr_map(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = local_name(attr.key.as_ref());
        let value = match attr.unescape_value() {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&attr.value).into_owned(),
        };
        m.insert(key, value);
    }
    m
}

fn parse_dep_entry(attrs: &HashMap<String, String>) -> Dependency {
    Dependency {
        name: attrs.get("name").cloned().unwrap_or_default(),
        epoch: attrs.get("epoch").and_then(|s| s.parse().ok()),
        version: attrs.get("ver").cloned(),
        release: attrs.get("rel").cloned(),
        flags: attrs.get("flags").cloned().unwrap_or_default(),
        pre: attrs.get("pre").is_some_and(|v| v == "1" || v == "true"),
    }
}

fn checksum_type_from(s: &str) -> ChecksumType {
    match s {
        "md5" => ChecksumType::Md5,
        "sha1" => ChecksumType::Sha1,
        "sha224" => ChecksumType::Sha224,
        "sha256" => ChecksumType::Sha256,
        "sha384" => ChecksumType::Sha384,
        "sha512" => ChecksumType::Sha512,
        _ => ChecksumType::Unknown,
    }
}

/// Parse primary.xml content into a list of [`Package`] structs.
pub fn parse_primary_xml(xml: &[u8]) -> Result<Vec<Package>, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut packages: Vec<Package> = Vec::new();
    let mut cur: Option<Package> = None;
    let mut path: Vec<String> = Vec::new();
    let mut text_buf = String::new();
    let mut current_file_type: Option<String> = None;
    let mut current_pco: Option<&'static str> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let attrs = attr_map(&e);
                match name.as_str() {
                    "package" => {
                        cur = Some(Package::default());
                    }
                    "checksum" if cur.is_some() => {
                        if let Some(p) = cur.as_mut() {
                            if let Some(t) = attrs.get("type") {
                                p.checksum_type = checksum_type_from(t);
                            }
                        }
                    }
                    "rpm:provides" | "provides" => current_pco = Some("provides"),
                    "rpm:requires" | "requires" => current_pco = Some("requires"),
                    "rpm:conflicts" | "conflicts" => current_pco = Some("conflicts"),
                    "rpm:obsoletes" | "obsoletes" => current_pco = Some("obsoletes"),
                    "rpm:suggests" | "suggests" => current_pco = Some("suggests"),
                    "rpm:enhances" | "enhances" => current_pco = Some("enhances"),
                    "rpm:recommends" | "recommends" => current_pco = Some("recommends"),
                    "rpm:supplements" | "supplements" => current_pco = Some("supplements"),
                    "file" => {
                        current_file_type = attrs.get("type").cloned();
                    }
                    _ => {}
                }
                path.push(name);
                text_buf.clear();
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                let attrs = attr_map(&e);
                if let Some(p) = cur.as_mut() {
                    match name.as_str() {
                        "version" => {
                            p.epoch = attrs.get("epoch").and_then(|s| s.parse().ok());
                            if let Some(v) = attrs.get("ver") {
                                p.version = v.clone();
                            }
                            if let Some(r) = attrs.get("rel") {
                                p.release = r.clone();
                            }
                        }
                        "time" => {
                            if let Some(t) = attrs.get("file") {
                                p.time_file = t.parse().unwrap_or(0);
                            }
                            if let Some(t) = attrs.get("build") {
                                p.time_build = t.parse().unwrap_or(0);
                            }
                        }
                        "size" => {
                            if let Some(s) = attrs.get("package") {
                                p.size_package = s.parse().unwrap_or(0);
                            }
                            if let Some(s) = attrs.get("installed") {
                                p.size_installed = s.parse().unwrap_or(0);
                            }
                            if let Some(s) = attrs.get("archive") {
                                p.size_archive = s.parse().unwrap_or(0);
                            }
                        }
                        "location" => {
                            if let Some(href) = attrs.get("href") {
                                p.location = href.clone();
                                p.location_href = Some(href.clone());
                                p.filename = href.clone();
                            }
                        }
                        "header-range" | "rpm:header-range" => {
                            p.header_start = attrs.get("start").and_then(|s| s.parse().ok());
                            p.header_end = attrs.get("end").and_then(|s| s.parse().ok());
                        }
                        "entry" | "rpm:entry" => {
                            let dep = parse_dep_entry(&attrs);
                            match current_pco {
                                Some("provides") => p.provides.push(dep),
                                Some("requires") => p.requires.push(dep),
                                Some("conflicts") => p.conflicts.push(dep),
                                Some("obsoletes") => p.obsoletes.push(dep),
                                Some("suggests") => p.suggests.push(dep),
                                Some("enhances") => p.enhances.push(dep),
                                Some("recommends") => p.recommends.push(dep),
                                Some("supplements") => p.supplements.push(dep),
                                _ => {}
                            }
                        }
                        "file" => {
                            let file_type = attrs.get("type").cloned().unwrap_or_default();
                            // Empty <file/> can have no path; skip
                            // (real entries are <file>path</file> handled in End)
                            let _ = file_type;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Text(t)) => match t.unescape() {
                Ok(s) => text_buf.push_str(&s),
                Err(_) => text_buf.push_str(&String::from_utf8_lossy(t.as_ref())),
            },
            Ok(Event::CData(t)) => {
                text_buf.push_str(&String::from_utf8_lossy(t.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if let Some(p) = cur.as_mut() {
                    let text = text_buf.trim().to_string();
                    match name.as_str() {
                        "name" => p.name = text.clone(),
                        "arch" => p.arch = text,
                        "checksum" => p.pkgid = text.clone(),
                        "summary" => p.summary = Some(text.clone()),
                        "description" => p.description = Some(text.clone()),
                        "packager" => p.packager = Some(text.clone()),
                        "url" => p.url = Some(text.clone()),
                        "rpm:license" | "license" if path.contains(&"format".to_string()) => {
                            p.license = Some(text.clone());
                        }
                        "rpm:vendor" | "vendor" if path.contains(&"format".to_string()) => {
                            p.vendor = Some(text.clone());
                        }
                        "rpm:group" | "group" if path.contains(&"format".to_string()) => {
                            p.group = Some(text.clone());
                        }
                        "rpm:buildhost" | "buildhost" if path.contains(&"format".to_string()) => {
                            p.buildhost = Some(text.clone());
                        }
                        "rpm:sourcerpm" | "sourcerpm" if path.contains(&"format".to_string()) => {
                            p.sourcerpm = Some(text.clone());
                            p.source_pkg = Some(text.clone());
                        }
                        "file" => {
                            if !text.is_empty() {
                                p.files.push(PackageFile {
                                    path: text.clone(),
                                    file_type: current_file_type
                                        .clone()
                                        .unwrap_or_else(|| "file".to_string()),
                                    digest: None,
                                    size: 0,
                                });
                            }
                            current_file_type = None;
                        }
                        "rpm:provides" | "provides" | "rpm:requires" | "requires"
                        | "rpm:conflicts" | "conflicts" | "rpm:obsoletes" | "obsoletes"
                        | "rpm:suggests" | "suggests" | "rpm:enhances" | "enhances"
                        | "rpm:recommends" | "recommends" | "rpm:supplements" | "supplements" => {
                            current_pco = None;
                        }
                        "package" => {
                            if let Some(done) = cur.take() {
                                packages.push(done);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(top) = path.last() {
                    if *top == name {
                        path.pop();
                    }
                }
                text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("primary.xml parse error: {e}")),
            _ => {}
        }
    }

    Ok(packages)
}

/// Parse filelists.xml content into a map keyed by `pkgid` containing the
/// package's full file list.
pub fn parse_filelists_xml(xml: &[u8]) -> Result<HashMap<String, Vec<PackageFile>>, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out: HashMap<String, Vec<PackageFile>> = HashMap::new();
    let mut current_pkgid: Option<String> = None;
    let mut current_files: Vec<PackageFile> = Vec::new();
    let mut current_file_type: Option<String> = None;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let attrs = attr_map(&e);
                match name.as_str() {
                    "package" => {
                        current_pkgid = attrs.get("pkgid").cloned();
                        current_files.clear();
                    }
                    "file" => {
                        current_file_type = attrs.get("type").cloned();
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Ok(Event::Text(t)) => match t.unescape() {
                Ok(s) => text_buf.push_str(&s),
                Err(_) => text_buf.push_str(&String::from_utf8_lossy(t.as_ref())),
            },
            Ok(Event::CData(t)) => {
                text_buf.push_str(&String::from_utf8_lossy(t.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "file" => {
                        let text = text_buf.trim().to_string();
                        if !text.is_empty() {
                            current_files.push(PackageFile {
                                path: text,
                                file_type: current_file_type
                                    .clone()
                                    .unwrap_or_else(|| "file".to_string()),
                                digest: None,
                                size: 0,
                            });
                        }
                        current_file_type = None;
                    }
                    "package" => {
                        if let Some(id) = current_pkgid.take() {
                            out.insert(id, std::mem::take(&mut current_files));
                        }
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("filelists.xml parse error: {e}")),
            _ => {}
        }
    }

    Ok(out)
}

/// Parse other.xml content into a map keyed by `pkgid` containing changelogs.
pub fn parse_other_xml(xml: &[u8]) -> Result<HashMap<String, Vec<ChangelogEntry>>, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out: HashMap<String, Vec<ChangelogEntry>> = HashMap::new();
    let mut current_pkgid: Option<String> = None;
    let mut current_changelogs: Vec<ChangelogEntry> = Vec::new();
    let mut current_author: Option<String> = None;
    let mut current_date: i64 = 0;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let attrs = attr_map(&e);
                match name.as_str() {
                    "package" => {
                        current_pkgid = attrs.get("pkgid").cloned();
                        current_changelogs.clear();
                    }
                    "changelog" => {
                        current_author = attrs.get("author").cloned();
                        current_date = attrs.get("date").and_then(|s| s.parse().ok()).unwrap_or(0);
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Ok(Event::Text(t)) => match t.unescape() {
                Ok(s) => text_buf.push_str(&s),
                Err(_) => text_buf.push_str(&String::from_utf8_lossy(t.as_ref())),
            },
            Ok(Event::CData(t)) => {
                text_buf.push_str(&String::from_utf8_lossy(t.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "changelog" => {
                        current_changelogs.push(ChangelogEntry {
                            author: current_author.take().unwrap_or_default(),
                            date: current_date,
                            content: text_buf.trim().to_string(),
                        });
                        current_date = 0;
                    }
                    "package" => {
                        if let Some(id) = current_pkgid.take() {
                            out.insert(id, std::mem::take(&mut current_changelogs));
                        }
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("other.xml parse error: {e}")),
            _ => {}
        }
    }

    Ok(out)
}

/// Load a complete cache of packages from an existing repodata directory.
///
/// Returns a map keyed by `location_href` so callers can look up cached
/// metadata for an RPM by its repository path.
pub fn load_cached_packages(repodata_dir: &Path) -> Result<HashMap<String, Arc<Package>>, String> {
    let repomd_path = repodata_dir.join("repomd.xml");
    let repomd_xml = std::fs::read(&repomd_path)
        .map_err(|e| format!("read {}: {}", repomd_path.display(), e))?;
    let records = crate::createrepo_rs::xml::repomd::parse_repomd(&repomd_xml)?;

    let parent = repodata_dir
        .parent()
        .ok_or_else(|| "repodata dir has no parent".to_string())?;

    let mut primary_path: Option<std::path::PathBuf> = None;
    let mut filelists_path: Option<std::path::PathBuf> = None;
    let mut other_path: Option<std::path::PathBuf> = None;
    for r in &records {
        let p = parent.join(&r.location);
        match r.type_.as_str() {
            "primary" => primary_path = Some(p),
            "filelists" => filelists_path = Some(p),
            "other" => other_path = Some(p),
            _ => {}
        }
    }

    let primary_path = primary_path.ok_or_else(|| "primary record missing".to_string())?;
    let primary_xml = read_metadata_file(&primary_path)?;
    let mut packages = parse_primary_xml(&primary_xml)?;

    if let Some(p) = filelists_path {
        if p.exists() {
            let xml = read_metadata_file(&p)?;
            let files_by_pkgid = parse_filelists_xml(&xml)?;
            for pkg in &mut packages {
                if let Some(files) = files_by_pkgid.get(&pkg.pkgid) {
                    pkg.files = files.clone();
                }
            }
        }
    }

    if let Some(p) = other_path {
        if p.exists() {
            let xml = read_metadata_file(&p)?;
            let changelogs_by_pkgid = parse_other_xml(&xml)?;
            for pkg in &mut packages {
                if let Some(cl) = changelogs_by_pkgid.get(&pkg.pkgid) {
                    pkg.changelogs = cl.clone();
                }
            }
        }
    }

    let mut by_location = HashMap::new();
    for pkg in packages {
        let key = pkg
            .location_href
            .clone()
            .unwrap_or_else(|| pkg.location.clone());
        by_location.insert(key, Arc::new(pkg));
    }
    Ok(by_location)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_primary() {
        let pkg = Package {
            pkgid: "abc123".to_string(),
            name: "foo".to_string(),
            arch: "x86_64".to_string(),
            version: "1.0".to_string(),
            epoch: Some(0),
            release: "1".to_string(),
            filename: "foo-1.0-1.x86_64.rpm".to_string(),
            location: "foo-1.0-1.x86_64.rpm".to_string(),
            checksum_type: ChecksumType::Sha256,
            checksum: "abc123".to_string(),
            summary: Some("s".to_string()),
            description: Some("d".to_string()),
            time_file: 1234,
            time_build: 5678,
            size_package: 100,
            size_installed: 200,
            size_archive: 300,
            location_href: Some("foo-1.0-1.x86_64.rpm".to_string()),
            ..Default::default()
        };
        let xml =
            crate::createrepo_rs::xml::dump::primary::dump_primary_xml(&[pkg], false).unwrap();
        let parsed = parse_primary_xml(&xml).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "foo");
        assert_eq!(parsed[0].size_package, 100);
        assert_eq!(parsed[0].time_file, 1234);
        assert_eq!(
            parsed[0].location_href.as_deref(),
            Some("foo-1.0-1.x86_64.rpm")
        );
    }
}

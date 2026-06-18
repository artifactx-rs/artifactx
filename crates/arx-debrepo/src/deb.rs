//! `.deb` package inspection: extract and parse the `control` file.
//!
//! A `.deb` is an `ar` archive containing `debian-binary`, `control.tar.{gz,xz,zst}`
//! and `data.tar.*`. We read the control tarball, pull out `./control`, and parse
//! its RFC822 paragraph (the same format apt `Packages` indices use).

use std::io::Read;

use anyhow::{anyhow, bail, Context, Result};

/// Parsed Debian control file: ordered fields preserving the original layout,
/// including folded multi-line values.
#[derive(Debug, Clone)]
pub struct Control {
    fields: Vec<(String, String)>,
}

impl Control {
    /// Case-insensitive field lookup.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn package(&self) -> Result<&str> {
        self.get("Package")
            .ok_or_else(|| anyhow!("control file missing Package field"))
    }

    pub fn version(&self) -> Result<&str> {
        self.get("Version")
            .ok_or_else(|| anyhow!("control file missing Version field"))
    }

    pub fn architecture(&self) -> Result<&str> {
        self.get("Architecture")
            .ok_or_else(|| anyhow!("control file missing Architecture field"))
    }

    /// The ordered fields, for re-emitting into a `Packages` stanza.
    pub fn fields(&self) -> &[(String, String)] {
        &self.fields
    }
}

/// Parse a Debian control paragraph (RFC822 with folded continuation lines).
pub fn parse_control(text: &str) -> Result<Control> {
    let mut fields: Vec<(String, String)> = Vec::new();

    for raw in text.lines() {
        // Stop at a blank line: end of the (single) paragraph.
        if raw.trim().is_empty() {
            if fields.is_empty() {
                continue;
            }
            break;
        }

        let first = raw.as_bytes()[0];
        if first == b' ' || first == b'\t' {
            // Continuation of the previous field's value.
            let (_, value) = fields
                .last_mut()
                .ok_or_else(|| anyhow!("control file starts with a continuation line"))?;
            value.push('\n');
            value.push_str(raw);
        } else {
            let (name, value) = raw
                .split_once(':')
                .ok_or_else(|| anyhow!("malformed control line: {raw:?}"))?;
            fields.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    if fields.is_empty() {
        bail!("empty control file");
    }
    Ok(Control { fields })
}

/// Read and parse the control file from a `.deb` at `path`.
pub fn read_control(path: &std::path::Path) -> Result<Control> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut archive = ar::Archive::new(file);

    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.context("reading ar entry")?;
        let name = String::from_utf8_lossy(entry.header().identifier()).to_string();
        let name = name.trim_end_matches('/'); // ar may pad identifiers
        if let Some(ext) = name.strip_prefix("control.tar") {
            let mut compressed = Vec::new();
            entry
                .read_to_end(&mut compressed)
                .context("reading control tarball")?;
            let tar_bytes = decompress(ext, &compressed)
                .with_context(|| format!("decompressing control.tar{ext}"))?;
            return control_from_tar(&tar_bytes)
                .with_context(|| format!("extracting control from {}", path.display()));
        }
    }
    bail!("{}: no control.tar member found", path.display())
}

/// Decompress a control tarball based on the member's suffix after `control.tar`.
fn decompress(suffix: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match suffix {
        "" => out.extend_from_slice(data),
        ".gz" => {
            flate2::read::GzDecoder::new(data)
                .read_to_end(&mut out)
                .context("gzip")?;
        }
        ".xz" => {
            xz2::read::XzDecoder::new(data)
                .read_to_end(&mut out)
                .context("xz")?;
        }
        ".zst" => {
            zstd::stream::copy_decode(data, &mut out).context("zstd")?;
        }
        other => bail!("unsupported control compression: control.tar{other}"),
    }
    Ok(out)
}

/// Find `./control` (or `control`) inside an uncompressed control tarball.
fn control_from_tar(tar_bytes: &[u8]) -> Result<Control> {
    let mut archive = tar::Archive::new(tar_bytes);
    for entry in archive.entries().context("reading control tar")? {
        let mut entry = entry.context("control tar entry")?;
        let path = entry.path().context("entry path")?.into_owned();
        let name = path.to_string_lossy();
        let name = name.trim_start_matches("./");
        if name == "control" {
            let mut text = String::new();
            entry.read_to_string(&mut text).context("reading control")?;
            return parse_control(&text);
        }
    }
    bail!("control tarball has no control file")
}

/// Extract the list of installed file paths from a `.deb`'s `data.tar` member.
/// Only regular files and symlinks are included (no directories); paths are
/// returned without the leading `./` that tar often prefixes.
pub fn read_data_paths(path: &std::path::Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut archive = ar::Archive::new(file);

    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.context("reading ar entry")?;
        let name = String::from_utf8_lossy(entry.header().identifier()).to_string();
        let name = name.trim_end_matches('/');
        if let Some(ext) = name.strip_prefix("data.tar") {
            let mut compressed = Vec::new();
            entry
                .read_to_end(&mut compressed)
                .context("reading data tarball")?;
            let tar_bytes = decompress(ext, &compressed)
                .with_context(|| format!("decompressing data.tar{ext}"))?;
            return data_paths_from_tar(&tar_bytes)
                .with_context(|| format!("listing data.tar from {}", path.display()));
        }
    }
    bail!("{}: no data.tar member found", path.display())
}

fn data_paths_from_tar(tar_bytes: &[u8]) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut archive = tar::Archive::new(tar_bytes);
    for entry in archive.entries().context("reading data tar")? {
        let entry = entry.context("data tar entry")?;
        let path = entry.path().context("entry path")?.into_owned();
        let kind = entry.header().entry_type();
        if kind != tar::EntryType::Regular && kind != tar::EntryType::Symlink {
            continue;
        }
        let name = path.to_string_lossy();
        let name = name.trim_start_matches("./");
        if !name.is_empty() {
            paths.push(name.to_string());
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_and_continuations() {
        let text = "Package: nginx\nVersion: 1.0-1\nArchitecture: amd64\nDescription: a web server\n very fast\n .\n really\n";
        let c = parse_control(text).unwrap();
        assert_eq!(c.package().unwrap(), "nginx");
        assert_eq!(c.version().unwrap(), "1.0-1");
        assert_eq!(c.architecture().unwrap(), "amd64");
        let desc = c.get("Description").unwrap();
        assert!(desc.starts_with("a web server"));
        assert!(desc.contains("\n very fast"));
    }

    #[test]
    fn case_insensitive_lookup() {
        let c = parse_control("Package: foo\nVersion: 2\nArchitecture: all\n").unwrap();
        assert_eq!(c.get("package"), Some("foo"));
        assert_eq!(c.get("ARCHITECTURE"), Some("all"));
    }

    #[test]
    fn stops_at_blank_line() {
        let c =
            parse_control("Package: foo\nVersion: 1\nArchitecture: all\n\nPackage: bar\n").unwrap();
        assert_eq!(c.package().unwrap(), "foo");
        // The second paragraph must not leak in.
        assert_eq!(c.fields().len(), 3);
    }
}

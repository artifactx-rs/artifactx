//! Native Nix expression builder.
//!
//! `build_nix` writes one nixpkgs-style package expression: a function
//! `{ lib, stdenv }:` returning `stdenv.mkDerivation`. Its install phase writes
//! the manifest payload into `$out` from embedded base64, so the expression is
//! self-contained: no host path, no network fetch, and ArtifactX never runs
//! `nix`.
//!
//! Package-name relationships (`depends`, `conflicts`, ...) and maintainer
//! scripts stay header comments. Deb/rpm names are not Nix attribute names, and
//! Nix has no install-time hooks; guessing either would produce a definition
//! that lies. See ADR-0030.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::declarative::{
    first_line, render_install_script, render_manifest_notes, spdx_ids, system_for_arch,
};
use crate::manifest::Manifest;

/// Build a Nix package expression for `manifest`, writing it into `out_dir`.
///
/// The file is named `{name}-{version}-{system}.nix`, where `{system}` is the
/// Nix system (`x86_64-linux`, ...) or `any` for architecture-independent
/// packages.
pub fn build_nix(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let payload = crate::expand_payload(manifest)?;
    let script = render_install_script(&payload)?;
    let system = system_for_arch(&manifest.arch)?;

    let mut out = render_manifest_notes(manifest, "# ", "Nix");
    out.push('\n');
    out.push_str("{ lib, stdenv }:\n\nstdenv.mkDerivation {\n");
    let _ = writeln!(out, "  pname = {};", nix_string(&manifest.name));
    let _ = writeln!(out, "  version = {};", nix_string(&manifest.version));
    out.push_str("\n  dontUnpack = true;\n  dontConfigure = true;\n  dontBuild = true;\n");
    // stdenv's fixupPhase rewrites shebangs, moves sbin/lib64, and strips
    // binaries. The payload must stay byte-identical to what the archive
    // builders install (ADR-0030).
    out.push_str("  dontFixup = true;\n\n");
    out.push_str("  # Shell body kept at column 0: the base64 heredoc must not be indented.\n");
    out.push_str("  installPhase = ''\nrunHook preInstall\n");
    out.push_str(&nix_indented(&script));
    out.push_str("runHook postInstall\n  '';\n\n");
    out.push_str("  meta = {\n");
    let _ = writeln!(
        out,
        "    description = {};",
        nix_string(first_line(&manifest.description))
    );
    match nix_license(&manifest.license) {
        Some(license) => {
            let _ = writeln!(out, "    license = {license};");
        }
        None => {
            let _ = writeln!(
                out,
                "    # license {:?} has no lib.licenses mapping; set it manually.",
                manifest.license
            );
        }
    }
    let platforms = match system {
        Some(system) => format!("[ {} ]", nix_string(system)),
        None => "lib.platforms.all".to_string(),
    };
    let _ = writeln!(out, "    platforms = {platforms};");
    out.push_str("  };\n}\n");

    let filename = format!(
        "{}-{}-{}.nix",
        manifest.name,
        manifest.version,
        system.unwrap_or("any")
    );
    crate::declarative::write_definition(out_dir, &filename, &out)
}

/// Quote a Nix double-quoted string, escaping interpolation.
fn nix_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Escape a shell body for a Nix indented string.
///
/// `''` starts an escape and `${` starts interpolation; both must be doubled
/// for the string to parse and to survive as literal shell text.
fn nix_indented(body: &str) -> String {
    body.replace("''", "'''").replace("${", "''${")
}

/// Render the `meta.license` value, or `None` when an id has no mapping.
fn nix_license(raw: &str) -> Option<String> {
    let ids = spdx_ids(raw);
    if ids.is_empty() {
        return None;
    }
    let attrs = ids
        .iter()
        .map(|id| nix_license_attr(id).map(|attr| format!("lib.licenses.{attr}")))
        .collect::<Option<Vec<_>>>()?;
    Some(match attrs.as_slice() {
        [single] => single.clone(),
        _ => format!("[ {} ]", attrs.join(" ")),
    })
}

fn nix_license_attr(id: &str) -> Option<&'static str> {
    Some(match id {
        "MIT" => "mit",
        "Apache-2.0" => "asl20",
        "BSD-2-Clause" => "bsd2",
        "BSD-3-Clause" => "bsd3",
        "ISC" => "isc",
        "MPL-2.0" => "mpl20",
        "GPL-2.0-only" => "gpl2Only",
        "GPL-2.0-or-later" => "gpl2Plus",
        "GPL-3.0-only" => "gpl3Only",
        "GPL-3.0-or-later" => "gpl3Plus",
        "LGPL-2.1-only" => "lgpl21Only",
        "LGPL-2.1-or-later" => "lgpl21Plus",
        "LGPL-3.0-only" => "lgpl3Only",
        "LGPL-3.0-or-later" => "lgpl3Plus",
        "AGPL-3.0-only" => "agpl3Only",
        "AGPL-3.0-or-later" => "agpl3Plus",
        "Unlicense" => "unlicense",
        "CC0-1.0" => "cc0",
        "Zlib" => "zlib",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::nix_indented;

    #[test]
    fn indented_strings_escape_quote_pairs_and_interpolation() {
        assert_eq!(nix_indented("it's"), "it's");
        assert_eq!(nix_indented("it''s"), "it'''s");
        assert_eq!(nix_indented("${x}"), "''${x}");
        assert_eq!(nix_indented("\\${x}"), "\\''${x}");
    }
}

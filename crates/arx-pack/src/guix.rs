//! Native Guix package-definition builder.
//!
//! `build_guix` writes one `(package ...)` definition using
//! `trivial-build-system`: its builder puts `bash` and `coreutils` on `PATH`
//! and runs the shared install script, which writes the manifest payload into
//! the output from embedded base64. The definition is self-contained: no
//! `origin`, no network fetch, and ArtifactX never runs `guix`.
//!
//! Package-name relationships and maintainer scripts stay header comments:
//! Guix inputs are package variables, not deb/rpm names, and there are no
//! install-time maintainer hooks. See ADR-0030.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::declarative::{
    first_line, render_install_script, render_manifest_notes, spdx_ids, system_for_arch,
};
use crate::manifest::Manifest;

/// Build a Guix package definition for `manifest`, writing it into `out_dir`.
///
/// The file is named `{name}-{version}-{system}.scm`, where `{system}` is the
/// Guix system (`x86_64-linux`, ...) or `any` for architecture-independent
/// packages.
pub fn build_guix(manifest: &Manifest, out_dir: &Path) -> Result<PathBuf> {
    let payload = crate::expand_payload(manifest)?;
    let script = render_install_script(&payload)?;
    let system = match system_for_arch(&manifest.arch)? {
        Some("armv7l-linux") => Some("armhf-linux"),
        Some("s390x-linux") => bail!(
            "Guix has no s390x system (Guix's %supported-systems has no s390x entry), \
             so a Guix definition cannot be generated for arch {:?} — build the other \
             formats instead",
            manifest.arch
        ),
        other => other,
    };

    let mut out = render_manifest_notes(manifest, ";;; ", "Guix");
    out.push_str("\n\n(use-modules (gnu packages base)\n");
    out.push_str("             (gnu packages bash)\n");
    out.push_str("             (guix build-system trivial)\n");
    out.push_str("             (guix gexp)\n");
    out.push_str("             (guix licenses)\n");
    out.push_str("             (guix packages))\n\n");
    // The install script is a plain file: ASCII-only, so no gexp syntax can
    // appear inside it, and the builder invokes bash on it explicitly.
    out.push_str("(define %arx-install-script\n  (plain-file \"arx-install.sh\"\n              ");
    out.push_str(&scheme_string(&script));
    out.push_str("))\n\n");
    out.push_str("(package\n");
    let _ = writeln!(out, "  (name {})", scheme_string(&manifest.name));
    let _ = writeln!(out, "  (version {})", scheme_string(&manifest.version));
    out.push_str("  (source #f)\n");
    out.push_str("  (build-system trivial-build-system)\n");
    out.push_str("  (arguments\n");
    out.push_str("   (list\n");
    out.push_str("    #:builder\n");
    out.push_str("    #~(begin\n");
    out.push_str("        (setenv \"PATH\"\n");
    out.push_str("                (string-append #$(file-append coreutils \"/bin\") \":\"\n");
    out.push_str("                               #$(file-append bash \"/bin\")))\n");
    out.push_str("        (unless (zero? (system* #$(file-append bash \"/bin/bash\")\n");
    out.push_str("                                #$%arx-install-script))\n");
    out.push_str("          (error \"installing the package payload failed\")))))\n");
    let _ = writeln!(
        out,
        "  (synopsis {})",
        scheme_string(first_line(&manifest.description))
    );
    let _ = writeln!(
        out,
        "  (description {})",
        scheme_string(&manifest.description)
    );
    out.push_str("  (home-page #f)\n");
    match guix_license(&manifest.license) {
        Some(license) => {
            let _ = writeln!(out, "  (license {license})");
        }
        None => {
            let _ = writeln!(
                out,
                "  ;; license {:?} has no Guix license variable mapping; set it manually.",
                manifest.license
            );
            out.push_str("  (license #f)\n");
        }
    }
    if let Some(system) = system {
        let _ = writeln!(out, "  (supported-systems '({}))", scheme_string(system));
    }
    out.push_str(")\n");

    let filename = format!(
        "{}-{}-{}.scm",
        manifest.name,
        manifest.version,
        system.unwrap_or("any")
    );
    crate::declarative::write_definition(out_dir, &filename, &out)
}

/// Quote a Guile string literal, including control characters.
fn scheme_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\x{:x};", u32::from(ch));
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Render the Guix `license` value, or `None` when an id has no mapping.
///
/// `(guix licenses)` exports the names unprefixed (`expat`, `gpl3+`); the
/// `license:` spelling some package modules use is an import-time namespace
/// prefix (`#:prefix license:`), not a different export. The generated file
/// imports the module plainly, so these names work on Guix 1.4 and 1.5 alike.
fn guix_license(raw: &str) -> Option<String> {
    let ids = spdx_ids(raw);
    if ids.is_empty() {
        return None;
    }
    let vars = ids
        .iter()
        .map(|id| guix_license_var(id).map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    Some(match vars.as_slice() {
        [single] => single.clone(),
        _ => format!("(list {})", vars.join(" ")),
    })
}

fn guix_license_var(id: &str) -> Option<&'static str> {
    Some(match id {
        "MIT" => "expat",
        "Apache-2.0" => "asl2.0",
        "BSD-2-Clause" => "bsd-2",
        "BSD-3-Clause" => "bsd-3",
        "ISC" => "isc",
        "MPL-2.0" => "mpl2.0",
        "GPL-2.0-only" => "gpl2",
        "GPL-2.0-or-later" => "gpl2+",
        "GPL-3.0-only" => "gpl3",
        "GPL-3.0-or-later" => "gpl3+",
        "LGPL-2.1-only" => "lgpl2.1",
        "LGPL-2.1-or-later" => "lgpl2.1+",
        "LGPL-3.0-only" => "lgpl3",
        "LGPL-3.0-or-later" => "lgpl3+",
        "AGPL-3.0-only" => "agpl3",
        "AGPL-3.0-or-later" => "agpl3+",
        "CC0-1.0" => "cc0",
        "Unlicense" => "unlicense",
        "Zlib" => "zlib",
        _ => return None,
    })
}

//! Minimal in-crate subset of `createrepo_rs` used by ArtifactX yum support.
//!
//! The crates.io `createrepo_rs 0.1.9` release still depends on `rpm 0.14`,
//! which pulls vulnerable `pgp 0.11`.  Keep this small source-compatible subset
//! local until an upstream release can move to `rpm 0.25+` without reintroducing
//! the old OpenPGP parser.
//!
//! Derived from `createrepo_rs 0.1.9` (GPL-2.0), matching ArtifactX's
//! GPL-2.0-or-later license.

#![allow(dead_code)]

pub mod compression;
pub mod pool;
pub mod rpm;
pub mod types;
pub mod walk;
pub mod xml;

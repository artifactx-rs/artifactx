//! XML dump modules for generating repodata XML files.
//!
//! This module provides pure Rust XML generation using quick-xml
//! to produce primary.xml, filelists.xml, other.xml, and repomd.xml.

pub mod filelists;
pub mod other;
pub mod primary;
pub mod repomd;

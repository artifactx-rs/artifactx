//! `pack` — a pure-Rust packager that builds `.deb` and `.rpm` artifacts from a
//! single TOML manifest, with **no native toolchain required** for the common
//! case (no `dpkg-deb`, no `rpmbuild`, no container runtime).
//!
//! # Why
//!
//! Most packaging needs are "take these files, put them at these paths, attach
//! this metadata". That doesn't require a foreign toolchain — it requires
//! correctly assembling two well-specified archive formats. `pack` does exactly
//! that in pure Rust, so the same code runs on a laptop and in CI, fast and
//! dependency-light.
//!
//! See [`backend`] for the native-first / Docker-fallback philosophy and build
//! hygiene guarantees.
//!
//! # Example
//!
//! ```no_run
//! use pack::Manifest;
//! use std::path::Path;
//!
//! let manifest = Manifest::from_toml_str(r#"
//!     name = "hello"
//!     version = "1.0.0"
//!     arch = "amd64"
//!     maintainer = "Jane Dev <jane@example.com>"
//!     description = "A friendly greeter"
//!     license = "MIT"
//!
//!     [[files]]
//!     source = "build/hello"
//!     dest = "/usr/bin/hello"
//!     mode = "0755"
//! "#).unwrap();
//!
//! let deb = pack::build_deb(&manifest, Path::new("dist")).unwrap();
//! let rpm = pack::build_rpm(&manifest, Path::new("dist")).unwrap();
//! ```

mod backend;
mod deb;
mod manifest;
mod rpm;

pub use backend::{Backend, Format};
pub use deb::build_deb;
pub use manifest::{FileEntry, Manifest, Scripts};
pub use rpm::build_rpm;

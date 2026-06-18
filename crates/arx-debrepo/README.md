# arx-debrepo

A lightweight, **pure-Rust** Debian/apt repository generator. It parses `.deb`
packages and emits `Packages` / `Packages.gz` indices plus a `Release` file —
the "createrepo for apt" you can call as a library.

`arx-debrepo` is **signing-agnostic** by design: it returns the `Release` text so the
caller signs it (`InRelease` / `Release.gpg`) with whatever PGP implementation it
prefers. No async runtime, no native dependencies.

> Part of the [ArtifactX](https://github.com/artifactx-rs/artifactx) workspace, but
> usable on its own. Licensed **MIT OR Apache-2.0**.

## Why

`apt-ftparchive` and `reprepro` are great but require the Debian toolchain;
`debian-packaging` is comprehensive but pulls in an async + S3/HTTP stack. When
all you need is "turn a pool of `.deb`s into a signed, servable repo," `arx-debrepo`
is a few hundred lines of focused, synchronous code with a tiny dependency tree
(`ar`, `tar`, `flate2`/`xz2`/`zstd`, `sha1`/`sha2`/`md-5`).

## Usage

```rust
use arx_debrepo::{build_apt, ReleaseMeta};
use std::path::Path;

// Expects packages under <apt_root>/pool/<component>/*.deb
let meta = ReleaseMeta::new("MyOrg", "MyOrg", "My package repository", "stable");
let build = build_apt(Path::new("./apt"), "stable", "main", &meta)?;

println!("indexed {} packages for {:?}", build.packages, build.architectures);

// `build.release_text` is the exact Release content — sign it yourself:
//   InRelease   = clearsign(release_text)
//   Release.gpg = detached_sign(release_text)
# Ok::<(), anyhow::Error>(())
```

It writes:

```
<apt_root>/dists/<dist>/<component>/binary-<arch>/Packages{,.gz}
<apt_root>/dists/<dist>/Release
```

`Architecture: all` packages are folded into every concrete-architecture index.

## API

- [`build_apt`] — scan a pool, write `Packages`/`Release`, return [`AptBuild`].
- [`ReleaseMeta`] — `Origin`/`Label`/`Description`/`Suite` written into `Release`.
- [`deb`] — `.deb` inspection: [`deb::read_control`] / [`deb::parse_control`].

## Status & limitations

Early but tested (`cargo test -p arx-debrepo`). Not yet implemented: incremental
updates, `by-hash`, `Contents-<arch>` files, multi-component consolidation into a
single `Release`, and `.deb` source packages. Contributions welcome.

## License

Licensed under either of MIT or Apache-2.0 at your option.

<p align="center">
  <img src="res/logo.png" alt="ArtifactX" width="360">
</p>

<h1 align="center">ArtifactX</h1>

<p align="center">
  <b>Build · Package · Publish · Everywhere</b><br>
  An open-source, pure-Rust platform for software packaging and distribution.
</p>

<p align="center">
  <img alt="status" src="https://img.shields.io/badge/status-alpha-blue">
  <img alt="rust" src="https://img.shields.io/badge/rust-2021-blue">
  <img alt="license" src="https://img.shields.io/badge/CLI-GPL--2.0-blue">
  <img alt="lib license" src="https://img.shields.io/badge/debrepo-MIT%2FApache--2.0-blue">
</p>

---

ArtifactX aims to be the **single tool you reach for to package software and run
your own repositories** — apt, yum, and (soon) more — without dragging in native
toolchains, language runtimes, or a heavyweight server stack. One static binary,
zero external services, signed by default.

Today it does **Publish** (repository management). The roadmap fills in
**Package** (a Rust take on [nFPM](https://nfpm.goreleaser.com/) — build `.deb`/
`.rpm`/`.apk` from a manifest, no `dpkg`/`rpmbuild` required) and **Everywhere**
(more formats and backends).

## ✨ Highlights

| | |
| --- | --- |
| 🦀 **Pure Rust, single binary** | No `createrepo`, `apt-ftparchive`, `dpkg`, or Docker required to generate repos. Static musl build drops into a `scratch` image. |
| 📦 **apt + yum in one tool** | Debian `Packages`/`Release` via [`debrepo`](crates/debrepo); RPM `repodata` via [`createrepo_rs`](https://crates.io/crates/createrepo_rs). |
| 🔏 **Signed by default** | v4 RSA PGP (rpgp): `InRelease`/`Release.gpg` for apt, `repomd.xml.asc` for yum. Verified end-to-end against real `apt-get` and `dnf`. |
| 🌐 **Serve built-in** | axum static server with a Prometheus `/metrics` endpoint and structured `tracing` logs. |
| 🧩 **Reusable library** | `debrepo` is a permissively-licensed, signing-agnostic apt-repo generator you can embed anywhere. |

> **Why not Nexus / aptly / Pulp / nfpm / Cloudsmith?** See the product-level
> [competitive teardown](COMPETITORS.md) — what we steal, what we refuse, and the
> position none of them occupy.

## 🚀 Quick start

```bash
cargo install --path crates/arx          # installs the `arx` binary

arx init ./myrepo                         # scaffold + generate signing key
arx add ./nginx_1.0_amd64.deb --root ./myrepo
arx add ./redis-7.x86_64.rpm  --root ./myrepo
arx publish --root ./myrepo               # generate + sign apt & yum metadata
arx serve   --root ./myrepo --addr 0.0.0.0:8080
```

Point a client at it:

```bash
# Debian/Ubuntu
curl -fsSL http://HOST:8080/keys/public.asc | sudo tee /etc/apt/keyrings/arx.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/arx.asc] http://HOST:8080/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/arx.list
sudo apt-get update && sudo apt-get install <pkg>
```

```ini
# RHEL/Rocky/Fedora — /etc/yum.repos.d/arx.repo
[arx]
baseurl=http://HOST:8080/yum/myrepo/$basearch
repo_gpgcheck=1
gpgkey=http://HOST:8080/keys/public.asc
```

See [`crates/arx/README.md`](crates/arx/README.md) for the full CLI reference.

## 🏗 Architecture

A Cargo workspace with a clean library/binary split:

```
artifactx/
├── crates/
│   ├── debrepo/   # 📚 pure-Rust apt repo generator — signing-agnostic, MIT/Apache
│   ├── pack/      # 📦 pure-Rust packager: manifest → .deb/.rpm, no toolchain, MIT/Apache
│   └── arx/       # 🔧 the CLI: orchestrates debrepo + createrepo_rs, signing,
│                  #    HTTP serving, config, observability (GPL-2.0)
```

- **`debrepo`** is deliberately independent and permissively licensed so it can
  be reused (and one day published) on its own.
- **`arx`** links `createrepo_rs` (GPL), so the binary is GPL-2.0-or-later.

## 🗺 Roadmap

The tagline *is* the roadmap:

- [x] **Publish — apt** · signed `Packages`/`Release`/`InRelease`; atomic
      multi-component/dist publish with `by-hash`
- [x] **Publish — yum** · signed `repodata`/`repomd.xml.asc`
- [x] **Package — `pack` (PoC)** · build `.deb`/`.rpm` from a manifest, pure-Rust,
      no native toolchain ([`crates/pack`](crates/pack))
- [x] **Everywhere** · single static binary + `docker compose up`
- [ ] **Publish — hardening** · package delete/GC/retention, `Contents-<arch>`,
      incremental updates, server TLS + auth
- [ ] **Package — more** · Docker fallback backend, `.apk` (Alpine)
- [ ] **Everywhere — more** · S3/object-store backends, mirroring, hosted mode

## ⚠️ Status

Alpha. The core apt/yum generate→sign→serve→install loop is verified end-to-end
against real `apt-get` and `dnf`; publishes are **atomic, multi-component, and
signed** with `by-hash`. Remaining gaps before production use:

- the built-in server has **no TLS/auth** (front it with a reverse proxy);
- signing-key encryption is **opt-in** — set `ARX_KEY_PASSPHRASE` (or
  `--passphrase-file`); without it the key is stored unencrypted;
- there is **no package removal / GC / retention** yet (the pool only grows);
- `pack` is a **PoC** (native `.deb`/`.rpm`; Docker fallback is a stub).

Don't point it at the public internet unguarded yet. See the roadmap.

## 🤝 Contributing

Issues and PRs welcome. `cargo test --workspace` and `cargo clippy --workspace`
must pass. The `debrepo` crate is a good first contribution target — it has a
small, well-tested surface.

## 📄 License

- `arx` (CLI): **GPL-2.0-or-later** (links `createrepo_rs`)
- `debrepo` (library): **MIT OR Apache-2.0**

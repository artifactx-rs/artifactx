# ADR-0010: Cargo.toml-driven packaging (`arx pack` for Rust projects)

- Status: **Accepted & implemented** (single-crate; workspace target = follow-up)
- Date: 2026-06-17

## Review outcome (all three leans accepted)

1. `[package.metadata.arx]` is canonical; cargo-deb/`generate-rpm` compat = later.
2. Default binary asset = `target/release/<name>` (overridable via `files`).
3. Missing binary → error + hint (`pack` does not drive cargo).

Implemented: `pack::Manifest::from_cargo_toml` + `arx pack` reads `./Cargo.toml`
(or `<dir>/Cargo.toml`) by filename; any other `.toml` is a standalone manifest.
Verified on a real single-crate project (zero extra config). **Follow-up:** resolve
the workspace `target/` dir so `arx pack` works at a workspace member without an
explicit `files` source (today, a workspace member sets an explicit asset path).

## Context

`cargo-deb` and `cargo-generate-rpm` nailed one thing: in a Rust project you
shouldn't *repeat* name/version/description/license — derive them from `Cargo.toml`
and add only the packaging-specific bits in `[package.metadata.*]`. Our wedge
audience is exactly Rust devs shipping CLIs, and `arx` itself is a Cargo project, so
this is both great DX and perfect dogfooding (our `packaging/arx.toml` could go away).

Our current `arx pack <manifest.toml>` requires a separate, fully-specified manifest.

## Decision (proposed)

Adopt their **thinking**, not their crate (`pack` keeps building packages itself —
pure-Rust, deterministic, embeddable, and we also publish; those are the moat).

- `arx pack` **with no manifest arg, in a Cargo project** reads `./Cargo.toml`:
  - `name`/`version`/`description`/`license` from `[package]`; `maintainer` from
    `authors` (or `[package.metadata.arx].maintainer`).
  - `[package.metadata.arx]` for packaging fields: `depends`/`conflicts`/`provides`/
    `replaces`, `section`, `scripts`, and `assets`/`files`.
  - **Convention over config (Caddy-style):** if no assets are given, default to
    `target/release/<name>` → `/usr/bin/<name>`, mode 0755. The common case needs
    *zero* extra config: `arx pack` in a built crate just works.
- The standalone `arx pack <manifest.toml>` stays for non-Rust projects.

Implementation: add `pack::Manifest::from_cargo_toml(...)` that maps `[package]` +
`[package.metadata.arx]` onto the existing `Manifest`. `build_deb`/`build_rpm` are
unchanged. `arx pack` picks the source: arg → standalone TOML; else `Cargo.toml`.

## Consequences

- Good: zero/low-config packaging for Rust projects; one source of truth; `arx`'s
  own release simplifies (drop `packaging/arx.toml`).
- Good: still pure-Rust, deterministic, no host tools.
- Bad: another manifest *source* to parse and document (the build path is shared).

## Explicitly NOT adopting (charter)

- **`$auto` dependency detection** (cargo-deb runs `ldd`/`dpkg-shlibdeps`): needs
  host tools and is non-deterministic — against ADR-0005. Maybe a future opt-in.
- **systemd-unit integration**: useful but scope; revisit after the core lands.
- **cargo-deb / cargo-generate-rpm as a dependency**: we render packages ourselves.

## Alternatives considered

- **Keep manifest-only.** Misses the headline DX for our core audience.
- **Use `cargo-deb` as a library.** Cedes control + only does deb + can't publish.

## Open questions for review

1. **Compatibility play:** also *read* `[package.metadata.deb]` (cargo-deb) and
   `[package.metadata.generate-rpm]` so existing users get the other format +
   publishing for free? Strong adoption hook, but two more schemas to track.
   (Lean: support our `[package.metadata.arx]` first; cargo-deb compat = later.)
2. **Default binary asset** `target/release/<name>` — assume release profile, or
   require an explicit `assets`/`--bin-path`? (Lean: default to release, override
   with config/flag.)
3. Should `arx pack` auto-run `cargo build --release` if the binary is missing, or
   just error with a hint? (Lean: error + hint — `pack` doesn't drive cargo.)

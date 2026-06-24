# ADR-0012: `pack` product-readiness — reproducibility, fail-loud, Cargo workspaces

- Status: **Accepted** (revised after adversarial review — see "Review outcome")
- Date: 2026-06-17
- Decided: source_date default = **0** (1970, SOURCE_DATE_EPOCH escape); symlink
  sources = **hard error**; workspace discovery = **vendored walk-up**.

## Context

With the repository pillar product-ready (ADR-0011), `pack` is next. `pack` is the
*Package* pillar and the **moat**: a pure-Rust, embeddable, **deterministic**
packager that also publishes — the gap nfpm/FPM leave open. Its README calls
itself a "proof of concept". An evidence-based audit + direct source inspection
found the moat's headline claim — *deterministic output* — is **half-true**, plus
two correctness footguns and the known Cargo-workspace follow-up.

### What the evidence shows (verified, not assumed)

- **`.deb` is genuinely reproducible across time and host.** Every tar entry uses
  `HeaderMode::Deterministic` + `set_mtime(0)`, entries are sorted, parent dirs are
  emitted deterministically, the `ar` headers use mtime 0 / mode 0644, and
  `flate2`'s gzip header `mtime` defaults to `0` (`GzBuilder` derives `Default`).
  A build-twice byte-comparison passes and is time-independent. ✅
- **`.rpm` is NOT reproducible.** `pack` never sets `source_date`, so the `rpm`
  crate (0.14) writes `RPMTAG_BUILDTIME = Timestamp::now()` (builder.rs:1005-1008)
  and uses each source file's **mtime** for the payload file times
  (builder.rs:748-751). Two builds differ as soon as they cross a one-second
  boundary; a naive "build twice in one test" check passes only by same-second
  luck. **This breaks the moat for half the output.** ❌
- **Silent wrong-architecture.** Unknown `arch` falls back to `amd64`/`x86_64`
  silently (deb.rs:97-98, rpm.rs:108-109) — a mislabeled package the user can't
  see. ❌
- **Cargo workspace members don't work zero-config.** `from_cargo_toml` hardcodes
  the asset source as `target/release/<name>` relative to cwd (manifest.rs:139),
  but a workspace member's binary is built into the **workspace root's** `target/`;
  and workspace-inherited `version`/`license`/`authors` (`x.workspace = true`) are
  rejected (manifest.rs:112-117). `arx` itself still uses an explicit
  `packaging/arx.toml` to dodge this (the ADR-0010 follow-up). ❌
- **`[[bin]].name` mismatch blocks dogfood.** `arx`'s `[package].name = "artifactx"`
  but `[[bin]].name = "arx"` (crates/arx/Cargo.toml:2,11-12). `from_cargo_toml`
  reads only `[package].name` (manifest.rs:109) — it would produce a package named
  `artifactx` looking for `target/release/artifactx`, which doesn't exist (cargo
  emits `target/release/arx`). ADR-0010's "drop `packaging/arx.toml`" bar is
  unreachable without resolving this. Bar #3 is revised accordingly (see Review
  outcome). ❌

## Review outcome (adversarial pass — incorporated)

An independent review verified the rpm non-determinism source-level (builder.rs
three timestamp sites all covered by `source_date`; flate2 default mtime=0 for the
payload compressor) and found the diagnosis correct but the completeness
under-argued. It also identified one CRITICAL dogfood showstopper and five MAJOR
fixes. All folded in below:

- **(CRIT) `[[bin]].name` resolution.** Early `from_cargo_toml` used
  `[package].name` for the binary path; Rust projects with `[[bin]].name !=
  [package].name` (like `arx` itself: package `artifactx` / bin `arx`) produce a
  different binary filename → pack would look for the wrong file. → Decision #3
  resolves `[[bin]].name`: when exactly one `[[bin]]` exists, `source` uses the
  bin name (not the package name); multiple bins → error asking for an explicit
  `files` source.
- **(MAJ) Target-dir resolution.** The "vendored walk-up" must cover
  `--target-dir` → `CARGO_TARGET_DIR` / `CARGO_BUILD_TARGET_DIR` →
  `.cargo/config.toml [build]target-dir`
  → `<workspace-root>/target`, plus `--target <triple>` subdirectory. Any
  ambiguity → fail-loud (don't guess). → Decision #3 lists the explicit order.
  `--profile` selects the profile directory, with `dev` mapping to `debug`.
- **(MAJ) Inherited fields.** `authors.workspace = true` (arx Cargo.toml:5) is a
  TOML table, not an array — `from_cargo_toml`'s `as_array()` silently returns
  `None`, producing `maintainer = "Unknown <unknown@localhost>"`. → Decision #3
  now resolves `{ workspace = true }` for authors/version/license from
  `[workspace.package]`.
- **(MAJ) Real-tool validation must not silently skip in CI.** A skip-if-absent
  test that's a permanent no-op on Apple Silicon (the maintainer's platform)
  offers no trust evidence. → Decision #4 adds `PACK_REQUIRE_REAL_TOOLS=1` for CI
  — tool absence then **fails** the test, not skips. Local dev still skips.
- **(MAJ) Determinism completeness + test assertions.** The review confirmed rpm
  builder.rs has three timestamp sites (`BUILDTIME`, payload mtime, *and* signature
  timestamp at builder.rs:649) — all covered by `source_date`; payload gzip mtime
  is also 0 via `flate2::GzEncoder::new`. The ADR's original argument only named
  two. → Decision #1 now lists all three. Tests assert a *fixed* build-time value
  (not a same-second byte-compare).
- **(MAJ) Unified regular-file gate.** `std::fs::read` follows symlinks silently;
  a FIFO/device would hang if any builder read sources directly. →
  Decision #2 now specifies a **shared** `symlink_metadata(source)
  .file_type().is_file()` check *before* staging, applied once before package
  rendering.
- **(MIN) Test input matrix + deb epoch consistency.** Decision #4 lists the
  minimum test inputs (single file, multi-file across directories, maintainer
  scripts, empty section). Decision #1 notes all 4 mtime sites (tar, ar, gzip,
  data-gzip) must go through one helper. "Not in this ADR" now explicitly
  reiterates: conffiles, `--target`, `--profile`.

1. **Deterministic across time and host for *both* `.deb` and `.rpm`** — proven by
   a test that asserts the *embedded timestamp is a fixed value* (not a
   same-second byte compare).
2. **Real `dpkg`/`rpm` accept the output** — validated, even if gated on tool
   presence.
3. **The Cargo workflow works for the common cases, including workspace members —
   `arx` dogfoods `from_cargo_toml`** (whether by dropping `arx.toml` or by
   complementary `[package.metadata.arx]` + explicit `source` — the target-dir and
   `[[bin]].name`/`authors.workspace` fixes make either path work).
4. **No silent wrong-arch / wrong-file-type** — failures are loud.
5. **Honest scope** — Docker backend stays explicit opt-in; dependency
   auto-detection stays out (ADR-0005).

## Decision (proposed)

### 1. Reproducible by construction (close the moat)

**`.rpm`:** pass `source_date` to `PackageBuilder`. `source_date` is the **single
switch** that determines build-time reproducibility in the rpm crate — it clamps
all three timestamp sites (verified at the source level, not assumed):

| Site | rpm crate location | Without `source_date` | With `source_date = X` |
| --- | --- | --- | --- |
| `RPMTAG_BUILDTIME` | builder.rs:1004-1008 | `Timestamp::now()` | `X` (if X < now) |
| Payload file mtime | builder.rs:748-751 | source file's mtime | `min(X, file_mtime)` |
| Signature timestamp | builder.rs:649-652 | `now()` | `X` (if X < now) |
| Payload gzip header mtime | compressor.rs:43-44 | `0` (flate2 default) | `0` (unchanged) |

Resolve the epoch once as: `SOURCE_DATE_EPOCH` (the reproducible-builds standard)
if set and valid, else **`0`**. With `source_date = 0`, all three sites are
clamped to `0`, so the bytes no longer depend on wall-clock or source-file
timestamps. The payload gzip is already time-fixed by flate2's default. (Note: a
zero epoch shows the RPM `Build Date` as 1970 — this is expected, documented, and
`SOURCE_DATE_EPOCH` is the escape hatch.)

**`.deb`:** honor the same resolved epoch for **all 4 mtime sites** (tar entries,
ar member headers, `control.tar.gz` gzip, `data.tar.gz` gzip) via a single
`resolve_source_epoch()` helper. Default `0` preserves today's byte-identical
output (all four sites already use `0`); a non-zero `SOURCE_DATE_EPOCH` gives
both formats a consistent, auditable timestamp. The helper lives in `lib.rs` and
feeds both `build_deb` and `build_rpm`.

### 2. Fail loud, not silent-wrong

- Unknown `arch` → **error** with the accepted spellings, instead of defaulting to
  `amd64`/`x86_64`. (`all`/`noarch` stay valid.)
- **Unified regular-file gate (shared, pre-staging).** Before a `files[].source`
  reaches either builder, a single `symlink_metadata(source).file_type().is_file()`
  check (not `metadata()` — that follows symlinks, so you can't distinguish them)
  gates every source path. Symlink → error ("source is a symlink, not a regular
  file"); directory → error; device/FIFO → error; each with the type named in the
  message. This means `std::fs::read` (deb) never follows a symlink silently and
  never hangs on a device, and rpm's `with_file` never sees a non-file. Both
  builders share one gate — no drift.

### 3. Cargo workspace support (ADR-0010 follow-up, revised)

**Target dir resolution** — explicit order; any ambiguity → error:

1. `--target-dir` flag.
2. `CARGO_TARGET_DIR` / `CARGO_BUILD_TARGET_DIR` env var.
3. `.cargo/config.toml` `[build] target-dir`, searched upward from the crate dir.
4. Default: `<workspace-root>/target` or `<crate>/target` outside a workspace.

On top of that base, append `release/` by default, or
`<target-triple>/<profile-dir>/` when `--target` is set. `--profile dev` maps to
Cargo's `debug` directory; custom profile names map to their own directory.

- **`[[bin]].name` resolution (CRIT from review).** When the default binary asset
  is derived, use the `[[bin]].name` if exactly one bin target exists (the common
  case for CLI crates); otherwise use `[package].name`. Multiple bin targets →
  error asking for an explicit `files` source (can't guess). This fixes the
  `arx`-specific trap where `[package].name = "artifactx"` but
  `[[bin]].name = "arx"` — without it `from_cargo_toml` looks for the wrong file.

- **Workspace root discovery:** walk up from the crate's `Cargo.toml` directory,
  find the nearest ancestor `Cargo.toml` whose `[workspace]` table includes (or
  whose dir is an ancestor of) this crate. The target dir is resolved from that
  root.

- **Inherited fields (`{ workspace = true }`).** When `version`, `license`, or
  `authors` is a TOML inline table `{ workspace = true }` (not a bare string),
  resolve it from `[workspace.package]` at the workspace root. The current code
  assumes `authors` is an array (`as_array()`) and silently produces
  `"Unknown <unknown@localhost>"` for the table form — that's the `arx` case
  (`authors.workspace = true`, crates/arx/Cargo.toml:5). This fix reads the table
  form and looks up the key in `[workspace.package]`.

- **Dogfood (revised bar #3).** Once the above lands, `arx`'s `Cargo.toml` gains
  `[package.metadata.arx]`. Whether `packaging/arx.toml` can be *deleted*
  depends on `[[bin]].name` resolution working correctly for `arx → artifactx`; if
  the single-bin heuristic doesn't cover it, the explicit `arx.toml` stays as a
  documented alias alongside `Cargo.toml`-driven packaging — both are valid inputs
  and the dogfood is that **the release workflow uses `from_cargo_toml`**, by one
  path or the other.

### 4. Real-tool validation (bar #2 evidence, with mandatory CI presence)

- An integration test that, **when `dpkg-deb` and/or `rpm` are on PATH**, builds a
  package and asserts the real tool reads it correctly: `dpkg-deb --info` /
  `--contents`; `rpm -qp --queryformat '%{NAME} %{VERSION} %{ARCH}'` / `-qlp`.
  On a bare macOS dev box these tools are absent → test is **skipped**, not failed.
- **CI enforces them (MAJ from review).** Set `PACK_REQUIRE_REAL_TOOLS=1` in
  `ci.yml` — when this var is set, missing `dpkg-deb` / `rpm` is a **test
  failure**, not a skip. Without the var (local dev, no tool), skip. Without the
  var but tools present → run.
- **Minimum test input matrix.** The real-tool test covers: single file; multiple
  files across nested directories; a package with maintainer scripts; a package
  with relationships (depends/conflicts); empty `section`/`group`. Each input is
  asserted against both `dpkg-deb` and `rpm`.

## Consequences

- Good: the determinism claim becomes **true and proven** for both formats (all
  three rpm timestamp sites and all four deb mtime sites gated through one
  helper). `SOURCE_DATE_EPOCH` support slots `pack` into reproducible-build
  pipelines at zero cost (default `0` = today's deterministic bytes). Wrong-arch/
  wrong-file footguns become loud, shared-gate errors. Rust workspace members get
  zero-config packaging with explicit, ordered target-dir resolution.
- Good: configuration-file intent is expressed once (`[[files]].config = true`
  or top-level `config_files`) and validated against the expanded regular-file
  payload before rendering. Debian gets `conffiles`; RPM gets
  `%config(noreplace)`; APK currently accepts the marker while emitting ordinary
  payload until Alpine-specific backup semantics are designed.
- Bad / cost: a fixed epoch shows the rpm "Build Date" as 1970 unless
  `SOURCE_DATE_EPOCH` is set (cosmetic; documented). Workspace discovery walks the
  filesystem (small, one-off per `pack` invocation). The `[[bin]].name` heuristic
  can't guess when there are multiple bins (error → explicit `files`).

## Explicitly NOT in this ADR (charter — compete by deleting)

- **Docker backend defaults**: Docker remains explicit opt-in and caller-image
  driven. It is not required for product-ready native packaging.
- **Dependency auto-detection** (`$auto`/`ldd`): out — needs host tools and is
  non-deterministic (ADR-0005).
- **SUID/setgid/capabilities/SELinux contexts**, **streaming very large files**,
  **`.changes`/source packages**: out of scope; revisit on real demand.

## Alternatives considered

- **Default `source_date` to "now" and only honor `SOURCE_DATE_EPOCH`.** Rejected:
  that leaves the *default* non-reproducible — the moat must hold without opt-in.
- **Make `pack` shell out to `rpmbuild` for determinism.** Rejected: cedes the
  pure-Rust/no-host-tools property (ADR-0005), the whole point of `pack`.
- **Keep the silent arch fallback for "convenience".** Rejected: a silently
  mislabeled package is a worse failure than a clear up-front error.

## Open questions for review (decided)

1. **Default epoch** — **decided: `0`** (fully zeroed, 1970 build date).
   `SOURCE_DATE_EPOCH` is the environment escape hatch for a real date, and
   `arx pack --source-date <epoch>` overrides it for one invocation.
2. **Symlink sources** — **decided: hard-error now**, with the type named in the
   message (`symlink_metadata`, not `metadata`, so symlinks are distinguishable).
   Follow semantics can be an opt-in later.
3. **Workspace discovery** — **decided: vendored walk-up** (no `cargo_metadata`
   host-tool dependency). Target-dir resolution order is explicit (see Decision
   #3); ambiguity → fail-loud.

## Future improvements

- Opt-in symlink following; richer rpm metadata (file digests already via the
  crate); `Contents`-style file manifests.

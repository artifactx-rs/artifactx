# ADR-0030: Nix and Guix packaging outputs from `arx pack`

- Status: Accepted (after the charter/competitor pass and independent review recorded below)
- Date: 2026-11-14
- Target: v0.3.x package pillar ([#156](https://github.com/artifactx-rs/artifactx/issues/156))

## Review notes

Charter and competitor pass (2026-11-14):

- **Pillar:** Package only. No new repository format, storage backend, trust
  model, proxy mode, or dashboard; apt/yum indexing is unchanged.
- **5-minute rule:** `arx pack` gains two files in `dist/` and needs no new
  input; the default path is still one manifest.
- **Competitors:** `nfpm`, `cargo-deb`, and `fpm` emit no Nix or Guix output. The
  advantage is one manifest → six artifacts with no `nix`/`guix` toolchain.
  ArtifactX does not become a channel or registry for either ecosystem.
- **Independent implementation review** (2026-11-14) raised two blockers, both
  fixed before acceptance: Guix system names (`armhf-linux`, no s390x) and
  stdenv's `fixupPhase` rewriting payload bytes (`dontFixup = true`).

## Context

`arx pack` builds self-contained binary artifacts (`.deb`, `.rpm`, `.apk`,
`.pkg.tar.zst`) from one manifest. Downstream projects that ship those artifacts
also need to be packaged for Nix and Guix — the trigger was the pgrx packaging
discussion, where a PostgreSQL extension wants one manifest to feed every
channel, not a hand-maintained expression per ecosystem.

The charter says packaging stays inside ArtifactX: no `nix` or `guix`
toolchain, no host inspection, deterministic output. So the question is not
"can we generate a definition" but "which definition, and what does it own".

Two facts constrain the answer:

- **A Nix/Guix definition cannot install to absolute paths.** Both build into a
  prefix (`$out`), so the manifest's `/usr/bin/hello` becomes `$out/bin/hello`.
  Anything that accepted the manifest's absolute paths verbatim would be a lie.
- **Deb/rpm names are not Nix attribute names or Guix variables.** `libc6` is
  `glibc` in nixpkgs and `glibc` in Guix, `postgresql-18` is
  `postgresql_18`, and the mapping is not derivable without asking the
  ecosystem's own index — which is host inspection and network access, both
  out of scope. Guessing produces definitions that either fail to evaluate or
  silently pull the wrong package.

There is no URL in the manifest either, so a `fetchurl`/`origin` expression
would have to invent the artifact location.

## Decision

Two new native builders in `arx-pack`, wired to `arx pack --nix` and
`arx pack --guix`. Each emits **one self-contained definition file**:

| Output | File |
| --- | --- |
| nixpkgs-style expression | `{name}-{version}-{system}.nix`, e.g. `hello-1.2.3-x86_64-linux.nix` |
| Guix package definition | `{name}-{version}-{system}.scm`, e.g. `hello-1.2.3-x86_64-linux.scm` |

`{system}` is the Nix/Guix system name derived from the manifest architecture
(`amd64` → `x86_64-linux`), or `any` for `all`/`noarch`.

### The payload is embedded, exactly like the archive builders

Both builders expand the payload through the same `expand_payload` gate as
`.deb`/`.rpm`: same files, same install paths, same modes, same symlink and
duplicate-destination rejection. The bytes are written into the definition as
base64 and installed by one shell script that both formats share:

```sh
set -eu
if [ "$#" -gt 0 ]; then out=$1; fi
mkdir -p "$out/usr/bin"
chmod 0755 "$out/usr/bin"
base64 --decode > "$out/usr/bin/hello" <<'ARX_PAYLOAD_EOF'
…
ARX_PAYLOAD_EOF
chmod 0755 "$out/usr/bin/hello"
```

Consequences of that choice:

- Installed paths are mapped onto the prefix: the `usr/` segment is dropped
  (`/usr/bin/hello` → `$out/bin/hello`) because `$out` is the prefix those
  ecosystems already use; every other path keeps its location below `$out`
  (`/etc/hello.conf` → `$out/etc/hello.conf`). Two destinations that collide
  after the mapping fail the build instead of silently overwriting.
- Directory modes are applied last, deepest first. Builders run unprivileged, so
  a manifest that declares a read-only directory (`dir_mode = "0555"`) still
  receives its files before the directory stops accepting writes.
- The definition is **reproducible and self-contained**: no host path, no URL,
  no network, no `nix`/`guix` invocation by ArtifactX. `SOURCE_DATE_EPOCH` still
  controls archive-based formats; these definitions carry no timestamp of their
  own (base64 of the payload is byte-stable).
- A definition is **one artifact**, like every other pack output; it is not a
  `payload/` directory that has to travel beside it.
- **Cost:** a definition is ~4/3 the payload size. That is the price of carrying
  the payload. A maintainer who wants a source-based nixpkgs entry should
  replace `src`-less `mkDerivation` with a `fetchurl`-driven one; this output is
  a packaging artifact, not a nixpkgs submission.

### Nix shape

`{ lib, stdenv }:` returning `stdenv.mkDerivation`, which is what
`callPackage`/flake `packages`/overlays expect:

- `dontUnpack`, `dontConfigure`, `dontBuild` — there is no source build.
- `dontFixup = true` — the generic fixup phase would rewrite shebangs, move
  `sbin`/`lib64`, strip binaries, and normalize modes. The payload must stay
  byte-identical to what the archive builders install, so it is disabled. Nix
  store paths are still read-only, so a declared `0755` lands as an executable
  file without the write bit.
- `installPhase` runs the shared script; both ecosystems export `$out` to the
  builder, and the script takes no positional arguments (stdenv evaluates the
  phase string inside `runPhase`, so `$1` is the phase name, not an install
  root).
- `meta.description` from the first description line, `meta.license` mapped from
  SPDX ids (see below), `meta.platforms` from the manifest architecture
  (`lib.platforms.all` for `all`/`noarch`).
- `maintainer`, `depends`, `conflicts`, `provides`, `replaces`, `config_files`
  and maintainer-script names are recorded **verbatim in the file header**.
  Nix has no install-time conffiles or maintainer hooks, and mapping package
  names to attributes would be a guess (see Context).

### Guix shape

`(package …)` with `(build-system trivial-build-system)`, `(source #f)` and a
`#:builder` gexp that puts `bash` and `coreutils` on `PATH` and runs the same
install script.

- `synopsis` and `description` from the manifest description; `home-page` is
  `#f` because the manifest has none.
- `license` mapped to Guix license variables from the same SPDX ids, `#f` when
  there is no trustworthy mapping.
- `supported-systems` from the manifest architecture; omitted for `all`/`noarch`.
  32-bit ARM uses Guix's `armhf-linux` spelling, and `--guix` fails for `s390x`
  because Guix has no s390x system to declare.
- The same header comments as Nix for unmappable fields.

### SPDX → ecosystem license mapping stays small and refundable

Licenses are split on SPDX ` OR ` / ` AND ` and each id is looked up in a fixed
table of unambiguous, long-standing names:

| SPDX id | nixpkgs attr | Guix variable |
| --- | --- | --- |
| MIT | `mit` | `expat` |
| Apache-2.0 | `asl20` | `asl2.0` |
| BSD-2-Clause / BSD-3-Clause | `bsd2` / `bsd3` | `bsd-2` / `bsd-3` |
| ISC | `isc` | `isc` |
| MPL-2.0 | `mpl20` | `mpl2.0` |
| GPL-2.0-only / -or-later | `gpl2Only` / `gpl2Plus` | `gpl2` / `gpl2+` |
| GPL-3.0-only / -or-later | `gpl3Only` / `gpl3Plus` | `gpl3` / `gpl3+` |
| LGPL-2.1-only / -or-later | `lgpl21Only` / `lgpl21Plus` | `lgpl2.1` / `lgpl2.1+` |
| LGPL-3.0-only / -or-later | `lgpl3Only` / `lgpl3Plus` | `lgpl3` / `lgpl3+` |
| AGPL-3.0-only / -or-later | `agpl3Only` / `agpl3Plus` | `agpl3` / `agpl3+` |
| Zlib | `zlib` | `zlib` |
| CC0-1.0 | `cc0` | `cc0` |

Guix exports license variables unprefixed (`expat`, `gpl3+`). The `license:`
spelling seen in some package modules is an import-time namespace prefix
(`#:use-module ((guix licenses) #:prefix license:)`), not a separate export or a
version migration; the generated definition imports `(guix licenses)` plainly,
so the same names work on Guix 1.4 and 1.5.

An unknown id (including `WITH` exception expressions) does not abort the build:
the file keeps a comment with the raw SPDX string, and the field is omitted for
Nix and `#f` for Guix. A wrong license claim is worse than a visible gap, and
both ecosystems' linters surface the gap immediately.

### CLI

- `--nix` and `--guix` are opt-in. The default set stays the four archive
  formats, so existing `arx pack` invocations keep succeeding: the `$out`
  mapping rejects some manifests the archives accept (`/usr/bin/tool` and
  `/bin/tool` collapse to one path) and Guix has no `s390x` system.
- `arx pack --add` leaves the definitions in the output directory with a
  "not supported by arx add" notice, like `.pkg.tar.zst`/`.apk`.

## Consequences

- One manifest now feeds six artifacts, including the two functional-channel
  ecosystems, without adding a toolchain dependency or a host lookup.
- Tests assert the payload round-trips (paths, modes, bytes), determinism, the
  license/platform mapping, and the unmappable-field header.
- Gated real-tool tests parse the Nix expression with `nix-instantiate` and can
  build it (`PACK_NIX_BUILD_E2E=1`); the Guix definition is evaluated with
  `guix build --dry-run` when a Guix installation is present, and built for real
  with `PACK_GUIX_BUILD_E2E=1` (needs substitutes or a store with all inputs).
  CI installs neither toolchain, so those gates skip there — payload rendering
  is covered by the deterministic Rust tests in every run.
- **Compatibility:** no public enum changes. `arx_pack::Format` keeps its four
  archive variants, and `build_nix` / `build_guix` are additive functions, so
  downstream exhaustive `match` expressions keep compiling and no `Breaking`
  release note is needed. The CLI default output set is also unchanged (four
  archive formats). The `Backend` abstraction stays archive-only; definitions
  are rendered through the direct builders (which is what the CLI uses).
- The definition is not a nixpkgs/Guix channel submission: it installs from an
  embedded payload instead of building from source, has no `src` hash, and does
  not map dependencies. Both files say so in their headers.

## Alternatives considered

- **`src = fetchurl { url; sha256; }` / `(origin …)`.** The manifest has no
  URL, and inventing one (or requiring a new manifest field in this change)
  either lies or expands the public manifest format. This is the natural
  follow-up (see below), not the first step.
- **A nixpkgs module/channel snippet instead of a package.** Same dependency
  mapping problem, plus a much larger surface to keep compatible.
- **A `payload/` directory emitted next to the expression.** Breaks "one pack
  command produces one file per format", and makes the artifact easy to split
  up by accident.
- **Shelling out to `nix`/`guix` to render or check.** Violates the charter's
  "no toolchain" rule and makes output host-dependent.

## Future improvements

- Optional manifest fields `source_url` + `sha256` so the definitions can use
  `fetchurl`/`origin` and skip the embedded blob for large payloads.
- Maintainer scripts as ecosystem-native phases once there is a mapping that is
  actually correct (`preInstall`-style build-time hooks are not install-time
  hooks; running user scripts during a sandboxed build is not equivalent).
- Dependency mapping via an explicit user-supplied table, never a guessed one.

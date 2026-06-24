# CLI reference

This reference summarizes the `arx` command surface. Use `arx <command> --help`
for the authoritative option list in your installed version.

## Global options

```text
--log-format <text|json>  Log output format. Default: text
-h, --help                Print help
-V, --version             Print version
```

## Commands

| Command | Purpose |
| --- | --- |
| `arx init [PATH]` | Scaffold a repository with `arx.toml`, directories, and signing key. |
| `arx key` | Generate, import, rotate, revoke, or export signing keys. |
| `arx add <PACKAGES|DIRS>...` | Add `.deb` and `.rpm` package files, or discover them recursively from directories, into the repository pool. |
| `arx publish` | Generate and sign apt/yum repository metadata; optionally export and cut over live public symlinks. |
| `arx publish-dir <DIR>` | Ingest a package drop directory, no-op unchanged inputs, publish, and optionally switch live symlinks. |
| `arx rollback [TARGET]` | Roll a target back to a retained published state. |
| `arx history [TARGET]` | List retained published states. |
| `arx pack [MANIFEST]` | Build `.deb`, `.rpm`, or `.apk` packages from a manifest. |
| `arx push --url <URL> <PACKAGES>...` | Upload packages to a running `arx serve` and publish remotely. |
| `arx rm <NAME>` | Remove packages from the pool, then publish. |
| `arx search [QUERY]` | Search local apt/yum pool entries before GC, remove, promote, or cutover. |
| `arx import <URL>` | Import packages from an existing apt/yum repo. |
| `arx gc [NAME]` | Prune old package versions from the pool, optionally scoped to one package name, then publish. |
| `arx promote --from <FROM> --to <TO> <NAME>` | Promote packages between apt components or yum repos. |
| `arx serve` | Serve the repository tree and API over HTTP. |
| `arx daemonize` | Install/update a systemd `arx serve` unit and generate a write token. |
| `arx mirror <URL>` | Mirror an upstream apt/yum repository. |
| `arx watch [DIR]` | Watch a directory for new packages and auto-publish. |
| `arx compose` | Generate `docker-compose.yml` and `Dockerfile`. |
| `arx export` | Export published repos into legacy-compatible public layouts. |

## Common command chains

Create, add, publish, serve:

```sh
arx init ./repo
arx add dist --root ./repo
arx publish --root ./repo
arx serve --root ./repo
```

`arx add` also accepts directories. Directory inputs are traversed recursively
without following symlinked directories; discovered `.deb` and `.rpm` files are
sorted before processing so output and partial failures are deterministic.
Unrelated files are ignored, and a directory with no supported package files
fails loudly:

```sh
arx add ./dist --root ./repo
arx add ./dist ./more-packages --root ./repo
```

Import, publish, serve:

```sh
arx init ./repo
arx import https://packages.example.com --apt --dist stable --component main --limit 20 --publish --root ./repo
arx serve --root ./repo
```

Build packages and add them:

```sh
arx pack ./arx.toml --out dist
arx add dist --root ./repo
arx publish --root ./repo
```

`arx pack` manifests support explicit `[[files]]` entries and recursive `[[dirs]]`
entries for payload trees. Directory entries are expanded deterministically and
reject symlinks, special files, and duplicate destinations.

For Rust crates, omitting `MANIFEST` reads `./Cargo.toml`; passing a path named
`Cargo.toml` derives package identity from `[package]` and packaging details from
`[package.metadata.arx]`. `pack` never runs `cargo build`: build first, then
point `pack` at the same Cargo output layout if you used non-default settings:

```sh
cargo build --release --target x86_64-unknown-linux-gnu --target-dir build/target
arx pack Cargo.toml --target x86_64-unknown-linux-gnu --target-dir build/target --out dist
```

Cargo binary lookup defaults to the selected crate or workspace `target/`
directory, profile `release`, and the package name unless a single
`[[bin]].name` is present. Multiple binaries require explicit
`[package.metadata.arx]` `files`. Use `--profile <NAME>` for custom profiles;
`--profile dev` maps to Cargo's `target/debug` output directory.

`pack` uses deterministic timestamps. `--source-date <EPOCH>` wins for that
invocation; otherwise `SOURCE_DATE_EPOCH` is honored; otherwise epoch `0` is
used.

Keep the two directory features separate: `arx add <DIR>` and `arx publish-dir`
ingest directories that already contain built `.deb` or `.rpm` packages
(ADR-0019), while `arx pack` `[[dirs]]` installs a directory tree inside a newly
built package payload (ADR-0018).

Operate a production package drop directory:

```sh
arx publish-dir /opt/packages \
  --root /data/arx/prod \
  --apt-live /srv/deb \
  --yum-flat-live /srv/repo \
  --staging-dir /data/arx/public-builds \
  --repo qgnet
```

`publish-dir` records source package fingerprints under `.arx-cache/` and turns
unchanged runs into fast no-ops. External mirror fan-out is opt-in:

```sh
arx publish-dir /opt/packages --root /data/arx/prod --sync-cmd 'systemctl start --no-block sync-srv'
```

Inspect before mutating:

```sh
arx search demo --root ./repo
arx search --name-prefix demo --apt --json --root ./repo
arx search --scope myrepo --arch x86_64 --yum --root ./repo
```

Serve and push:

```sh
ARX_SERVE_TOKEN='change-me' arx serve --root ./repo
arx push --url http://127.0.0.1:8080 --token 'change-me' dist/myapp.deb
```

## Key options by command

### `arx init`

- `--no-key`: create config and directories without generating a key.
- `--key-dir <DIR>`: place generated keys under a custom repo-relative directory.
- `--pool-dir <DIR>`: choose the apt pool subdirectory name.
- `--passphrase-file <FILE>`: encrypt generated key with the file contents.

### `arx key`

Subcommands:

- `generate`
- `rotate`
- `revoke`
- `import <PRIVATE_KEY>`
- `export`

`--passphrase-file` encrypts generated keys or unlocks imported encrypted keys.
If omitted, ArtifactX falls back to `ARX_KEY_PASSPHRASE`.

### `arx publish`

- `--apt`: publish only apt metadata.
- `--yum`: publish only yum metadata.
- `--full`: rebuild all metadata from scratch.
- `--strict`: fail if packages are skipped.
- `--apt-live <PATH>`: after publishing apt metadata, export the apt public layout and switch this live symlink.
- `--yum-flat-live <PATH>`: after publishing yum metadata, export a flat yum layout and switch this live symlink.
- `--staging-dir <DIR>`: parent directory for versioned cutover exports when live paths are set.
- `--repo <REPO>` / `--arch <ARCH>`: select the yum repo/architectures for `--yum-flat-live`.
- `--dry-run`: publish, export, and validate staged live layouts without switching symlinks.
- `--require-signed-rpms`: fail live yum cutover if any staged RPM payload is unsigned.
- `--passphrase-file <FILE>`: unlock encrypted signing key.

Configured `pre_publish` hooks run before metadata changes, and `post_publish`
hooks run after a successful publish. See
[`[hooks]`](config.md#hooks) for command shape and environment variables.

APT publishes also generate `Contents-<arch>` and `Contents-<arch>.gz` indices
so `apt-file`-style clients can search installed file paths. These files are
listed in `Release` and get by-hash copies like `Packages`.

For production public roots that are symlinks, prefer the single-command form:

```sh
arx publish --root ./repo \
  --apt-live ./public/deb \
  --yum-flat-live ./public/repo \
  --staging-dir ./public/.arx-cutovers
```

This uses the same preflight and atomic symlink switching as `arx cutover`.

### `arx publish-dir`

`publish-dir` is the operational wrapper for package drop directories. It scans
a directory for `.deb` and `.rpm` files, adds them to the pool, publishes
metadata, optionally exports live public layouts, and stores source-directory
state so unchanged runs can exit quickly.

- `<DIR>`: package drop directory. Direct children are scanned by default.
- `--recursive`: discover packages recursively below `<DIR>`.
- `--root <DIR>`: ArtifactX repository root.
- `--component <COMPONENT>` / `--repo <REPO>`: destination apt component or yum repo.
- `--state-file <FILE>`: override the no-op state file. Defaults under `.arx-cache/`.
- `--force`: publish even when the source directory state is unchanged.
- `--full`: rebuild metadata from scratch.
- `--apt` / `--yum`: limit the publish to one format.
- `--apt-live <PATH>` / `--yum-flat-live <PATH>` / `--staging-dir <DIR>`: use the same preflighted live symlink cutover as `arx publish`.
- `--dry-run`: validate staged output without switching live symlinks or updating `publish-dir` state.
- `--require-signed-rpms`: fail live yum cutover if any staged RPM payload is unsigned.
- `--sign-rpms`: sign unsigned source RPM payloads with the system `rpm --addsign` backend before ingest. ArtifactX skips already-signed RPMs and verifies each payload is signed.
- `--rpm-sign-cmd <COMMAND>`: optional custom signer for environments that do not use the default RPM signing backend.
- `--sync-cmd <COMMAND>`: optional shell command to run after a successful non-no-op publish. ArtifactX does not enable sync by default.
- `--passphrase-file <FILE>`: unlock encrypted signing key.

Use `--sign-rpms` when your drop directory receives unsigned RPM payloads and
clients require `gpgcheck=1`:

```sh
arx publish-dir /opt/packages --root /data/arx/prod \
  --sign-rpms \
  --require-signed-rpms
```

`--sign-rpms` uses the system RPM signing backend (`rpm --addsign`) and relies on
the operator's normal RPM/GPG key configuration. Use `--rpm-sign-cmd` only for a
custom signer; it receives `ARX_ROOT`, `ARX_SOURCE_DIR`, `ARX_RPM_PATH`, and
`ARX_PACKAGE_PATH`. Both signers are skipped for RPMs that are already signed.

Use `--sync-cmd` only for site-specific fan-out such as rsync, CDN upload, or
`systemctl start --no-block sync-srv`. The command receives `ARX_ROOT`,
`ARX_SOURCE_DIR`, and `ARX_PACKAGE_COUNT`. It is skipped for no-op runs.

### `arx import`

- `--apt` or `--yum`: choose upstream repo format.
- `--dist <DIST>`: apt distribution.
- `--component <COMPONENT>`: apt component or yum repo name.
- `--arch <ARCH>`: apt architecture filter. Default: `amd64`.
- `--limit <N>`: import only the first N packages.
- `--match-name <PREFIX>`: import packages whose names match the prefix.
- `--strict`: fail a yum import if any upstream metadata entry is missing, corrupt, or fails size/checksum validation. Use this for production cutover gates; omit it for best-effort migrations.

For apt imports, ArtifactX reads upstream `dists/<dist>/Release` when available and preserves `Origin`, `Label`, `Suite`, and `Codename` in `arx.toml`; subsequent `publish` keeps those identity fields unless you deliberately edit `[apt.release]`.

### `arx search`

- `[QUERY]`: optional substring match against package names.
- `--name-prefix <PREFIX>`: match package names starting with a prefix.
- `--version <VERSION>`: match an exact package version.
- `--arch <ARCH>`: match an exact architecture.
- `--scope <SCOPE>`: match an apt component or yum repo name.
- `--apt` / `--yum`: restrict to one pool; omitting both scans both.
- `--json`: emit a JSON array of `PackageInfo` objects for scripts.

### `arx gc`

- `[NAME]`: optionally prune only this package name.
- `--name-prefix <PREFIX>`: prune only packages whose names start with this prefix.
- `--keep <N>`: keep this many newest versions per package/scope/arch.
- `--keep-within <DAYS>`: also keep packages newer than this many days.
- `--grace <DAYS>`: defer pruning packages younger than this grace period.
- `--dry-run`: report what would be pruned without deleting.
- `--ignore-rollback-states`: allow pruning files referenced by retained rollback
  states. By default, ArtifactX keeps rollback-referenced files so old states do
  not 404. Use this only after deciding those rollback states may become invalid.
- `--apt` / `--yum`: restrict to one pool; omitting both scans both.

For an operator recipe, see [Prune old packages with GC](../how-to/prune-and-gc.md).

### `arx serve`

- `--root <ROOT>`: canonical ArtifactX repository root and write API state.
  Default: `.`. Static fallback serves this root, so canonical paths such as
  `/apt/...` and `/yum/<repo>/<arch>/...` remain available.
- `--apt-live <DIR>`: also serve an exported apt public layout at `/deb/*`.
  This is for legacy client URLs like `deb https://host/deb stable main` while
  writes still publish into `--root`.
- `--yum-flat-live <DIR>`: also serve an exported flat yum layout at `/repo/*`.
  This is for legacy client URLs like `baseurl=https://host/repo` while writes
  still publish into `--root`.
- `--addr <ADDR>`: listen address. Default comes from `[server].addr`, normally
  `127.0.0.1:8080`.

`arx serve` exposes static apt/yum repository files, optional legacy `/deb` and
`/repo` live mounts, `/metrics`, `/api/v1/...` JSON APIs, `/api/openapi.yaml`,
and `/api/docs` Swagger UI.

Example production-shaped service behind a reverse proxy:

```sh
arx serve --root /data/arx/prod --apt-live /srv/deb --yum-flat-live /srv/repo
```

### `arx daemonize`

- `--root <ROOT>`: repository root served by systemd. Default:
  `/var/lib/arx/repo`.
- `--addr <ADDR>`: listen address written into the unit. Default:
  `127.0.0.1:8080`.
- `--apt-live <DIR>`: add `--apt-live` to the generated `ExecStart` and grant
  the unit read-only access to that directory.
- `--yum-flat-live <DIR>`: add `--yum-flat-live` to the generated `ExecStart`
  and grant the unit read-only access to that directory.
- `--unit <NAME>`: systemd unit name. Default: `arx`.
- `--env-file <PATH>`: environment file for generated `ARX_SERVE_TOKEN`.
  Default: `/etc/arx/arx.env`.
- `--reuse-token`: keep an existing `ARX_SERVE_TOKEN` when present.
- `--enable`: run `systemctl enable`.
- `--start`: run `systemctl restart`; implies enable behavior.
- `--dry-run`: print files/actions without writing or calling systemd.

`arx daemonize` writes a hardened `arx serve` unit, creates a random 32-byte
bearer token for write API access, stores it in a mode `0600` env file, verifies
the unit with `systemd-analyze verify`, and reloads systemd.

### `arx cutover`

Publishes selected metadata, exports fresh legacy-compatible layouts, validates
them, then switches live symlink pointers. Live paths must be absent or symlinks;
ordinary directories are refused so one-time migrations stay explicit.

- `--apt-live <PATH>`: live apt path to switch to the staged `deb` export.
- `--yum-flat-live <PATH>`: live flat yum path to switch to the staged `repo` export.
- `--staging-dir <DIR>`: parent directory for versioned cutover exports. Defaults near the first live path.
- `--repo <REPO>`: yum repo name to export. Defaults to `[yum].repo`.
- `--arch <ARCH>`: limit yum export to one or more architectures.
- `--dry-run`: publish/export/preflight but do not switch live pointers.
- `--no-publish`: cut over currently published metadata.
- `--require-signed-rpms`: fail if any staged RPM payload is unsigned. This is separate from signed yum repository metadata (`repomd.xml.asc`).
- `--passphrase-file <FILE>`: unlock encrypted signing key.

A successful second and later cutover leaves `<live>.previous` pointing to the
previous live target for rollback.

### `arx export`

- `--apt-out <DIR>`: write a fresh apt public tree containing `dists/` and the configured pool directory.
- `--yum-flat-out <DIR>`: write a fresh flat yum public tree containing `*.rpm` and `repodata/`.
- `--repo <REPO>`: yum repo to flatten. Defaults to `[yum].repo`.
- `--arch <ARCH>`: repeatable yum arch filter. Defaults to all arch directories.
- `--passphrase-file <FILE>`: unlock an encrypted signing key when rebuilding exported yum metadata.

The flat yum export intentionally writes gzip metadata (`*.xml.gz`) for CentOS 7 compatibility; it must not be changed to xz-only for production cutovers. Export paths must be fresh versioned directories so operators can atomically switch symlinks and roll back.

### `arx compose`

- `--root <ROOT>`: repository root mounted into the container.
- `--out <DIR>`: output directory for generated files.
- `--addr <ADDR>`: host-side published port source. Default: `0.0.0.0:8080`.

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
| `arx publish` | Generate and sign apt/yum repository metadata. |
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
| `arx mirror <URL>` | Mirror an upstream apt/yum repository. |
| `arx watch [DIR]` | Watch a directory for new packages and auto-publish. |
| `arx compose` | Generate `docker-compose.yml` and `Dockerfile`. |
| `arx export` | Export published repos into legacy-compatible public layouts. |

## Common command chains

Create, add, publish, serve:

```sh
arx init ./repo
arx add dist/*.deb dist/*.rpm --root ./repo
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
arx import https://packages.example.com --apt --dist stable --component main --limit 20 --root ./repo
arx publish --root ./repo
arx serve --root ./repo
```

Build packages and add them:

```sh
arx pack ./arx.toml --out dist
arx add dist --root ./repo
arx publish --root ./repo
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
- `--passphrase-file <FILE>`: unlock encrypted signing key.

### `arx import`

- `--apt` or `--yum`: choose upstream repo format.
- `--dist <DIST>`: apt distribution.
- `--component <COMPONENT>`: apt component or yum repo name.
- `--arch <ARCH>`: apt architecture filter. Default: `amd64`.
- `--limit <N>`: import only the first N packages.
- `--match-name <PREFIX>`: import packages whose names match the prefix.
- `--strict`: fail a yum import if any upstream metadata entry is missing, corrupt, or fails size/checksum validation. Use this for production cutover gates; omit it for best-effort migrations.

For apt imports, ArtifactX reads upstream `dists/<dist>/Release` when available and preserves `Origin`, `Label`, `Suite`, and `Codename` in `arx.toml`; subsequent `publish` keeps those identity fields unless you deliberately edit `[repo]`.

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
  not 404.
- `--apt` / `--yum`: restrict to one pool; omitting both scans both.

### `arx serve`

- `--root <ROOT>`: repository root to serve. Default: `.`.
- `--addr <ADDR>`: listen address. Default comes from `[server].addr`, normally
  `127.0.0.1:8080`.

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

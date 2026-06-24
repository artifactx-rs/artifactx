# ADR-0019: Directory inputs for repository ingestion

- Status: **Accepted**
- Date: 2026-06-22
- Target: v0.2.0
- Related: [GitHub issue #14](https://github.com/artifactx-rs/artifactx/issues/14)

## Context

ArtifactX's repository workflow is intentionally file-first: users can add or
import existing `.deb` and `.rpm` package files into a repository, then publish
metadata from that local pool.

For real migrations, users often start with a directory tree rather than a short
list of package paths:

- a CI `dist/` directory with many package artifacts;
- an exported package repository or staging directory;
- a mixed folder where only package files should be selected;
- repeated local dogfood workflows where pointing at a directory is simpler than
  maintaining a long shell glob.

Today users can often work around this with shell globs, but that pushes behavior
into the shell and makes recursive discovery, mixed file filtering, stable order,
and error reporting inconsistent across platforms.

Issue #14 may be about packaging payload directories (`arx pack`) or about adding
/ importing a directory of already-built packages. This ADR tracks the second
workflow explicitly because it is useful on its own even if issue #14 turns out
to be about `pack`.

## Decision

Accept local package-directory ingestion in v0.2 through two explicit surfaces:

1. `arx add <DIR>` for adding already-built packages to a repository pool.
2. `arx publish-dir <DIR>` for operational package-drop workflows that should
   ingest packages, publish metadata, optionally cut over live public layouts,
   and optionally trigger downstream sync.

Upstream apt/yum repository import remains URL/metadata-oriented because it has
different semantics from copying already-built local package files.

The accepted `arx add` shape overloads positional package arguments:

```bash
arx add ./dist --root ./repo
arx add ./dist ./more-packages --root ./repo
```

`publish-dir` is the higher-level wrapper for repeated drop-directory operation:

```bash
arx publish-dir ./dist --root ./repo
arx publish-dir ./dist --root ./repo \
  --apt-live ./public/deb \
  --yum-flat-live ./public/repo \
  --staging-dir ./public/.arx-cutovers
```

Implemented `arx add` behavior:

1. **Clear scope:** directory inputs are recursive by default.
2. **Package filtering:** only `.deb` and `.rpm` files are selected; unrelated
   files are ignored.
3. **Stable processing order:** discovered package files are sorted before
   copying into the pool.
4. **Fail-loud errors:** invalid package files still fail with the package path
   in the error context; directories with no supported package files fail
   instead of silently doing nothing.
5. **Symlink policy:** directory traversal does **not** follow symlinked
   directories.
6. **CLI compatibility:** existing explicit file arguments and shell-glob usage
   continue to work unchanged.
7. **Tests and docs:** CLI regression coverage verifies recursive mixed
   directory input, stable ordering, publishability, and empty-directory failure.

Implemented `publish-dir` behavior:

1. **Drop-directory scope:** direct children are scanned by default; `--recursive`
   opts into recursive discovery.
2. **No-op state:** source package fingerprints are recorded under `.arx-cache/`
   by default so unchanged runs can exit without republishing.
3. **Publish and cutover:** after ingesting packages, `publish-dir` can publish
   metadata and use the same preflighted `--apt-live` / `--yum-flat-live`
   symlink cutover as `arx publish`.
4. **RPM payload signing boundary:** `--sign-rpms` and `--rpm-sign-cmd` can sign
   unsigned RPM payloads before ingest; repository metadata signing remains the
   normal ArtifactX publish responsibility.
5. **Sync boundary:** `--sync-cmd` is opt-in and runs only after a successful
   non-no-op publish. ArtifactX does not assume downstream replication by
   default.

## Consequences

- Good: migrations and CI workflows become simpler: users can point ArtifactX at
  a directory instead of enumerating package files.
- Good: discovery and filtering behavior becomes portable and documented instead
  of shell-dependent.
- Good: `publish-dir` covers the common production package-drop loop without a
  site-specific wrapper script.
- Good: directly supports the import-first and publish/API product focus.
- Bad / cost: directory traversal adds ambiguity around recursion, symlinks,
  hidden files, ignored files, and partial failures.
- Bad / cost: overloading `arx add <PACKAGES>...` to accept directories could be
  surprising unless errors and docs are clear.
- Bad / cost: `arx add` and `publish-dir` deliberately use different recursion
  defaults, so docs and help text must stay explicit.

## Alternatives considered

- **Keep requiring shell globs.** Rejected as the only path: it works for simple
  flat directories, but it is not portable enough for recursive migration and CI
  workflows.
- **Require only `arx add <DIR>` and no wrapper.** Rejected: it still leaves
  repeated production drops, no-op detection, live cutover, RPM payload signing,
  and downstream sync to site-specific shell glue.
- **Always recurse automatically for every directory workflow.** Rejected:
  convenient for `arx add`, but too surprising for operational package drops or
  large exported trees. `publish-dir` therefore keeps recursion explicit.

## Future improvements

- Add include/exclude patterns if simple directory input proves useful.
- Consider dry-run output that lists discovered package files before mutating the
  repository.
- Revisit whether `arx watch` and directory `arx add` should share discovery and
  filtering internals.

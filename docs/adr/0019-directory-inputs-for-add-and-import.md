# ADR-0019: Directory inputs for `arx add` and import workflows

- Status: **Accepted for `arx add`; import-local-directory remains proposed**
- Date: 2026-06-22
- Target: v0.2.0 planning candidate
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

Add directory inputs to `arx add` in v0.2.0. Import-like local directory
workflows remain a proposed follow-up because upstream apt/yum repository import
still has different metadata semantics from copying already-built package files.

The accepted `arx add` shape overloads positional package arguments:

```bash
arx add ./dist --root ./repo
arx add ./dist ./more-packages --root ./repo
```

Implemented behavior:

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

## Consequences

- Good: migrations and CI workflows become simpler: users can point ArtifactX at
  a directory instead of enumerating package files.
- Good: discovery and filtering behavior becomes portable and documented instead
  of shell-dependent.
- Good: directly supports the import-first product focus.
- Bad / cost: directory traversal adds ambiguity around recursion, symlinks,
  hidden files, ignored files, and partial failures.
- Bad / cost: overloading `arx add <PACKAGES>...` to accept directories could be
  surprising unless errors and docs are clear.

## Alternatives considered

- **Keep requiring shell globs.** Rejected as the only path: it works for simple
  flat directories, but it is not portable enough for recursive migration and CI
  workflows.
- **Require a separate command for directory import.** Possible, but likely too
  much surface area if `arx add` can safely accept directory inputs.
- **Always recurse automatically.** Deferred: convenient, but may surprise users
  in large exported trees or mixed workspaces.

## Future improvements

- Add include/exclude patterns if simple directory input proves useful.
- Consider dry-run output that lists discovered package files before mutating the
  repository.
- Revisit whether `arx watch` and directory `arx add` should share discovery and
  filtering internals.

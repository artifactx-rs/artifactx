# ADR-0019: Directory inputs for `arx add` and import workflows

- Status: **Proposed**
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

Add directory inputs to the v0.2.0 planning backlog for `arx add` and import-like
repository workflows.

The intended user-facing shape is still open, but acceptable designs include:

```bash
arx add ./dist --root ./repo
arx add ./dist --recursive --root ./repo
arx import ./exported-repo --root ./repo
```

or an explicit option if overloading positional package arguments would be too
ambiguous:

```bash
arx add --from-dir ./dist --root ./repo
arx add --from-dir ./dist --recursive --root ./repo
```

Implementation requirements before acceptance:

1. **Clear scope:** define whether directory inputs are shallow by default,
   recursive by default, or require `--recursive`.
2. **Package filtering:** only supported package files (`.deb`, `.rpm`, and any
   accepted future formats) should be selected; unrelated files must be ignored
   or reported according to a documented policy.
3. **Stable processing order:** discovered package files must be sorted before
   add/import so output and logs are deterministic.
4. **Fail-loud errors:** invalid packages should produce actionable errors that
   identify the path; partial success semantics must be documented.
5. **Symlink policy:** directory traversal must not accidentally follow unsafe or
   surprising symlink loops. Symlink behavior needs an explicit decision.
6. **CLI compatibility:** existing explicit file arguments and shell-glob usage
   must continue to work unchanged.
7. **Tests and docs:** add CLI regression coverage for shallow/recursive
   directory input, mixed files, stable ordering, and invalid package handling.

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

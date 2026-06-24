# Prune old packages with GC

Use `arx gc` for repository maintenance after you have decided which old package
versions can disappear from the pool. GC is separate from package ingest: it does
not scan source drop directories, sign new packages, export public layouts, or
run downstream sync by itself.

## Safe workflow

1. Inspect candidates with `--dry-run`.
2. Decide whether rollback-pinned files may be removed.
3. Run GC.
4. Run `arx publish` so client metadata no longer references pruned files.
5. If you publish a separate public tree, run `arx export`, `arx cutover`, or your
   explicit downstream sync after publish.

```sh
arx gc --root ./repo --name-prefix myapp- --keep 3 --dry-run
arx gc --root ./repo --name-prefix myapp- --keep 3
arx publish --root ./repo
```

`--keep` is version-aware. ArtifactX compares Debian and RPM versions with their
native ordering rules, so `1.10` is not treated as older than `1.9` by string
sorting.

## Rollback-pinned files

By default, GC keeps files referenced by retained rollback states. That can leave
more than `--keep N` versions on disk, but it keeps `arx rollback` from producing
metadata that points at missing package payloads.

If GC reports retained rollback pins, read that as a safety stop:

```text
Kept 2 older file(s) pinned by retained rollback states. Rerun with --ignore-rollback-states only if those rollback states may no longer be valid.
```

Use the override only after you have intentionally decided that the older
rollback states may become invalid:

```sh
arx gc --root ./repo --name-prefix myapp- --keep 3 --ignore-rollback-states
arx publish --root ./repo
```

## Target one package family

Use a package name or prefix to avoid pruning unrelated packages:

```sh
# One exact package name.
arx gc myapp --root ./repo --keep 5 --dry-run

# A family/prefix, useful for generated service packages.
arx gc --root ./repo --name-prefix wss- --keep 3 --dry-run
```

Use `--apt` or `--yum` when you want to restrict the maintenance pass to one pool.
Omitting both scans both pools.

## When to export or sync

CLI `gc` edits the private pool only. It prints `Run arx publish` after actual
prunes because client metadata must be regenerated. Export and remote sync are a
separate, explicit step:

```sh
arx gc --root ./repo --name-prefix myapp- --keep 3
arx publish --root ./repo
arx export --root ./repo --apt-out ./public/deb --yum-flat-out ./public/repo
```

If you use `publish-dir` or a service wrapper for ingest, do not route GC through
that wrapper unless it has an explicit maintenance mode. GC should not re-scan a
package drop directory or trigger package-signing automation by accident.

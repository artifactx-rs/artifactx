# Curated packaging feed blueprint

This page is a maintainer blueprint for running a separate ArtifactX-powered
package feed for upstream projects that publish archives or manual install
instructions, but no reliable first-party `.deb` / `.rpm` channels.

The feed is a product experiment, not a promise that ArtifactX will become a
large downstream distribution. Keep it small, signed, reproducible, and easy to
roll back. Successful recipes should eventually become upstream packaging
requests instead of permanent local forks.

## Goals

A curated feed should prove that `arx pack` and ArtifactX publish flows can carry
real upstream software with boring operations:

- one signed apt/yum repository maintained outside this source tree;
- one recipe directory per upstream project or binary family;
- reproducible package builds from pinned upstream inputs;
- install smoke tests before every publish;
- atomic cutover and rollback for the generated repository;
- enough history to explain what changed when a package breaks.

## Non-goals

Do not use the feed to hide missing core features:

- Do not promise official upstream support. The feed is community-maintained
  unless the upstream project adopts the recipe.
- Do not auto-detect dependencies with host tools. Recipes must declare package
  dependencies explicitly.
- Do not inline package signing into `arx pack`. ArtifactX signs repository
  metadata; package payload signing remains a separate build-pipeline policy.
- Do not treat `.apk` or Arch `.pkg.tar.zst` output as repository-published by
  ArtifactX. `arx pack` can emit those artifacts; `arx add` / publish currently
  index apt/yum pools.
- Do not add every requested project. Add a recipe only when it has an owner,
  repeatable smoke tests, and a refresh policy.

## Repository model

Use one maintainer repository for the experiment so shared policy, signing,
smoke helpers, and cutover rules stay consistent. Split package-specific work by
recipe directory, not by giant scripts.

```text
artifactx-packages/
  README.md
  arx.toml                  # repository config for the feed
  policy/
    support-matrix.md       # distro/arch matrix and lifecycle rules
    signing.md              # repository key ownership and rotation notes
  helpers/
    fetch-release.sh        # shared download/checksum helper
    smoke-apt.sh            # install smoke-test harness
    smoke-dnf.sh
  recipes/
    prometheus/
      recipe.toml           # owner, upstream, cadence, outputs, matrix
      arx-pack.toml.in      # package manifest template
      fetch.sh              # upstream-specific fetch/stage logic
      systemd/
      config/
      smoke/
    victoriametrics/
      recipe.toml
      arx-pack.toml.in
      fetch.sh
      systemd/
      smoke/
  repo/                     # generated ArtifactX repository root; ignored or artifacted
  dist/                     # generated packages; ignored or artifacted
```

The exact names are not magic. The contract is:

- shared helpers are reusable and audited once;
- recipe-owned service units, config examples, patches, and smoke tests stay next
  to the recipe;
- generated packages and repository metadata are not hand-edited;
- CI can rebuild one recipe without touching unrelated recipes.

## Recipe contract

Each recipe should declare enough metadata that a reviewer can decide whether a
refresh is safe without reading every shell line.

Minimum fields:

| Field | Meaning |
| --- | --- |
| `owner` | GitHub handle or team responsible for refreshes and breakages. |
| `upstream` | Canonical upstream release URL or API. |
| `source_kind` | Archive, checksummed binary, source build, or another explicit input type. |
| `version_source` | How the recipe discovers the version to package. |
| `outputs` | Package names produced by the recipe; multi-binary upstreams may emit multiple packages. |
| `arches` | Supported architectures for this recipe. Start narrow and expand only with smoke coverage. |
| `distros` | Client families tested before publish, for example apt and dnf/yum families. |
| `dependencies` | Explicit package dependencies rendered into the pack manifest. |
| `config_paths` | Installed config files that must be marked as config/backup files. |
| `smoke_tests` | Commands that prove install, service syntax, and binary basics. |
| `refresh` | Manual, scheduled, or upstream-release-triggered cadence. |
| `rollback` | What operator signal means cut over should be reverted. |

Example shape:

```toml
owner = "@artifactx-rs/packagers"
upstream = "https://example.invalid/releases"
source_kind = "checksummed-archive"
version_source = "manual"
outputs = ["example-server", "example-tool"]
arches = ["amd64"]
distros = ["apt", "dnf"]
dependencies = ["ca-certificates"]
config_paths = ["/etc/example/example.yml"]
smoke_tests = ["example-server --version", "systemd-analyze verify example.service"]
refresh = "manual until the recipe has three clean refreshes"
rollback = "client install smoke fails or service fails to start"
```

Use placeholder examples in docs and templates. Real recipe commits should pin a
specific upstream version and checksum so rebuilds are auditable.

## Build and publish flow

A recipe refresh should be a small pull request in the feed repository:

1. Update the recipe version/checksum or source reference.
2. Fetch and verify upstream inputs in an isolated staging directory.
3. Render one or more `arx-pack.toml` manifests from the recipe metadata.
4. Run `arx pack` for the supported package formats.
5. Run package-level structural checks before repository publish.
6. Add `.deb` / `.rpm` artifacts to a temporary ArtifactX repo root.
7. Publish metadata, then run apt and dnf/yum install smoke tests against the
   staged repo.
8. Cut over the public repo only after smoke tests pass.
9. Keep rollback state and generated logs long enough to debug regressions.

The package build and repository publish steps should stay separate. A package
can be rebuilt many times in a PR; the public repository should move only at the
cutover step.

## Smoke-test baseline

Every recipe needs at least one install smoke test per published repository
family. A useful baseline is:

- install the package from the staged apt repo;
- install the package from the staged dnf/yum repo;
- run the main binary with `--version` or an equivalent no-network command;
- verify shipped systemd units when a service unit is included;
- check that config files survive package upgrade semantics when the recipe owns
  config paths;
- confirm removal does not leave package-manager metadata in a broken state.

Cross-architecture smoke tests are required before claiming support for that
architecture. If CI cannot execute an architecture, the recipe should mark it as
not supported rather than relying on archive inspection alone.

## Phase-one policy

Start deliberately small:

- one maintainer repo, not one repo per upstream project;
- one or two recipes with clear user demand;
- apt and dnf/yum repository publication first;
- one architecture until the smoke harness is boring;
- manual refreshes until the recipe has repeated successful updates;
- public docs that state the feed is community-maintained and experimental.

Add scheduled refreshes only after manual refreshes have stopped producing
surprises. Add architectures only after the smoke harness can prove install and
basic runtime behavior for them.

## Graduation path

A recipe is ready to propose upstream when it has:

- repeated clean refreshes;
- stable package names and filesystem layout;
- explicit dependency and config-file semantics;
- reproducible input checksums;
- install smoke history across the claimed distro/arch matrix;
- a clear support boundary for repository signing versus payload signing.

The upstream proposal should be small: link the recipe, smoke evidence, and
ArtifactX manifest, then ask whether the project wants to adopt or adapt the
packaging. Do not drag unrelated recipes into the upstream conversation.

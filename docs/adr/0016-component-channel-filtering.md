# ADR-0016: Per-component / per-channel operations (aptly parity)

- Status: **Accepted**
- Date: 2026-06-17
- Decided: `--component` (apt) + `--repo` (yum); publish restricts Release to
  selected component only.

## Context

Current operations (`publish`, `list`, `gc`, `rm`, `mirror`) operate on ALL
components/channels indiscriminately. This is a regression from aptly, which
lets you operate on a single component (e.g., `aptly publish repo main`).

In the Debian/apt world, a repository is organized into **components**:
`main`, `contrib`, `non-free`, `stable`, `testing`, etc. apt clients reference
them as `deb <url> <dist> <component1> <component2> ...`.

In the RPM/yum world, the equivalent concept is the **repo name** (the
sub-directory under `yum/`). yum/dnf clients reference `baseurl=<url>/<repo>/`.

Arx already supports multiple components (the pool is `apt/pool/<component>/`),
but there's no way to filter operations to a single component.

## Decision (proposed)

### Naming: `--component` for apt, `--repo` for yum

- apt: `--component main` — filter to a single Debian component
- yum: `--repo myrepo` — filter to a single yum repo

These already exist as CLI flags on some commands (`arx add --component`,
`arx push --component`). Add them to the remaining commands that operate on
pool data.

### Commands to extend

| Command | New flag | Behavior |
|---|---|---|
| `arx publish` | `--component` | Only publish metadata for that component |
| `arx list` | `--component` / `--repo` | Only list packages in that component/repo |
| `arx gc` | `--component` / `--repo` | Only prune within that component/repo |
| `arx rm` | `--component` / `--repo` | Only remove from that component/repo |
| `arx mirror` | `--component` | Only mirror that component from upstream |

Default behavior (no flag): all components/repos, same as today.

### Implementation

- `pool::list` already filters by `apt`/`yum` booleans. Add optional
  `component: Option<&str>` / `repo: Option<&str>` parameters that further
  filter the `scope` field of `Entry` (which already stores the component/repo name).
- `gc` and `rm` use `list` internally, so filtering flows naturally.
- `publish_apt` already iterates over all discovered components. Add a
  `component: Option<&str>` parameter to restrict the component loop.
- `publish_yum` already iterates over `yum/<repo>/<arch>/`. Add a
  `repo: Option<&str>` parameter.

### Not in this ADR

- apt "channel" (dist/release) filtering — the dist is already part of
  `publish_apt`'s signature and controlled by config.
- yum "channel" is not a standard concept in the RPM world.

## Consequences

- Good: component-level parity with aptly. Users can manage large repos
  one component at a time.
- Good: API and CLI use the same flag names (`--component` for apt,
  `--repo` for yum), consistent with existing `arx add`/`arx push`.
- Bad: adds optional parameters to several functions. Minor API churn.

## Alternatives considered

- **Do nothing (current behavior).** Rejected: breaks the aptly migration
  story. Users coming from aptly expect per-component ops.
- **Unified `--scope` flag.** Rejected: apt components and yum repos are
  semantically different. Using distinct flag names mirrors the upstream
  tooling and avoids confusion.

## Open questions for review

1. **`--component` on `publish`** — should it also restrict the `Release`
   file to only list that component, or still list all components but only
   regenerate the Packages for the selected one? Lean: restrict Release to
   only the selected component (apt requires matching Release components).
2. **API endpoint filter** — add `?component=` and `?repo=` query params
   to the corresponding REST API endpoints for parity.

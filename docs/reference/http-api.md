# HTTP API reference

`arx serve` serves the repository tree and exposes a JSON API under `/api/v1`.
The API mirrors the operational CLI commands so CI jobs and internal tooling can
push packages, publish metadata, prune old versions, promote packages, and roll
back published repository states without shelling into the server.

For the machine-readable contract, see [OpenAPI](openapi.yaml). When
`arx serve` is running, the same contract is served at `/api/openapi.yaml` and
an embedded Swagger UI is available at `/api/docs`.

`/api/v1` is currently a **beta developer API**: endpoint names and the
main request/response fields are intended to be script-friendly, but the project
may still add fields or tighten validation before declaring the API stable.
Clients should ignore unknown JSON fields and should not parse human-readable
summary strings as a stable contract.

Compatibility policy before stable API status:

- Existing `/api/v1` endpoint paths will not be renamed without a deprecation
  note in this document.
- New optional query parameters or response fields may be added in minor
  releases.
- Error responses are currently plain text with HTTP status codes; do not depend
  on exact wording.
- Large pool pagination/streaming is not implemented yet. Use search filters
  (`q`, `name_prefix`, `version`, `arch`, `scope`, `apt`, `yum`) to keep scripted
  responses bounded until pagination is designed.

## Base URL

Examples below assume the default development server:

```sh
BASE_URL=http://127.0.0.1:8080
```

Static apt/yum repository files are served from the same origin, for example
`/apt/dists/...` and `/yum/<repo>/<arch>/repodata/...`.

## Authentication

Reads are public:

- `GET /api/v1/health`
- `GET /api/v1/packages`
- `GET /api/v1/history/{target}`
- static repository files
- `GET /metrics`

Writes require bearer authentication when the server is configured for writes:

```http
Authorization: Bearer <token-or-github-oidc-jwt>
```

Supported write authentication modes:

1. **Static token**: set `ARX_SERVE_TOKEN` in the `arx serve` environment and
   send the same value as the bearer token.
2. **GitHub Actions OIDC**: enable `[oidc]` in `arx.toml`; `arx push` can mint
   and send a GitHub Actions OIDC JWT automatically from CI.

If neither `ARX_SERVE_TOKEN` nor `[oidc].enabled = true` is configured, read
endpoints still work and write endpoints return `403 Forbidden`.

For GitHub Actions examples, see [Push packages from CI](../how-to/push-from-ci.md).
For production TLS exposure, put `arx serve` behind a reverse proxy; see
[Secure `arx serve` behind a TLS proxy](../how-to/secure-serve-behind-proxy.md).

## Common status codes

| Status | Meaning |
| --- | --- |
| `200 OK` | Request completed. JSON endpoints return JSON unless noted. |
| `400 Bad Request` | Invalid path, query parameter, upload filename, or package scope. |
| `401 Unauthorized` | Write auth is configured but the bearer token/JWT is missing or invalid. |
| `403 Forbidden` | Writes are disabled because no write auth mode is configured. |
| `422 Unprocessable Entity` | A strict publish rejected a package skipped from apt metadata. |
| `500 Internal Server Error` | I/O, signing, repository, or unexpected internal error. |

Error responses are plain text with a trailing newline.

## Shared schemas

### PackageInfo

`GET /api/v1/packages`, delete, and GC responses use this shape:

```json
{
  "name": "myapp",
  "version": "1.2.3-1",
  "arch": "amd64",
  "scope": "main",
  "kind": "apt"
}
```

Fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | string | Package name parsed from the `.deb` or `.rpm`. |
| `version` | string | Debian version or RPM version. For RPM, `release` (and `epoch`, when present) are tracked in separate fields. |
| `arch` | string | Package architecture, such as `amd64` or `x86_64`. |
| `scope` | string | apt component or yum repo name. |
| `kind` | string | `apt` or `yum`. |

### Format selection query flags

Several write endpoints accept `apt` and `yum` boolean query flags:

- `?apt=true` limits the operation to apt.
- `?yum=true` limits the operation to yum.
- Omitting both usually means both formats for operations that support both.

For `DELETE /api/v1/packages/{name}` and `POST /api/v1/gc`, omitting both also
scans both pools because it follows the shared pool-maintenance behavior.

## Endpoints

### `GET /api/v1/health`

Returns server identity and version.

```sh
curl -fsSL "$BASE_URL/api/v1/health"
```

Response:

```json
{
  "name": "arx",
  "version": "0.1.4"
}
```

### `GET /api/v1/packages`

Lists package files currently present in the apt and yum pools. Query parameters
apply the same package-search model as `arx search`.

```sh
curl -fsSL "$BASE_URL/api/v1/packages"
curl -fsSL "$BASE_URL/api/v1/packages?q=demo&apt=true&scope=main"
```

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `q` | string | none | Match package names containing this substring. |
| `name_prefix` | string | none | Match package names starting with this prefix. |
| `version` | string | none | Match an exact package version. |
| `arch` | string | none | Match an exact package architecture. |
| `scope` | string | none | Match an apt component or yum repo name. |
| `apt` | boolean | false | Restrict to apt pool. |
| `yum` | boolean | false | Restrict to yum pool. |

Omitting both `apt` and `yum` scans both pools.

Response:

```json
[
  {
    "name": "myapp",
    "version": "1.2.3-1",
    "arch": "amd64",
    "scope": "main",
    "kind": "apt"
  }
]
```

### `POST /api/v1/packages`

Uploads one `.deb` or `.rpm`, stores it in the pool, then republishes metadata
for the corresponding repository format.

Required headers:

| Header | Meaning |
| --- | --- |
| `Authorization` | `Bearer <token-or-github-oidc-jwt>` |
| `X-Arx-Filename` | Package filename. Must be a single safe path component. |

Optional headers:

| Header | Applies to | Meaning |
| --- | --- | --- |
| `X-Arx-Component` | `.deb` | apt component. Defaults to `[apt].component`. |
| `X-Arx-Repo` | `.rpm` | yum repo name. Defaults to `[yum].repo`. |

Body: raw package bytes.

```sh
curl -fsSL \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  -H "X-Arx-Filename: myapp_1.2.3-1_amd64.deb" \
  -H "X-Arx-Component: main" \
  --data-binary @dist/myapp_1.2.3-1_amd64.deb \
  "$BASE_URL/api/v1/packages"
```

Response:

```json
{
  "stored": "apt/pool/main/myapp_1.2.3-1_amd64.deb",
  "published": "apt: indexed 1 package(s) across 1 dist/component(s)",
  "skipped": []
}
```

`skipped` is omitted when empty. If `[apt].strict = true` and publishing skips a
package, the request fails with `422` instead of returning `skipped`.

### `DELETE /api/v1/packages/{name}`

Removes packages matching `name` and optional exact `version`, then republishes
metadata.

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `version` | string | none | Remove only this exact version. |
| `apt` | boolean | `false` | Limit to apt pool when true. |
| `yum` | boolean | `false` | Limit to yum pool when true. |

```sh
curl -fsSL -X DELETE \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/packages/myapp?version=1.2.3-1&apt=true"
```

Response:

```json
{
  "removed": [
    {
      "name": "myapp",
      "version": "1.2.3-1",
      "arch": "amd64",
      "scope": "main",
      "kind": "apt"
    }
  ],
  "published": "apt: indexed 0 package(s) across 1 dist/component(s); yum: indexed 0 package(s) across 0 repo/arch dir(s)"
}
```

### `POST /api/v1/gc`

Prunes older package versions from the pool. It keeps rollback-referenced files
so retained published states remain valid.

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | string | none | Prune only this package name. |
| `name_prefix` | string | none | Prune only packages whose names start with this prefix. |
| `keep` | integer | `3` | Keep this many highest-precedence versions per package/scope/arch, using package version ordering semantics (Debian/RPM-style), not file mtime. |
| `keep_within_days` | integer | `0` | Also keep packages newer than this many days. |
| `grace_days` | integer | `0` | Defer pruning packages newer than this grace period. |
| `dry_run` | boolean | `false` | Report what would be pruned without deleting or publishing. |
| `ignore_rollback_states` | boolean | `false` | Allow pruning files referenced by retained rollback states. |
| `apt` | boolean | `false` | Limit to apt pool when true. |
| `yum` | boolean | `false` | Limit to yum pool when true. |

```sh
curl -fsSL -X POST \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/gc?keep=5&keep_within_days=90&dry_run=true"
```

Response:

```json
{
  "pruned": [],
  "dry_run": true,
  "retained_for_rollback": 0,
  "deferred": 0,
  "bytes_freed": 0,
  "published": null
}
```

`published` is `null` for dry runs or when nothing was pruned.
By default, rollback-referenced files are retained and counted in
`retained_for_rollback`; pass `ignore_rollback_states=true` only after deciding
those old rollback states no longer need to be valid.

### `POST /api/v1/publish`

Regenerates apt and yum repository metadata from the current pool.

```sh
curl -fsSL -X POST \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/publish"
```

Response:

```json
{
  "apt": "apt: indexed 1 package(s) across 1 dist/component(s)",
  "yum": "yum: indexed 1 package(s) across 1 repo/arch dir(s)"
}
```

### `GET /api/v1/history/{target}`

Lists retained published states for a rollback target.

Targets:

- apt: distribution name, for example `stable`
- yum: `<repo>/<arch>`, for example `myrepo/x86_64`

```sh
curl -fsSL "$BASE_URL/api/v1/history/stable"
curl -fsSL "$BASE_URL/api/v1/history/myrepo/x86_64"
```

Response:

```json
[
  {"id": "20260620T120000Z-a1b2c3d4", "current": false},
  {"id": "20260620T123000Z-e5f6a7b8", "current": true}
]
```

### `POST /api/v1/rollback/{target}`

Atomically flips a target back to a retained published state.

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `to` | string | latest previous state | State id to roll back to. |

```sh
curl -fsSL -X POST \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/rollback/stable?to=20260620T120000Z-a1b2c3d4"
```

Response:

```json
{
  "previous": "stable",
  "current": "20260620T120000Z-a1b2c3d4"
}
```

### `POST /api/v1/import`

Imports packages from an upstream apt or yum repository into the local pool.
Pass `publish=true` to regenerate and sign repository metadata in the same
request, or run `POST /api/v1/publish` afterwards. Importing does not reuse
upstream `InRelease`, `Release.gpg`, or `repomd.xml.asc` signatures because
ArtifactX publishes a new repository boundary.

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `url` | string | required | Upstream repository base URL. |
| `apt` | boolean | `false` | Import apt packages. |
| `yum` | boolean | `false` | Import yum packages. |
| `dist` | string | `[apt].dist` | apt distribution. |
| `component` | string | `[apt].component` / `[yum].repo` | apt component or yum repo. |
| `arch` | string | `amd64` | apt architecture filter. |
| `limit` | integer | none | Maximum packages to import. |
| `match_name` | string | none | apt package-name prefix filter. |
| `publish` | boolean | `false` | Publish apt/yum metadata after a successful import. |

If neither `apt` nor `yum` is true, ArtifactX attempts both formats.
For yum imports, metadata entries whose downloaded RPM fails size/checksum
validation are skipped with a warning so one damaged historical entry does not
block the rest of the migration.

```sh
curl -fsSL -X POST \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/import?url=https%3A%2F%2Fpackages.example.com&apt=true&dist=stable&component=main&limit=20"
```

Response:

```json
{
  "imported": 20,
  "published": "apt: indexed 20 package(s) across 1 dist/component(s)"
}
```

### `POST /api/v1/promote`

Moves packages between apt components or yum repos.

Query parameters:

| Name | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | string | required | Package name to move. |
| `from` | string | required | Source apt component or yum repo. |
| `to` | string | required | Destination apt component or yum repo. |
| `version` | string | none | Move only this exact version. |
| `apt` | boolean | `false` | Promote apt packages. |
| `yum` | boolean | `false` | Promote yum packages. |

If neither `apt` nor `yum` is true, ArtifactX attempts both formats.

```sh
curl -fsSL -X POST \
  -H "Authorization: Bearer $ARX_SERVE_TOKEN" \
  "$BASE_URL/api/v1/promote?name=myapp&from=staging&to=main&version=1.2.3-1&apt=true"
```

Response:

```json
{
  "moved": 1
}
```


## Scriptable workflow example

This example uses `curl` and `jq` to exercise the same operational path covered
by the API E2E regression test: upload, list/search, GC dry-run, publish,
history, rollback, and promote.

```sh
set -euo pipefail

BASE_URL=${BASE_URL:-http://127.0.0.1:8080}
TOKEN=${ARX_SERVE_TOKEN:?set ARX_SERVE_TOKEN}
PKG=${PKG:-dist/myapp_1.2.3-1_amd64.deb}
NEXT_PKG=${NEXT_PKG:-dist/myapp_1.2.4-1_amd64.deb}
STAGED_PKG=${STAGED_PKG:-dist/otherpkg_1.0-1_amd64.deb}

auth=(-H "Authorization: Bearer ${TOKEN}")

curl -fsSL "${BASE_URL}/api/v1/health" | jq .

curl -fsSL -X POST \
  "${auth[@]}" \
  -H "X-Arx-Filename: $(basename "$PKG")" \
  -H "X-Arx-Component: main" \
  --data-binary @"$PKG" \
  "${BASE_URL}/api/v1/packages" | jq .

curl -fsSL -X POST \
  "${auth[@]}" \
  -H "X-Arx-Filename: $(basename "$NEXT_PKG")" \
  -H "X-Arx-Component: main" \
  --data-binary @"$NEXT_PKG" \
  "${BASE_URL}/api/v1/packages" | jq .

curl -fsSL "${BASE_URL}/api/v1/packages?q=myapp&apt=true&scope=main" | jq .

curl -fsSL -X POST \
  "${auth[@]}" \
  "${BASE_URL}/api/v1/gc?name=myapp&keep=1&apt=true&dry_run=true" | jq .

curl -fsSL -X POST "${auth[@]}" "${BASE_URL}/api/v1/publish" | jq .
curl -fsSL "${BASE_URL}/api/v1/history/stable" | jq .
curl -fsSL -X POST "${auth[@]}" "${BASE_URL}/api/v1/rollback/stable" | jq .

curl -fsSL -X POST \
  "${auth[@]}" \
  -H "X-Arx-Filename: $(basename "$STAGED_PKG")" \
  -H "X-Arx-Component: staging" \
  --data-binary @"$STAGED_PKG" \
  "${BASE_URL}/api/v1/packages" | jq .

curl -fsSL -X POST \
  "${auth[@]}" \
  "${BASE_URL}/api/v1/promote?name=otherpkg&from=staging&to=main&apt=true" | jq .
curl -fsSL -X POST "${auth[@]}" "${BASE_URL}/api/v1/publish" | jq .
```

Use `POST /api/v1/import?publish=true` when migrating from an upstream apt/yum
repository instead of uploading local package files. Without `publish=true`, call
`POST /api/v1/publish` after import.

### `GET /metrics`

Returns Prometheus text exposition for the server process.

```sh
curl -fsSL "$BASE_URL/metrics"
```

The server records HTTP request/response counters such as
`arx_http_requests_total` and `arx_http_responses_total`.

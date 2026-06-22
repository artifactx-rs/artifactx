# Configuration reference

ArtifactX stores repository configuration in `arx.toml` at the repository root.
`arx init` writes this file.

## Example

```toml
[repo]
origin = "ArtifactX"
label = "ArtifactX"
description = "Signed package repository managed by ArtifactX"
# Optional apt Release identity overrides. Defaults to [apt].dist.
# suite = "stable"
# codename = "stable"

[signing]
enabled = true
encrypted = false
keys_dir = "keys"
private_key = "keys/private.asc"
public_key = "keys/public.asc"
user_id = "ArtifactX Repository Signing <signing@artifactx.local>"

[server]
addr = "127.0.0.1:8080"

[apt]
dist = "stable"
component = "main"
valid_days = 7
strict = false
pool_dir = "pool"

[yum]
repo = "myrepo"
base_dir = "yum"

[oidc]
enabled = false
audience = "arx"
allowed_repos = []
```

## `[repo]`

Human-facing repository identity used in generated metadata.

| Key | Meaning |
| --- | --- |
| `origin` | apt `Origin` value. |
| `label` | apt `Label` value. |
| `description` | Human-readable description. |
| `suite` | Optional apt `Suite` override. Defaults to `[apt].dist` when omitted. |
| `codename` | Optional apt `Codename` override. Defaults to `[apt].dist` when omitted. |

Change these before publishing a production repo if clients should see your
company or project identity instead of the ArtifactX default. During `arx import --apt`, ArtifactX reads upstream `Release` identity fields (`Origin`,
`Label`, `Suite`, `Codename`) when available and writes them here so a migrated
repo can keep apt-secure identity stable across cutover. Edit these values
intentionally before publish only when you want clients to accept a repository
identity change.

## `[signing]`

| Key | Meaning |
| --- | --- |
| `enabled` | Whether ArtifactX signs generated repository metadata. |
| `encrypted` | Whether the private key is passphrase-encrypted at rest. |
| `keys_dir` | Repo-relative key directory. |
| `private_key` | Repo-relative armored private key path. |
| `public_key` | Repo-relative armored public key path. |
| `user_id` | User ID used when generating a new key. |

Passphrases are supplied by `--passphrase-file` or `ARX_KEY_PASSPHRASE`.
`arx serve` blocks the configured `private_key` path (including `.old` and
`.bak` rotation backups) from static HTTP responses. The configured
`public_key` path remains readable so clients can import the repository key.

## `[server]`

| Key | Meaning |
| --- | --- |
| `addr` | Default listen address for `arx serve`. |

The default is `127.0.0.1:8080`. Keep that for localhost or reverse-proxy
setups. Use `--addr` to override at runtime.

## `[apt]`

| Key | Meaning |
| --- | --- |
| `dist` | Default apt distribution/suite. |
| `component` | Default apt component. |
| `valid_days` | Days until `Release` `Valid-Until`; `0` omits the field. |
| `strict` | Fail publish/server writes when packages are skipped. |
| `pool_dir` | Apt pool subdirectory under `apt/`. |

`arx init` writes `valid_days = 7` for new repositories so stale apt metadata
expires. Republish refreshes the window.

## `[yum]`

| Key | Meaning |
| --- | --- |
| `repo` | Default yum repo name. |
| `base_dir` | Base directory for yum repositories. |

A typical published path is `yum/<repo>/<arch>/repodata/repomd.xml`. Yum metadata is generated as gzip (`*.xml.gz`) so older CentOS 7 clients remain compatible. Use `arx export --yum-flat-out <DIR>` when an existing public URL expects a flat repo such as `/repo/*.rpm` plus `/repo/repodata`.

## `[oidc]`

GitHub Actions OIDC push authentication.

| Key | Meaning |
| --- | --- |
| `enabled` | Enable GitHub OIDC JWT validation on the server. |
| `audience` | Expected JWT audience. Default: `arx`. |
| `allowed_repos` | GitHub repository allowlist patterns, such as `myorg/*`. |

If OIDC is disabled, write API calls require `ARX_SERVE_TOKEN`. If neither OIDC
nor `ARX_SERVE_TOKEN` is configured, reads are public and writes are disabled.

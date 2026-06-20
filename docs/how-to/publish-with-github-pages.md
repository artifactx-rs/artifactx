# Publish an ArtifactX repo with GitHub Pages

This guide shows how to publish a signed apt/yum repository to GitHub Pages.
Use this path when you want a public, serverless repository: GitHub Pages serves
static files; ArtifactX creates and signs those files before deployment.

If you need remote write APIs (`arx push`, `POST /api/v1/packages`), use
`arx serve` instead. GitHub Pages is static hosting only.

## What this gives you

After the workflow runs, clients can install packages from URLs like:

- `https://OWNER.github.io/REPO/apt/dists/stable/Release`
- `https://OWNER.github.io/REPO/yum/myrepo/x86_64/repodata/repomd.xml`
- `https://OWNER.github.io/REPO/install.sh`

The deployed Pages artifact contains:

| Path | Purpose |
| --- | --- |
| `/index.html` | Landing page for humans and search engines. |
| `/install.sh` | Convenience installer for apt and dnf/yum clients. |
| `/public.asc` | Public repository signing key for clients. |
| `/apt/...` | Signed apt metadata and `.deb` packages. |
| `/yum/...` | Signed yum/dnf metadata and `.rpm` packages. |
| `/robots.txt`, `/sitemap.xml` | Crawl hints for the landing page. |

## How the ArtifactX workflows are wired

ArtifactX has two Pages paths:

| Workflow | Trigger | What it does |
| --- | --- | --- |
| `.github/workflows/release.yml` | tag push `v*` or manual dispatch | Builds `arx`, creates release artifacts, packages `arx`, then deploys the Pages repo using the just-built binary. |
| `.github/workflows/pages.yml` | manual dispatch, or changes to Pages inputs on `main` | Rebuilds and redeploys only the Pages site/repo. It downloads the latest release binary for the Cargo version; it does **not** compile Rust source. |

Both workflows call `scripts/build-pages-site.sh`. That script is the source of
truth for the generated Pages layout.

The standalone `pages` workflow checks out the repository because it needs the
workflow file, `scripts/build-pages-site.sh`, `packaging/arx.toml`, and the Cargo
version. It deliberately avoids the Rust toolchain and release build steps.

## Prerequisites

1. GitHub Pages is enabled for the repository with **Source: GitHub Actions**.
2. The workflow has these permissions:
   - `contents: read`
   - `pages: write`
   - `id-token: write`
3. The repository has a stable private signing key stored as a secret:
   - `ARX_SIGNING_KEY` — required, armored OpenPGP private key.
   - `ARX_KEY_PASSPHRASE` — required only if the private key is encrypted.
4. A release asset named `arx-latest-amd64` exists for the version used by
   `crates/arx/Cargo.toml` when using the standalone `pages` workflow.

Optional repository variable:

| Variable | Meaning |
| --- | --- |
| `PAGES_BASE_URL` | Override the generated repo URL. Use this for custom domains or non-standard Pages paths. |

If `PAGES_BASE_URL` is not set, the script derives the URL from GitHub context:

- user/organization site repo: `https://OWNER.github.io`
- project repo: `https://OWNER.github.io/REPO`

## Prepare a stable signing key

Production clients trust the repository key, not the workflow run. Do not let a
new key be generated on every Pages deployment.

If you already have an organization OpenPGP private key, store its armored
private key as `ARX_SIGNING_KEY` and store its passphrase as
`ARX_KEY_PASSPHRASE` when encrypted.

If you need to create a key with ArtifactX first:

```sh
arx init ./repo-for-key
arx key export --root ./repo-for-key > public.asc
cat ./repo-for-key/keys/private.asc
```

Add the private key content to the `ARX_SIGNING_KEY` repository secret. Keep
`public.asc` somewhere auditable so operators can compare the public key served
from Pages after deployment.

Do not commit `keys/private.asc`, passphrase files, or copied secrets.

## Deploy from a release tag

Use the release workflow when shipping a new ArtifactX version or when you want
Pages to be built from the same binary that was just released.

```sh
scripts/sync-version.py --check
git tag -a v0.1.5 -m 'v0.1.5'
git push origin v0.1.5
```

The release workflow will:

1. verify the tag version matches `crates/arx/Cargo.toml`;
2. build static Linux binaries;
3. package `arx` into `.deb` and `.rpm` artifacts;
4. create the GitHub Release;
5. build `public/` with `scripts/build-pages-site.sh`;
6. deploy `public/` to GitHub Pages.

## Redeploy Pages without rebuilding Rust

Use the `pages` workflow when you changed the landing page, installer, Pages
metadata, or signing secret and want to redeploy without running the full release
pipeline.

```sh
gh workflow run pages.yml --repo OWNER/REPO --ref main
gh run watch --repo OWNER/REPO
```

The standalone workflow will:

1. read the version from `crates/arx/Cargo.toml`;
2. download `arx-latest-amd64` from release `v<version>`;
3. run `scripts/build-pages-site.sh`;
4. upload and deploy the `public/` artifact.

It does not run `cargo build`, `cargo test`, or `cargo zigbuild`.

## Test the Pages build locally

A local dry run is useful before changing the workflow or generated landing page.
It writes a `public/` directory exactly like the workflow does.

```sh
cargo build --release -p artifactx
mkdir -p build
cp target/release/arx build/arx

export ARX_VERSION="$(python3 - <<'PY'
from pathlib import Path
import re
text = Path('crates/arx/Cargo.toml').read_text()
print(re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE).group(1))
PY
)"
export GITHUB_REPOSITORY_OWNER=artifactx-rs
export GITHUB_REPOSITORY=artifactx-rs/artifactx
export PAGES_REPOSITORY_NAME=artifactx
export ARX_SIGNING_KEY="$(cat ./repo-for-key/keys/private.asc)"
# export ARX_KEY_PASSPHRASE='...'  # only if the key is encrypted

scripts/build-pages-site.sh
```

Inspect the generated output:

```sh
test -s public/index.html
test -s public/install.sh
test -s public/public.asc
test ! -e public/keys/private.asc
test -s public/apt/dists/stable/Release
test -s public/yum/myrepo/x86_64/repodata/repomd.xml
python3 -m http.server --directory public 8089
```

Then, from another terminal:

```sh
curl -fsSL http://127.0.0.1:8089/
curl -fsSL http://127.0.0.1:8089/install.sh
curl -fsSL http://127.0.0.1:8089/public.asc
curl -fsSL http://127.0.0.1:8089/robots.txt
curl -fsSL http://127.0.0.1:8089/sitemap.xml
```

Stop the local server with `Ctrl-C` when finished.

## Verify a deployed Pages repo

After GitHub Pages reports a successful deployment, verify the public URLs:

```sh
BASE=https://OWNER.github.io/REPO
curl -fsSL "$BASE/" >/dev/null
curl -fsSL "$BASE/install.sh" | sed -n '1,80p'
curl -fsSL "$BASE/public.asc" | grep -q 'BEGIN PGP PUBLIC KEY BLOCK'
curl -fsSL "$BASE/apt/dists/stable/Release" | grep -q '^Origin:'
curl -fsSL "$BASE/yum/myrepo/x86_64/repodata/repomd.xml" | grep -q '<repomd'
```

For ArtifactX's own dogfood repo:

```sh
BASE=https://artifactx-rs.github.io/artifactx
curl -fsSL "$BASE/install.sh" | sh
arx --version
```

For production fleets, prefer managing the keyring and repo files through your
configuration management instead of piping `install.sh` directly into `sh`. See
[Install clients](install-clients.md) for manual apt and dnf/yum setup.

## Update the landing page

The generated landing page lives inside `scripts/build-pages-site.sh`. Edit that
script when changing copy, metadata, install snippets, or links. Then run:

```sh
bash -n scripts/build-pages-site.sh
scripts/build-pages-site.sh
```

Commit the script change and push to `main`. The standalone `pages` workflow is
configured to redeploy when `scripts/build-pages-site.sh`, `packaging/arx.toml`,
or `.github/workflows/pages.yml` changes on `main`.

## Security and operations notes

- Treat `ARX_SIGNING_KEY` like production infrastructure. Rotate it only with a
  client trust rollout.
- `scripts/build-pages-site.sh` fails if `ARX_SIGNING_KEY` is missing, because a
  public repo must keep a stable trust root.
- The script removes `public/keys/private.asc` before deployment and checks that
  the private key is absent.
- GitHub Pages cannot receive `arx push` uploads. Use a release workflow,
  standalone Pages workflow, or another static file upload mechanism to update
  it.
- Pages serves HTTPS static files, but apt/dnf trust still depends on signed
  repository metadata and the client-installed public key.
- Keep backups of the private key, passphrase, and release artifacts outside the
  GitHub Actions runtime.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `ARX_SIGNING_KEY is required` | Secret is missing or empty. | Add the armored private key to the repository secret `ARX_SIGNING_KEY`. |
| `gh release download ... v<version>` fails | The standalone Pages workflow cannot find a release for the Cargo version. | Publish/tag that version first, or use the release workflow. |
| Pages URL in `install.sh` is wrong | The derived owner/repo URL does not match your custom domain or hosting path. | Set repository variable `PAGES_BASE_URL` to the final base URL. |
| apt reports `NO_PUBKEY` | Client has the wrong or missing public key. | Reinstall `$BASE/public.asc` into `/etc/apt/keyrings/arx.asc`. |
| apt reports expired metadata | The repo has not been republished within `[apt].valid_days`. | Redeploy Pages or adjust `valid_days` if you intentionally omit expiry. |
| dnf cannot verify repo metadata | `repo_gpgcheck=1` cannot fetch or trust the repo key. | Check `$BASE/public.asc` and the generated `.repo` file. |

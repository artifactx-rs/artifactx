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
| `.github/workflows/release.yml` | newest tag push `v*` or manual dispatch | Builds `arx`, creates release artifacts, packages `arx`, then deploys the Pages repo using the just-built binary. Older `v*` tags still verify metadata, but skip externally visible publication when a newer tag exists. |
| `.github/workflows/pages.yml` | manual dispatch, or changes to Pages inputs on `main` | Rebuilds and redeploys only the Pages site/repo. It prefers the release binary for the Cargo version and falls back to a local `cargo build` when that release asset is not available yet. |

Both workflows call `scripts/build-pages-site.sh`. That script is the source of
truth for the generated repository layout, while `site/` is the maintainable
source for the landing page, install helper, robots file, and sitemap.

The standalone `pages` workflow checks out the repository because it needs the
workflow file, `scripts/build-pages-site.sh`, `site/`, `packaging/arx.toml`, and
the Cargo version. It avoids the full release pipeline (`cargo zigbuild`, GitHub
Release writes, GHCR, and tag publication). It uses a quick local build only as a
fallback when the matching release binary is not available.

## Prerequisites

1. GitHub Pages is enabled for the repository with **Source: GitHub Actions**.
2. The workflow has these permissions:
   - `contents: read`
   - `pages: write`
   - `id-token: write`
3. The repository has a stable private signing key stored as a secret:
   - `ARX_SIGNING_KEY` — required, armored OpenPGP private key.
   - `ARX_KEY_PASSPHRASE` — required only if the private key is encrypted.
4. Recommended: a release asset named `arx-latest-amd64` exists for the version
   used by `crates/arx/Cargo.toml` when using the standalone `pages` workflow.
   If it is missing, the workflow falls back to building `arx` from the checked
   out commit before generating Pages.

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
git tag -a vX.Y.Z -m 'vX.Y.Z'
git push origin vX.Y.Z
```

The release workflow will:

1. verify the tag version matches `crates/arx/Cargo.toml`;
2. check whether the pushed tag is the newest semantic `v*` tag;
3. build static Linux binaries only when this tag is the newest release tag;
4. package `arx` into `.deb` and `.rpm` artifacts;
5. create the GitHub Release;
6. build `public/` with `scripts/build-pages-site.sh`;
7. deploy `public/` to GitHub Pages.

This protects rapid patch iteration: if several `v0.2.x` tags are pushed close
together, only the newest tag updates `arx-latest-*`, GHCR `latest`, and the
dogfood Pages repository. Older tags fail only on real metadata mismatches; they
do not overwrite the public "latest" surfaces.

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
2. download `arx-latest-amd64` from release `v<version>`, or build `arx`
   locally if that release asset does not exist yet;
3. run `scripts/build-pages-site.sh`;
4. upload and deploy the `public/` artifact.

It does not run `cargo test`, `cargo zigbuild`, GitHub Release creation, or GHCR
publication.

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

The generated landing page source lives in `site/`, not in the generated
`public/index.html` artifact. Edit `site/index.html` for copy, layout, metadata,
and links; edit `site/install.sh.in` for the installer; edit
`site/robots.txt.in` or `site/sitemap.xml.in` for crawler metadata. Then run:

```sh
bash -n scripts/build-pages-site.sh
scripts/build-pages-site.sh
```

Commit the `site/` or script change and push to `main`. The standalone `pages`
workflow is configured to redeploy when `site/`, `scripts/build-pages-site.sh`,
`packaging/arx.toml`, or `.github/workflows/pages.yml` changes on `main`.

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
| `gh release download ... v<version>` fails | The standalone Pages workflow cannot find a release for the Cargo version. | It falls back to `cargo build --release -p artifactx`; publish/tag that version first if you require Pages to be built from a released binary. |
| Pages URL in `install.sh` is wrong | The derived owner/repo URL does not match your custom domain or hosting path. | Set repository variable `PAGES_BASE_URL` to the final base URL. |
| apt reports `NO_PUBKEY` | Client has the wrong or missing public key. | Reinstall `$BASE/public.asc` into `/etc/apt/keyrings/arx.asc`. |
| apt reports expired metadata | The repo has not been republished within `[apt].valid_days`. | Redeploy Pages or adjust `valid_days` if you intentionally omit expiry. |
| dnf cannot verify repo metadata | `repo_gpgcheck=1` cannot fetch or trust the repo key. | Check `$BASE/public.asc` and the generated `.repo` file. |

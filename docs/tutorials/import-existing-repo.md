# Import an existing apt or yum repo

Use this tutorial when you already publish packages somewhere else and want to
move clients to an ArtifactX-managed repository without a big-bang cutover.

The migration pattern is:

1. Create an empty ArtifactX repo.
2. Import a small, filtered slice from the upstream repo.
3. Publish signed metadata under your ArtifactX key.
4. Test clients against the new repo.
5. Expand the import and cut over when the repo is boring.

`arx import` is the payload-copy step. `arx publish` is the metadata step: it
regenerates `Packages`/`Release` or `repodata/repomd.xml` and signs that new
metadata for the ArtifactX repository. Existing upstream repository signatures
cannot be reused because the new repository has its own paths, checksums,
expiry, and trust boundary. Individual `.deb`/`.rpm` payloads are not rewritten
or re-signed.

For apt migrations, `arx import --apt` also reads upstream `dists/<dist>/Release`
when available and preserves `Origin`, `Label`, `Suite`, and `Codename` in
`arx.toml`. This prevents apt-secure from seeing an accidental identity change
when clients cut over to the ArtifactX-generated metadata.

## Prerequisites

- `arx` installed.
- An upstream apt or yum repository reachable over HTTP(S).
- Network access from the machine running `arx import`.

## Import from apt

Create the local repo:

```sh
arx init ./repo
```

Import a limited apt slice first:

```sh
arx import https://packages.example.com \
  --apt \
  --dist stable \
  --component main \
  --arch amd64 \
  --match-name myapp \
  --limit 20 \
  --root ./repo
```

Publish the new repository metadata:

```sh
arx publish --root ./repo
```

This creates fresh `InRelease` / `Release.gpg` for the imported pool using the
key configured in `./repo/arx.toml`. If the upstream `Release` file was readable,
the generated `Release` keeps its `Origin`, `Label`, `Suite`, and `Codename`
identity unless you deliberately edit `[repo]` in `arx.toml` before publishing.

Serve locally for client testing:

```sh
arx serve --root ./repo
```

Then configure an apt client with the repo public key and URL. See
[Install clients](../how-to/install-clients.md).

## Import from yum/dnf

Create the local repo:

```sh
arx init ./repo
```

Import a limited yum repo slice:

```sh
arx import https://packages.example.com \
  --yum \
  --component myrepo \
  --match-name myapp \
  --limit 20 \
  --root ./repo
```

Publish and serve:

```sh
arx publish --root ./repo
arx serve --root ./repo
```

This creates fresh `repomd.xml.asc` for each generated yum architecture repo.
If upstream metadata contains damaged historical entries (for example a size or
checksum that does not match the downloaded RPM), ArtifactX warns and skips the
bad entry while continuing to import the rest.

Then configure a dnf/yum client. See [Install clients](../how-to/install-clients.md).

## Expand after the canary

After one or two test clients install successfully, remove `--limit` and widen
`--match-name` or omit it entirely:

```sh
arx import https://packages.example.com --apt --dist stable --component main --arch amd64 --root ./repo
arx publish --root ./repo
```

## Cutover checklist

- The imported package set matches what clients need.
- For apt, `[repo]` identity in `arx.toml` matches the old repo unless you are
  intentionally changing `Origin` / `Label` / `Suite` / `Codename`.
- `arx publish --root ./repo` succeeds without unexpected skipped packages.
- Clients trust `keys/public.asc` from the ArtifactX repo.
- At least one apt client and one yum/dnf client, if both formats are used, can
  install from the new URL.
- If you import an existing organization signing key, verify the public key
  fingerprint on both apt and yum/dnf clients before cutover.
- Rollback is clear: clients can point back to the old repo URL until cutover is
  complete.

## What import does not do

- It does not take ownership of your upstream repo.
- It does not re-sign individual package payloads.
- It does not reuse upstream repository metadata signatures; `publish` signs the
  new ArtifactX metadata. It can preserve apt identity fields, but the signature
  is still newly generated for the ArtifactX repository.
- It does not replace your organization key-governance process.
- It does not make stale upstream packages fresh; it republishes selected
  packages under new repository metadata.

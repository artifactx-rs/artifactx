# Import an existing apt or yum repo

Use this tutorial when you already publish packages somewhere else and want to
move clients to an ArtifactX-managed repository without a big-bang cutover.

The migration pattern is:

1. Create an empty ArtifactX repo.
2. Import a small, filtered slice from the upstream repo.
3. Publish signed metadata under your ArtifactX key.
4. Test clients against the new repo.
5. Expand the import and cut over when the repo is boring.

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
- `arx publish --root ./repo` succeeds without unexpected skipped packages.
- Clients trust `keys/public.asc` from the ArtifactX repo.
- At least one apt client and one yum/dnf client, if both formats are used, can
  install from the new URL.
- Rollback is clear: clients can point back to the old repo URL until cutover is
  complete.

## What import does not do

- It does not take ownership of your upstream repo.
- It does not re-sign individual package payloads.
- It does not replace your organization key-governance process.
- It does not make stale upstream packages fresh; it republishes selected
  packages under new repository metadata.

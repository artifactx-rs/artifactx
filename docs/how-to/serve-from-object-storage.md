# Serve package blobs from object storage

`arx serve` can answer package downloads with a presigned `302` while all
repository metadata stays on the local filesystem. The apt/yum clients still
verify the same `Release`/`repomd.xml` signatures; only the package bytes come
from the bucket.

This is Phase 1 of [ADR-0031](../adr/0031-object-storage-blob-backend.md). The
local pool remains the input for `arx add`, `arx import`, `arx publish`, and
`arx gc`; the bucket is a serving replica.

## 1. Create a private bucket

Use AWS S3 or any S3-compatible store (MinIO, R2, Ceph). Keep the bucket private:
presigned URLs are the only read path. Grant the arx process `s3:PutObject` and
`s3:GetObject` on the bucket prefix.

## 2. Configure `arx.toml`

```toml
[storage]
backend = "s3"
endpoint = "https://s3.example.com"
region = "us-east-1"
bucket = "my-packages"
# prefix = "repos/prod"
path_style = true
presign_ttl_secs = 300
```

`endpoint` must not include a path. `path_style = true` works for MinIO and AWS
S3; set it to `false` for virtual-host addressing.

## 3. Provide credentials

Credentials are environment-only and are never read from `arx.toml`:

```sh
export ARX_S3_ACCESS_KEY_ID=...
export ARX_S3_SECRET_ACCESS_KEY=...
# export ARX_S3_SESSION_TOKEN=...   # temporary credentials
```

## 4. Upload the blobs

```sh
arx publish --root ./repo
arx storage push --root ./repo --dry-run   # inspect keys first
arx storage push --root ./repo
```

`push` walks `apt/pool/**` and `yum/**` (RPMs only, never `repodata/`), maps
each file to `<prefix>/<repo-relative path>`, and uploads it. Rerunning is safe.
Run it after every publish that adds packages.

## 5. Serve

```sh
arx serve --root ./repo
```

`GET`/`HEAD` for `/apt/pool/...` and `/yum/...` now returns `302` to the bucket.
`dists/`, `repodata/`, keys, and the HTTP API are still served locally. The
`/deb` and `/repo` cutover mounts are not redirected.

Verify the wire behavior:

```sh
curl -sI http://127.0.0.1:8080/apt/pool/main/a/app/app_1.0_amd64.deb | head -3
# HTTP/1.1 302 Found
# location: https://s3.example.com/my-packages/apt/pool/...
curl -fsSL -o /tmp/app.deb http://127.0.0.1:8080/apt/pool/main/a/app/app_1.0_amd64.deb
sha256sum /tmp/app.deb
```

## Limits

- A missing object returns the bucket's `404`; `arx storage push` is the supported
  way to make the local pool and the bucket agree.
- Presigned URLs are bearer credentials for their TTL. Do not log them; keep the
  TTL short and the bucket private.
- The local pool is not pruned by `storage push`; bucket lifecycle/GC is outside
  ArtifactX. Remove deleted packages from the bucket with your storage tooling.
- The filesystem backend stays the default. Without `[storage]`, `arx serve`
  streams package bytes exactly as before.

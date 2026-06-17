# ADR-0015: Object-storage backend — deferred with architecture notes

- Status: **Deferred**
- Date: 2026-06-17

## Context

The repository is currently file-system-based: pool files live under `apt/pool/`
and `yum/<repo>/<arch>/`, and `arx serve` hosts them via `tower-http::ServeDir`.
This is simple, fast, and zero-config — but it doesn't scale to multi-node
serving or cloud-native deployments where the backing store is S3/GCS/MinIO.

Cloudsmith / JFrog / Nexus all support object-storage backends for the pool.
Adding one to arx would let users host the repository metadata on a small
compute node while the heavy package blobs live in cheap object storage.

## Why defer

The file-system backend is the **correct default** (one binary, stateless,
zero-config — charter principle 8). An object-storage backend requires:

1. An abstraction layer over the pool (trait `PoolStore` with `get`/`put`/
   `list`/`delete`)
2. AWS S3 SDK integration (or a generic S3-compatible client — `rust-s3` or
   `aws-sdk-s3`)
3. URL signing for redirect-based serving (`X-Accel-Redirect` / presigned URLs)
4. Consistent caching of metadata (the `Packages`/`Release` indices are small
   and should stay local; only the `.deb`/`.rpm` blobs are remote)

This is a **separate ADR** — the file-system backend is not going away, and
the object-storage path is additive. The design should follow the same pattern
as `Backend::{Native, Docker}` in `pack`: a clean trait, a file-system impl
that is the default and stays the default, and an S3 impl gated behind a
cargo feature.

## Architecture sketch (for the future ADR)

```rust
trait PoolStore {
    fn put(&self, rel: &str, data: &[u8]) -> Result<()>;
    fn get(&self, rel: &str) -> Result<Vec<u8>>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn delete(&self, rel: &str) -> Result<()>;
}

struct FsPool { root: PathBuf }        // today's implementation, made explicit
struct S3Pool { bucket: String, ... }  // future
```

`serve` gains a redirect mode: when the pool backend supports URL signing,
`/apt/pool/...` requests return `302` to a presigned S3 URL instead of
streaming the bytes through arx.

## When to revive

- A real multi-node deployment need (the file-system backend works for single-
  node and NFS-backed setups today).
- A user asks for it with a concrete use case (the "build it when needed"
  charter rule).

## Decision

**Deferred.** The architecture is sketched above so the next person doesn't
start from zero. No code is written under this ADR.

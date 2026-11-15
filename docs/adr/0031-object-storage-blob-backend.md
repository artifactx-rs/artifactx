# ADR-0031: Object-storage blob backend for served repositories

- Status: Accepted (independent review: two blockers fixed; residual risks noted)
- Date: 2026-11-14
- Supersedes: [ADR-0015](0015-object-storage-backend-deferred.md) (Deferred)
- Target: v0.6 distributed delivery ([#94](https://github.com/artifactx-rs/artifactx/issues/94))

## Review notes

Independent review (2026-11-14) found two blockers, both fixed before
acceptance: SigV4 path canonicalization now percent-encodes reserved
characters, and `[storage]` rejects endpoint userinfo. Accepted residual risks:
the redirect predicate is unit-tested but has no automated Axum middleware
regression test (the route was exercised manually), and streaming uploads were
verified against s3rver (transport) and rclone (SigV4 positive/negative), not
against real AWS.

## Context

ADR-0015 deferred an object-storage backend because the filesystem is the correct
default and no concrete deployment needed S3 yet. The repository layout is still
local-first: `arx add`/`import` write pool files, `arx publish` scans them, and
`arx serve` streams them with `ServeDir`. Deferring kept the product small.

The demand now exists: operators want repository metadata on a small always-on
node while package blobs are served from cheap, durable object storage. The two
facts that shape the design are unchanged:

- **The filesystem must stay the default and the source of truth.** `publish`,
  `gc`, `rm`, `promote`, and `rollback` all derive metadata from local pool bytes;
  a remote-authoritative pool would touch every one of them, and that is not a
  single change.
- **Signed URLs are the only way to avoid proxying blobs through `arx serve`.**
  An S3-compatible backend with presigned GETs lets the server answer with `302`
  while the client downloads directly from the bucket.

The issue also mentions a managed/hosted offering. That is a business and
operations product, not a CLI feature; this ADR does not cover it.

## Decision

### Phase 1 (this ADR): upload + presigned read path

1. **An optional, additive config block.** `[storage]` selects
   `backend = "s3"`; the default (`"filesystem"`) leaves every existing command
   unchanged. S3 settings are `endpoint`, `region`, `bucket`, `prefix`,
   `path_style`, and `presign_ttl_secs`. Credentials come only from
   `ARX_S3_ACCESS_KEY_ID`, `ARX_S3_SECRET_ACCESS_KEY`, and the optional
   `ARX_S3_SESSION_TOKEN`; they are never read from `arx.toml` and never logged.
2. **`arx storage push` uploads existing blobs.** It walks `apt/pool/**` and
   `yum/**` (package files only, never `repodata/`), maps each file to the
   repository-relative key (`apt/pool/...`, `yum/<repo>/<arch>/...`), prepends
   the configured prefix, and `PUT`s it with SigV4. `--dry-run` lists keys
   without network access. Uploads are idempotent; a partial run is safe to
   repeat.
3. **`arx serve` redirects blob reads.** For `GET`/`HEAD` under `/apt/pool/` and
   `/yum/`, with `backend = "s3"`, the server answers `302` with a presigned URL
   (TTL from config, default 300 s). Metadata, keys, `dists/`, and `repodata/`
   stay local and are served by `ServeDir` exactly as before. The redirect
   grants only `GET`/`HEAD`; there is no listing, upload, or delete API.
4. **No S3 SDK and no new service.** SigV4 query/header signing is implemented
   in `arx` on top of the already-locked `hmac`/`sha2` crates; the existing
   `reqwest` client performs uploads. The repository stays one static binary and
   the filesystem stays authoritative.

### Phase 2 (separate ADR, not this change)

A remote-authoritative pool — `add`/`import` writing only to the bucket,
`publish` reading pool metadata from a local index instead of scanning blobs,
and `gc` deleting remote objects. That phase needs its own ADR because it
changes the meaning of "the repository is a directory you can inspect, back up,
and move", and because it must preserve the existing rollback and retention
semantics.

## Consequences

- Phase 1 duplicates bytes: the local pool remains the publish input and the
  bucket is a serving replica. Operators must run `arx storage push` after
  publishing new packages (or in CI). Phase 2 removes that duplication.
- A presigned URL is a bearer credential for its TTL. The bucket must be
  private, the TTL should be short, and URLs must not be logged. The redirect
  path changes the bytes on the wire (S3 serves them), not the bytes clients
  verify: repository metadata still carries the same hashes and signatures.
- If an object is missing, the client receives the bucket's `404`, not an
  `arx` error page. `arx storage push` is the supported way to make the two
  copies agree.
- No existing public API, CLI flag, repository layout, or state format
  changes; `arx storage push`, `--root`, and `--dry-run` are additive. A repo
  without `[storage]` behaves exactly as before.

## Acceptance

- Unit vectors for SigV4 presigning (AWS published example) and key mapping.
- SigV4-validating S3-compatible end-to-end (MinIO or rclone): a valid
  credential upload succeeds; a wrong secret returns `403
  SignatureDoesNotMatch`; `arx serve` returns `302` and the presigned GET/HEAD
  is accepted; following it yields byte-identical package files; tampering the
  signature returns `403`. Without `[storage]`, the same request is streamed
  locally.
- A path outside the blob roots, a `repodata/` path, or a `..` segment is never
  redirected.
- `cargo fmt`, `cargo clippy --workspace --all-targets --all-features -D warnings`,
  and `cargo test --workspace --all-features` pass.

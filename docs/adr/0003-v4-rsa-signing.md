# ADR-0003: v4 RSA PGP signing

- Status: Accepted
- Date: 2026-06-17

## Context

apt and dnf verify repository signatures with the gpg they ship — often old. The
signature and key versions must be ones that stock gpg accepts everywhere. We sign
with [rpgp](https://crates.io/crates/pgp) (pure Rust, no `gpg` binary needed).

A subtle trap: rpgp's `CleartextSignedMessage::sign` picks the signature version
from the **key version** — a v6 key (RFC 9580) produces a v6 signature that
traditional gpg *cannot verify*.

## Decision

Generate **v4 RSA-2048** signing keys. v4 + RSA verifies under old and new
gpg/apt/dnf. The private key may be passphrase-encrypted at rest (OpenPGP S2K),
enabled by `ARX_KEY_PASSPHRASE`/`--passphrase-file`; unset = unencrypted (keeps the
5-minute path frictionless, with a warning).

## Consequences

- Good: signatures verify on RHEL 7-era gpg through current Debian. Verified
  end-to-end against real `apt-get` (signed-by) and `dnf` (`repo_gpgcheck`).
- Good: no `gpg` binary dependency — pure Rust, single static binary.
- Bad: RSA-2048 is the floor, not future-proof forever; v4 is "old" by spec.
- Bad: RSA-2048 and OpenPGP v4 are compatibility choices, not crypto-fashion choices; configurable key profiles remain intentionally deferred.

## Alternatives considered

- **Ed25519 / v6 keys.** Smaller/faster, but not verifiable by older gpg → breaks
  the "works everywhere" promise. Rejected as a default.
- **RSA-4096 default.** ~85s `init` on this machine vs ~10s for 2048 — hurts the
  5-minute rule. 2048 is the de-facto repo-signing standard.

## Implementation status update — 2026-06-18

The accepted signing profile is unchanged: generated keys are still v4 RSA-2048,
and publish unlocks encrypted keys with `--passphrase-file` or
`ARX_KEY_PASSPHRASE`.

The key lifecycle is now implemented at the CLI layer:

- `arx key generate` creates a fresh repo signing key.
- `arx key import <private.asc>` imports an existing armored private key.
- `arx key export` prints the armored public key clients should trust.
- `arx key rotate` backs up the current key and generates a replacement.
- `arx key revoke` removes the backup created by rotate.

This does **not** reopen the rejected defaults. RSA-4096, Ed25519, and custom
signature policies remain deferred until there is a concrete compatibility matrix
and UX reason to expose them.

## Future improvements

Configurable key type/size; optional Ed25519 for users who only target modern
distros; consider constant-time token compare adjacent to this for the serve API.

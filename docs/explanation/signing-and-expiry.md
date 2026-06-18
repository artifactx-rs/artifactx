# Signing and expiry

ArtifactX keeps repository trust simple: it signs repository metadata and leaves
package payload signing to your package build pipeline.

## Repository metadata signing

When signing is enabled, ArtifactX writes signatures for generated metadata:

| Client family | Signed metadata |
| --- | --- |
| apt | `InRelease`, `Release.gpg` |
| yum/dnf | `repomd.xml.asc` |

Clients trust the repository by trusting `keys/public.asc` or an exported public
key from `arx key export`.

## Package payload signatures

ArtifactX does not re-sign individual `.deb` or `.rpm` files.

That matters for yum/dnf: a repo can have signed metadata while individual RPM
payload signature checks are handled separately. If your organization requires
RPM package signatures, sign packages before they enter ArtifactX and configure
clients accordingly.

## Generated keys

`arx init` generates OpenPGP v4 RSA-2048 signing keys unless `--no-key` is used.
Generated keys are written under `keys/` by default.

ArtifactX currently does not expose generation parameters for bit size,
algorithm, or key expiry. Organizations with existing security policy should
import their managed armored OpenPGP private key instead of relying on generated
production keys.

## Passphrase handling

Use a passphrase-encrypted key for production:

```sh
arx init ./repo --passphrase-file passphrase.txt
arx publish --root ./repo --passphrase-file passphrase.txt
```

or:

```sh
ARX_KEY_PASSPHRASE='replace-with-a-real-secret' arx publish --root ./repo
```

Keep private keys and passphrases out of git.

## Metadata expiry

New repositories created by `arx init` set:

```toml
[apt]
valid_days = 7
```

Each `arx publish` refreshes apt `Valid-Until`. This helps apt clients reject
stale repository metadata.

Set `valid_days = 0` only when you intentionally want to omit `Valid-Until`:

```toml
[apt]
valid_days = 0
```

## Operational responsibility

ArtifactX gives you a safe default for repository metadata freshness. It does not
replace your organization key-governance process.

You still own:

- private key backups
- key expiry policy
- key rotation rollout
- HSM/KMS requirements
- package payload signing policy
- client trust deployment

# Use custom signing keys

ArtifactX can generate a repository signing key or import an existing armored
OpenPGP private key. Production repositories should normally use an
organization-owned key so client trust can survive rebuilds and redeploys.

## What ArtifactX signs

ArtifactX signs repository metadata:

- apt: `InRelease` and `Release.gpg`
- yum/dnf: `repomd.xml.asc`

It does not re-sign individual package payloads.

## Local/dev: generated key

```sh
arx init ./repo
```

This writes:

- `./repo/keys/private.asc`
- `./repo/keys/public.asc`

If no passphrase is provided, the private key is stored unencrypted and ArtifactX
warns. That is acceptable for throwaway local repos, not public or production
repos.

## Production: generated encrypted key

```sh
printf '%s\n' 'replace-with-a-real-secret' > passphrase.txt
arx init ./repo --passphrase-file passphrase.txt
arx publish --root ./repo --passphrase-file passphrase.txt
```

You can also use the environment variable:

```sh
ARX_KEY_PASSPHRASE='replace-with-a-real-secret' arx publish --root ./repo
```

Do not commit `passphrase.txt` or `keys/private.asc`.

## Import a company key

Start without generating a key:

```sh
arx init ./repo --no-key
```

Import the armored private key:

```sh
arx key import company-private.asc --root ./repo --passphrase-file passphrase.txt
```

Export the matching public key for clients:

```sh
arx key export --root ./repo > public.asc
```

Publish with the passphrase if the key is encrypted:

```sh
arx publish --root ./repo --passphrase-file passphrase.txt
```

## Rotate a key

```sh
arx key rotate --root ./repo --passphrase-file passphrase.txt
arx key export --root ./repo > public.asc
```

Clients must trust the new public key before the next cutover. Plan key rotation
as a client rollout, not only a server operation.

## Current crypto knobs

ArtifactX currently generates OpenPGP v4 RSA-2048 keys. It does not expose CLI
knobs for bit size, algorithm, key expiry, or HSM/KMS-backed signing yet.

That is deliberate for now: stock apt/dnf compatibility is more important than a
large crypto configuration surface in the happy path. Organizations with strict
key governance should import their managed OpenPGP key and run their normal
approval, expiry, backup, and rotation process outside ArtifactX.

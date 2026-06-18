# Install clients from an ArtifactX repo

This guide shows how to configure apt and dnf/yum clients for an ArtifactX
repository.

If the repository provides `install.sh`, the easiest path is the one-command
installer:

```sh
curl -fsSL https://repo.example.com/install.sh | sh
```

For production fleets, prefer the manual steps below so your configuration
management owns the keyring and repo files.

## apt clients

Install the public key:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://repo.example.com/keys/public.asc \
  | sudo tee /etc/apt/keyrings/arx.asc >/dev/null
```

Add the repo:

```sh
echo 'deb [arch=amd64 signed-by=/etc/apt/keyrings/arx.asc] https://repo.example.com/apt stable main' \
  | sudo tee /etc/apt/sources.list.d/arx.list >/dev/null
```

Install a package:

```sh
sudo apt-get update
sudo apt-get install myapp
```

## dnf/yum clients

Create a repo file:

```sh
sudo tee /etc/yum.repos.d/arx.repo >/dev/null <<'EOF_REPO'
[arx]
name=ArtifactX
baseurl=https://repo.example.com/yum/myrepo/x86_64
enabled=1
gpgcheck=0
repo_gpgcheck=1
gpgkey=https://repo.example.com/keys/public.asc
EOF_REPO
```

Install a package:

```sh
sudo dnf install myapp
```

`repo_gpgcheck=1` verifies signed repository metadata. ArtifactX does not
re-sign individual RPM payloads, so `gpgcheck=0` is expected unless your package
build pipeline signs RPMs separately.

## GitHub Pages dogfood repo

ArtifactX publishes its own repo on GitHub Pages:

```sh
curl -fsSL https://artifactx-rs.github.io/artifactx/install.sh | sh
arx --version
```

That public repo is a distribution channel for `arx` itself. For your own public
repo, import or generate a stable organization key before users cut over.

## Troubleshooting

- `NO_PUBKEY` or signature failures: reinstall the public key and verify the repo
  URL points to the matching repository.
- apt `Release file expired`: republish the repo or adjust `[apt].valid_days` in
  `arx.toml` if you intentionally do not want `Valid-Until`.
- dnf asks to import a key: confirm the fingerprint is the repo key you expect.
- Wrong architecture: change the apt `arch=` value or yum architecture path.

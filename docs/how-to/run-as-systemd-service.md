# Run as a systemd service

Use systemd when ArtifactX should run as a long-lived local service behind a
reverse proxy or on an internal host.

`arx serve` defaults to `127.0.0.1:8080`. Keep that default unless you are
intentionally exposing the service on another interface.

## Quick path: let `arx daemonize` write the unit

On Linux hosts with systemd, use the one-shot setup command:

```sh
sudo arx daemonize --root /var/lib/arx/repo --enable --start
```

This creates `/etc/arx/arx.env` with a random `ARX_SERVE_TOKEN`, writes
`/etc/systemd/system/arx.service`, verifies the unit, reloads systemd, enables
the service, and starts it.

Use `--dry-run` first to inspect the exact unit and token file without writing:

```sh
arx daemonize --dry-run
```

Use `--reuse-token` when re-running the command and you want to keep the
existing bearer token.

If your public clients already use exported legacy paths, include them in the
generated service. The API still writes to `--root`; the live paths are served
read-only under `/deb/*` and `/repo/*`:

```sh
sudo arx daemonize \
  --root /data/arx/prod \
  --apt-live /srv/deb \
  --yum-flat-live /srv/repo \
  --enable --start
```

## Manual path

### 1. Prepare directories

```sh
sudo install -d -o arx -g arx /var/lib/arx/repo
sudo install -d -o arx -g arx /etc/arx
```

Create or copy the repository into `/var/lib/arx/repo`:

```sh
sudo -u arx arx init /var/lib/arx/repo
```

For production, use your organization signing key or passphrase-encrypted key.
See [Use custom signing keys](use-custom-signing-keys.md).

### 2. Optional write token

If the HTTP API should accept writes, create an environment file:

```sh
sudo tee /etc/arx/arx.env >/dev/null <<'EOF_ENV'
ARX_SERVE_TOKEN=replace-with-a-long-random-token
# ARX_KEY_PASSPHRASE=only-if-the-repo-key-is-encrypted
EOF_ENV
sudo chmod 0600 /etc/arx/arx.env
```

If `ARX_SERVE_TOKEN` is unset, reads still work but write API operations are
disabled unless OIDC is configured.

### 3. Create the unit

```ini
[Unit]
Description=ArtifactX package repository
Documentation=https://github.com/artifactx-rs/artifactx
After=network-online.target
Wants=network-online.target

[Service]
User=arx
Group=arx
EnvironmentFile=-/etc/arx/arx.env
ExecStart=/usr/local/bin/arx serve --root /var/lib/arx/repo --addr 127.0.0.1:8080
Restart=on-failure
RestartSec=5s
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/arx/repo

[Install]
WantedBy=multi-user.target
```

Write it to `/etc/systemd/system/arx.service`.

### 4. Enable and start

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now arx.service
sudo systemctl status arx.service
```

Health check:

```sh
curl -fsS http://127.0.0.1:8080/api/v1/health
```

### 5. Expose publicly through a reverse proxy

Put Caddy, nginx, or another TLS reverse proxy in front of the localhost service.
Public examples should terminate TLS at the proxy and forward to
`http://127.0.0.1:8080`.

Only bind `arx serve` to `0.0.0.0:8080` when the host firewall, network, and auth
model are intentionally designed for that exposure.

For concrete Caddy/nginx snippets, see
[Secure `arx serve` behind a TLS proxy](secure-serve-behind-proxy.md).

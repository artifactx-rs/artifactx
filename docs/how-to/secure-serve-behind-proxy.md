# Secure `arx serve` behind a TLS proxy

`arx serve` is an application server for repository files and the write API. It
does not terminate TLS itself. For production exposure, bind ArtifactX to
localhost, put Caddy/nginx/another TLS proxy in front of it, and configure write
authentication with `ARX_SERVE_TOKEN` or GitHub Actions OIDC.

## Baseline checklist

- Keep `arx serve` bound to `127.0.0.1:8080` unless you have a deliberate network
  exposure plan.
- Terminate TLS at a reverse proxy.
- Configure a write-auth mode before allowing CI or users to push packages.
- Back up `arx.toml`, `keys/`, `apt/pool/`, and `yum/<repo>/<arch>/*.rpm`.
- Do not copy `keys/private.asc`, passphrase files, `.arx-cache/`, or rollback
  state into a public static root.

ArtifactX blocks the configured private signing key path from static HTTP
responses, including `.old` and `.bak` rotation backups, but the safer production
shape is still to keep private repo state out of public document roots.

## Caddy example

```caddyfile
repo.example.com {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8080
}
```

Run ArtifactX with a write-auth mode:

```sh
ARX_SERVE_TOKEN='replace-with-a-long-random-token' \
  arx serve --root /data/arx/repo --addr 127.0.0.1:8080
```

## nginx example

```nginx
server {
    listen 443 ssl http2;
    server_name repo.example.com;

    ssl_certificate     /etc/letsencrypt/live/repo.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/repo.example.com/privkey.pem;

    client_max_body_size 2g;

    location / {
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_pass http://127.0.0.1:8080;
    }
}
```

## Public reads, authenticated writes

Package managers fetch repository metadata and payloads without auth. Writes use
the HTTP API and require a bearer credential:

```sh
arx push dist/myapp_1.2.3-1_amd64.deb \
  --url https://repo.example.com \
  --token "$ARX_SERVE_TOKEN"
```

For GitHub Actions OIDC, see [Push packages from CI](push-from-ci.md).

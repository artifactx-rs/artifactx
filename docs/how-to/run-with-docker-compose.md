# Run with Docker Compose

Use Docker Compose when you want a small, repeatable server process around an
existing ArtifactX repository directory.

## Generate compose files

From a repo root or any working directory:

```sh
arx compose --root ./repo --out ./deploy
```

ArtifactX writes:

- `./deploy/docker-compose.yml`
- `./deploy/Dockerfile`

The generated compose file mounts the actual repository root into the container
as `/repo:ro`.

## Start the server

```sh
cd ./deploy
docker compose up -d
```

By default, generated compose uses a container listener of `0.0.0.0:8080` and
publishes host port `8080`.

## Use a different host port

```sh
arx compose --root ./repo --out ./deploy --addr 0.0.0.0:18080
cd ./deploy
docker compose up -d
```

`--addr` controls the host-side published port in generated compose. The
container still runs `arx serve --addr 0.0.0.0:8080 --root /repo` because Docker
port publishing requires the process inside the container to listen beyond
localhost.

## Add write API authentication

The generated compose file is read-only by default because the repo volume is
mounted `:ro`. If you want API writes, deliberately change the volume to read-write
and set `ARX_SERVE_TOKEN`:

```yaml
services:
  arx:
    environment:
      ARX_SERVE_TOKEN: "replace-with-a-secret"
    volumes:
      - "./repo:/repo"
```

Do this only when the container is the writer for that repo. Avoid multiple
writers against the same repository root.

## Validate the generated config

```sh
docker compose -f ./deploy/docker-compose.yml config
```

Then check the API health endpoint:

```sh
curl -fsS http://127.0.0.1:8080/api/v1/health
```

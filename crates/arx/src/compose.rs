//! Generate `docker-compose.yml` + `Dockerfile` so `docker compose up` serves
//! the repository with a single command.

use std::path::Path;

use anyhow::{Context, Result};

const DOCKERFILE: &str = r#"# Build a static arx binary, then run it from a minimal image.
FROM rust:1-bookworm AS build
WORKDIR /src
RUN rustup target add x86_64-unknown-linux-musl \
    && apt-get update && apt-get install -y musl-tools clang && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl
RUN cp target/x86_64-unknown-linux-musl/release/arx /arx

FROM alpine:3.20
COPY --from=build /arx /usr/local/bin/arx
EXPOSE 8080
ENTRYPOINT ["arx"]
CMD ["serve", "--addr", "0.0.0.0:8080", "--root", "/repo"]
"#;

fn compose_yml(addr: &str) -> String {
    let port = addr.rsplit(':').next().unwrap_or("8080");
    format!(
        r#"services:
  arx:
    build:
      context: .
      dockerfile: Dockerfile
    image: artifactx:latest
    command: ["serve", "--addr", "0.0.0.0:8080", "--root", "/repo"]
    ports:
      - "{port}:8080"
    volumes:
      - "./:/repo:ro"
    restart: unless-stopped
"#
    )
}

/// Write `Dockerfile` and `docker-compose.yml` into `out_dir`.
pub fn generate(_root: &Path, out_dir: &Path, addr: &str) -> Result<()> {
    std::fs::create_dir_all(out_dir).context("creating output dir")?;
    let yml = compose_yml(addr);
    std::fs::write(out_dir.join("Dockerfile"), DOCKERFILE).context("writing Dockerfile")?;
    std::fs::write(out_dir.join("docker-compose.yml"), yml).context("writing docker-compose.yml")?;
    Ok(())
}

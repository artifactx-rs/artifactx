//! Generate `docker-compose.yml` + `Dockerfile` so `docker compose up` serves
//! the repository with a single command.

use std::path::Path;

use anyhow::{Context, Result};

const DOCKERFILE: &str = r#"FROM ghcr.io/artifactx-rs/arx:latest
EXPOSE 8080
ENTRYPOINT ["arx"]
CMD ["serve", "--addr", "0.0.0.0:8080", "--root", "/repo"]
"#;

fn compose_yml(addr: &str) -> String {
    let port = addr.rsplit(':').next().unwrap_or("8080");
    format!(
        r#"services:
  arx:
    image: ghcr.io/artifactx-rs/arx:latest
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

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn generates_dockerfile_and_compose() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("output");
        let _repo = tmp.path().join("repo");
        std::fs::create_dir_all(&_repo).unwrap();

        generate(&_repo, &out, "0.0.0.0:8080").unwrap();

        let dockerfile = out.join("Dockerfile");
        let compose = out.join("docker-compose.yml");
        assert!(dockerfile.exists(), "Dockerfile should exist");
        assert!(compose.exists(), "docker-compose.yml should exist");

        let df = std::fs::read_to_string(&dockerfile).unwrap();
        assert!(df.contains("FROM"), "Dockerfile should have FROM");
        assert!(df.contains("arx"), "Dockerfile should reference arx");

        let cy = std::fs::read_to_string(&compose).unwrap();
        assert!(cy.contains("8080"), "compose should reference port");
    }
}

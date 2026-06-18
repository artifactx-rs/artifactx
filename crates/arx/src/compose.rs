//! Generate `docker-compose.yml` + `Dockerfile` so `docker compose up` serves
//! the repository with a single command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const DOCKERFILE: &str = r#"FROM ghcr.io/artifactx-rs/arx:latest
EXPOSE 8080
ENTRYPOINT ["arx"]
CMD ["serve", "--addr", "0.0.0.0:8080", "--root", "/repo"]
"#;

fn compose_yml(addr: &str, repo_volume_source: &Path) -> String {
    let port = addr.rsplit(':').next().unwrap_or("8080");
    let repo_volume_source = repo_volume_source.display();
    format!(
        r#"services:
  arx:
    image: ghcr.io/artifactx-rs/arx:latest
    command: ["serve", "--addr", "0.0.0.0:8080", "--root", "/repo"]
    ports:
      - "{port}:8080"
    volumes:
      - "{repo_volume_source}:/repo:ro"
    restart: unless-stopped
"#
    )
}

fn absolute_repo_path(root: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = root.canonicalize() {
        return Ok(canonical);
    }

    if root.is_absolute() {
        Ok(root.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("resolving current directory for compose root")?
            .join(root))
    }
}

/// Write `Dockerfile` and `docker-compose.yml` into `out_dir`.
pub fn generate(root: &Path, out_dir: &Path, addr: &str) -> Result<()> {
    std::fs::create_dir_all(out_dir).context("creating output dir")?;
    let repo_volume_source = absolute_repo_path(root)?;
    let yml = compose_yml(addr, &repo_volume_source);
    std::fs::write(out_dir.join("Dockerfile"), DOCKERFILE).context("writing Dockerfile")?;
    std::fs::write(out_dir.join("docker-compose.yml"), yml)
        .context("writing docker-compose.yml")?;
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
        assert!(
            df.contains("ghcr.io"),
            "Dockerfile should reference GHCR image"
        );
        assert!(df.contains("arx"), "Dockerfile should reference arx");

        let cy = std::fs::read_to_string(&compose).unwrap();
        let repo = _repo.canonicalize().unwrap();
        assert!(cy.contains("8080"), "compose should reference port");
        assert!(
            cy.contains(&format!("{}:/repo:ro", repo.display())),
            "compose should mount the requested repo root"
        );
        assert!(
            !cy.contains("./:/repo:ro"),
            "compose must not accidentally mount the deploy directory"
        );
    }
}

# ArtifactX Pages source

This directory is the maintainable source for the ArtifactX GitHub Pages landing
page and install helper. `scripts/build-pages-site.sh` renders these templates
into `public/` next to the generated apt/yum repository metadata.

Keep long-lived copy, layout, and SEO metadata here instead of embedding a large
HTML heredoc in the shell script. The shell script should only orchestrate the
package-repository build and substitute deployment-specific values such as the
Pages base URL and GitHub repository name. The Cargo-derived `arx` version is
still used to build the dogfood package artifact, but the landing page copy
should not display a fixed release version.

# ArtifactX Pages source

This directory is the maintainable source for the ArtifactX GitHub Pages landing
page and install helper. `scripts/build-pages-site.sh` renders these templates
into `public/` next to the generated apt/yum repository metadata.

## Files

| Path | Rendered to | Purpose |
| --- | --- | --- |
| `index.html` | `public/index.html` | Landing page. Uses `__REPO_URL__` and `__REPOSITORY__` placeholders. |
| `install.sh.in` | `public/install.sh` | Convenience installer for apt and dnf/yum clients. |
| `robots.txt.in` | `public/robots.txt` | Crawl hints plus the sitemap location. |
| `sitemap.xml.in` | `public/sitemap.xml` | Single-page sitemap. |
| `assets/` | `public/assets/` | Hero image and Open Graph card, copied verbatim. |

## Conventions

Keep long-lived copy, layout, and SEO metadata here instead of embedding a large
HTML heredoc in the shell script. The shell script should only orchestrate the
package-repository build and substitute deployment-specific values such as the
Pages base URL and GitHub repository name.

- The landing page must stay version-neutral. The dogfood package is rebuilt from
  the current Cargo version, but the page should say *current* / *latest* rather
  than pinning a release number. The build script enforces this by rejecting
  `__ARX_VERSION__` in `index.html`.
- Reference in-page assets with root-relative or same-directory paths
  (`assets/hero.webp`) so a custom domain or `PAGES_BASE_URL` override still works.
  Only social-preview URLs need the absolute `__REPO_URL__` form.
- Keep images small: the hero is a 1600px-wide WebP and the Open Graph card is a
  1200×630 JPEG. Regenerate `assets/og.jpg` whenever the headline changes.

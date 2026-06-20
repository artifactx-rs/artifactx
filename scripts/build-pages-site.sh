#!/usr/bin/env bash
set -euo pipefail

# Build the GitHub Pages dogfood repository and landing page.
test -x ./build/arx


python3 - <<'PY'
from pathlib import Path
import os

path = Path('packaging/arx.toml')
version = os.environ['ARX_VERSION']
lines = path.read_text().splitlines()
for i, line in enumerate(lines):
    if line.startswith('version = '):
        lines[i] = f'version = "{version}"'
        break
else:
    raise SystemExit('version field not found in packaging/arx.toml')
path.write_text('\n'.join(lines) + '\n')
PY
./build/arx pack packaging/arx.toml --out dist


if [ -z "${ARX_SIGNING_KEY:-}" ]; then
  echo "ARX_SIGNING_KEY is required for the public Pages repo so clients keep trusting a stable key." >&2
  exit 1
fi
./build/arx init public --no-key
printf '%s' "$ARX_SIGNING_KEY" > /tmp/key.asc
./build/arx key import /tmp/key.asc --root public
./build/arx add dist/*.deb dist/*.rpm --root public
./build/arx publish --root public
cp public/keys/public.asc public/public.asc
rm -f public/keys/private.asc /tmp/key.asc
test -s public/public.asc
test ! -e public/keys/private.asc
OWNER="$GITHUB_REPOSITORY_OWNER"
REPOSITORY="$GITHUB_REPOSITORY"
REPO="$PAGES_REPOSITORY_NAME"
if [ -n "${PAGES_BASE_URL:-}" ]; then
  REPO_URL="${PAGES_BASE_URL%/}"
elif [ "$REPO" = "$OWNER.github.io" ]; then
  REPO_URL="https://${OWNER}.github.io"
else
  REPO_URL="https://${OWNER}.github.io/${REPO}"
fi
cat > public/install.sh <<'SCRIPT'
#!/bin/sh
set -eu

REPO_URL="__REPO_URL__"
run_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    echo "This installer needs root privileges or sudo." >&2
    exit 1
  fi
}

if command -v apt-get >/dev/null 2>&1; then
  run_root install -d -m 0755 /etc/apt/keyrings
  curl -fsSL "$REPO_URL/public.asc" | run_root tee /etc/apt/keyrings/arx.asc >/dev/null
  echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/arx.asc] $REPO_URL/apt stable main" | run_root tee /etc/apt/sources.list.d/arx.list >/dev/null
  run_root apt-get update
  run_root apt-get install -y arx
elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; then
  PM="dnf"
  command -v dnf >/dev/null 2>&1 || PM="yum"
  cat <<REPO | run_root tee /etc/yum.repos.d/arx.repo >/dev/null
[arx]
name=ArtifactX
baseurl=$REPO_URL/yum/myrepo/\$basearch
enabled=1
repo_gpgcheck=1
gpgcheck=0
gpgkey=$REPO_URL/public.asc
REPO
  run_root "$PM" install -y arx
else
  echo "Unsupported system: expected apt-get, dnf, or yum." >&2
  exit 1
fi
SCRIPT
export REPO_URL
python3 - <<'PY'
from pathlib import Path
import os
path = Path('public/install.sh')
path.write_text(path.read_text().replace('__REPO_URL__', os.environ['REPO_URL']))
PY
chmod +x public/install.sh

cat > public/robots.txt <<TXT
User-agent: *
Allow: /
Sitemap: ${REPO_URL}/sitemap.xml
TXT

cat > public/sitemap.xml <<XML
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>${REPO_URL}/</loc>
    <changefreq>weekly</changefreq>
    <priority>1.0</priority>
  </url>
</urlset>
XML

cat > public/index.html <<HTML
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>ArtifactX — publish package repos like a static blog</title>
  <meta name="description" content="ArtifactX imports or creates signed apt and yum repositories, then publishes static files you can host on GitHub Pages, S3, nginx, or arx serve.">
  <meta name="robots" content="index,follow">
  <link rel="canonical" href="${REPO_URL}/">
  <meta property="og:type" content="website">
  <meta property="og:url" content="${REPO_URL}/">
  <meta property="og:title" content="ArtifactX — publish package repos like a static blog">
  <meta property="og:description" content="Import existing apt/yum repos or start from local .deb/.rpm files. ArtifactX signs metadata and emits static files for Pages, S3, nginx, or arx serve.">
  <meta property="og:site_name" content="ArtifactX">
  <meta name="twitter:card" content="summary">
  <meta name="twitter:title" content="ArtifactX — publish package repos like a static blog">
  <meta name="twitter:description" content="One Rust binary to import, sign, publish, serve, promote, prune, and roll back apt/yum package repositories.">
  <script type="application/ld+json">
  {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "name": "ArtifactX",
    "alternateName": "arx",
    "applicationCategory": "DeveloperApplication",
    "operatingSystem": "Linux, macOS",
    "description": "ArtifactX imports or creates signed apt and yum repositories, then publishes static files you can host anywhere.",
    "url": "${REPO_URL}/",
    "codeRepository": "https://github.com/${REPOSITORY}",
    "programmingLanguage": "Rust",
    "license": "https://github.com/${REPOSITORY}/blob/main/LICENSES/GPL-2.0-or-later.txt"
  }
  </script>
  <style>
    :root {
      color-scheme: dark;
      --bg: #080b12;
      --panel: rgba(16, 22, 34, .82);
      --panel-strong: #101827;
      --text: #eef4ff;
      --muted: #a7b4c7;
      --line: rgba(148, 163, 184, .24);
      --cyan: #22d3ee;
      --lime: #a3e635;
      --violet: #a78bfa;
      --amber: #fbbf24;
      --shadow: 0 24px 80px rgba(0, 0, 0, .42);
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background:
        radial-gradient(circle at 14% 6%, rgba(34, 211, 238, .16), transparent 26rem),
        radial-gradient(circle at 86% 4%, rgba(167, 139, 250, .14), transparent 30rem),
        radial-gradient(circle at 80% 76%, rgba(163, 230, 53, .10), transparent 28rem),
        linear-gradient(135deg, #071118 0%, #080b12 48%, #05070b 100%);
      color: var(--text);
      min-height: 100vh;
    }
    a { color: inherit; }
    code { color: #d7fbe8; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
    .wrap { width: min(1180px, calc(100% - 40px)); margin: 0 auto; }
    header {
      min-height: min(760px, 92vh);
      display: grid;
      grid-template-columns: minmax(0, 1.05fr) minmax(340px, .72fr);
      gap: clamp(28px, 5vw, 72px);
      align-items: center;
      padding: clamp(44px, 7vw, 86px) 0 clamp(30px, 5vw, 56px);
    }
    .eyebrow {
      display: inline-flex;
      gap: 10px;
      align-items: center;
      padding: 8px 12px;
      border: 1px solid var(--line);
      border-radius: 999px;
      color: var(--cyan);
      background: rgba(8, 13, 24, .72);
      font: 750 12px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
      letter-spacing: .10em;
      text-transform: uppercase;
    }
    h1 {
      margin: 22px 0 18px;
      max-width: 820px;
      font-size: clamp(48px, 6.8vw, 86px);
      line-height: 1.02;
      letter-spacing: -.045em;
      text-wrap: balance;
    }
    .grad {
      background: linear-gradient(90deg, var(--cyan), var(--lime), var(--amber));
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
    }
    .lead {
      max-width: 720px;
      color: var(--muted);
      font-size: clamp(18px, 2vw, 22px);
      line-height: 1.62;
      margin: 0;
    }
    .actions { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 30px; }
    .btn {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      border: 1px solid var(--line);
      border-radius: 14px;
      padding: 13px 16px;
      min-height: 50px;
      text-decoration: none;
      background: rgba(255,255,255,.055);
      font-weight: 750;
      transition: transform .18s ease, border-color .18s ease, background .18s ease;
    }
    .btn:hover { transform: translateY(-2px); border-color: var(--cyan); background: rgba(34,211,238,.10); }
    .btn.primary { background: linear-gradient(135deg, rgba(34,211,238,.22), rgba(163,230,53,.16)); border-color: rgba(34,211,238,.55); }
    .hero-panel {
      border: 1px solid rgba(148, 163, 184, .28);
      background: linear-gradient(180deg, rgba(17,24,39,.86), rgba(7,11,18,.88));
      border-radius: 28px;
      padding: 22px;
      box-shadow: var(--shadow), 0 0 100px rgba(34,211,238,.10);
    }
    .hero-panel h2 { margin: 0 0 12px; font-size: 22px; letter-spacing: -.02em; }
    .steps { display: grid; gap: 10px; margin: 18px 0 0; }
    .step {
      display: grid;
      grid-template-columns: 34px 1fr;
      gap: 12px;
      align-items: start;
      padding: 13px;
      border: 1px solid rgba(148,163,184,.18);
      border-radius: 16px;
      background: rgba(255,255,255,.035);
    }
    .num {
      display: grid;
      place-items: center;
      width: 34px;
      height: 34px;
      border-radius: 12px;
      background: rgba(34,211,238,.14);
      color: var(--cyan);
      font: 800 13px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .step strong { display: block; margin-bottom: 3px; }
    .step span { color: var(--muted); line-height: 1.45; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 16px; margin: 22px 0 34px; }
    .card {
      min-width: 0;
      border: 1px solid var(--line);
      background: var(--panel);
      box-shadow: var(--shadow);
      border-radius: 22px;
      padding: 20px;
      backdrop-filter: blur(16px);
    }
    .card.feature { display: flex; flex-direction: column; }
    .card strong { color: var(--text); }
    .card p { color: var(--muted); line-height: 1.55; margin: 10px 0 0; }
    .tag { color: var(--lime); font: 750 12px/1 ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .1em; text-transform: uppercase; }
    .terminal {
      min-width: 0;
      border: 1px solid rgba(34,211,238,.35);
      border-radius: 22px;
      overflow: hidden;
      background: #05070b;
      box-shadow: var(--shadow), 0 0 80px rgba(34,211,238,.10);
      margin: 26px 0;
    }
    .feature .terminal { margin-top: auto; margin-bottom: 0; }
    .bar { display: flex; gap: 8px; padding: 12px 14px; border-bottom: 1px solid var(--line); background: #0c111b; }
    .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--cyan); opacity: .8; }
    pre {
      margin: 0;
      overflow-x: auto;
      padding: 20px;
      color: #d7fbe8;
      font: 14px/1.7 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    }
    section { padding: 20px 0; }
    h2 { font-size: clamp(28px, 4vw, 42px); letter-spacing: -.035em; margin: 0 0 14px; line-height: 1.08; }
    .split { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 16px; align-items: stretch; }
    footer { color: var(--muted); border-top: 1px solid var(--line); padding: 30px 0 48px; margin-top: 30px; }
    @media (max-width: 920px) {
      header { grid-template-columns: 1fr; min-height: auto; }
      .hero-panel { max-width: 720px; }
    }
    @media (max-width: 780px) {
      .wrap { width: min(100% - 28px, 1180px); }
      .split { grid-template-columns: 1fr; }
      h1 { font-size: clamp(42px, 12vw, 64px); letter-spacing: -.035em; }
      .lead { font-size: 18px; }
    }
  </style>
</head>
<body>
  <header class="wrap">
    <div>
      <span class="eyebrow">Static hosting for package repos.</span>
      <h1>Publish package repos like a <span class="grad">static blog</span>.</h1>
      <p class="lead">Import an existing apt/yum repo or start from local .deb/.rpm files. ArtifactX signs the metadata and emits static files you can host on Pages, S3, nginx, or <code>arx serve</code>.</p>
      <div class="actions">
        <a class="btn primary" href="${REPO_URL}/install.sh">Install arx</a>
        <a class="btn" href="${REPO_URL}/apt/dists/stable/Release">APT metadata</a>
        <a class="btn" href="${REPO_URL}/yum/myrepo/x86_64/repodata/repomd.xml">YUM metadata</a>
        <a class="btn" href="https://github.com/${REPOSITORY}/blob/main/docs/README.md">Docs</a>
        <a class="btn" href="https://github.com/${REPOSITORY}/blob/main/docs/how-to/publish-with-github-pages.md">Pages guide</a>
        <a class="btn" href="https://github.com/${REPOSITORY}">GitHub</a>
      </div>
    </div>
    <aside class="hero-panel" aria-label="ArtifactX quick path">
      <h2>From packages to static hosting</h2>
      <div class="steps">
        <div class="step"><span class="num">01</span><span><strong>Bring packages</strong>Import upstream metadata or add local .deb/.rpm files.</span></div>
        <div class="step"><span class="num">02</span><span><strong>Publish</strong>Generate signed apt/yum metadata under your own key.</span></div>
        <div class="step"><span class="num">03</span><span><strong>Host</strong>Upload static files or run the same binary as a server.</span></div>
      </div>
    </aside>
  </header>

  <main class="wrap">
    <section>
      <div class="terminal">
        <div class="bar"><span class="dot"></span><span class="dot"></span><span class="dot"></span></div>
        <pre># One installer, apt or dnf/yum.
curl -fsSL ${REPO_URL}/install.sh | sh

# Want to inspect first?
curl -fsSL ${REPO_URL}/install.sh</pre>
      </div>
    </section>

    <section class="grid">
      <article class="card"><span class="tag">Import</span><p><strong>Migrate existing repos.</strong> Pull from apt Packages metadata or yum repodata, then publish your own signed repo. It is a controlled migration path, not a blind mirror.</p></article>
      <article class="card"><span class="tag">Create</span><p><strong>Start from local packages too.</strong> Run <code>arx init</code>, add .deb/.rpm files, publish, and serve from the same binary.</p></article>
      <article class="card"><span class="tag">Rollback</span><p><strong>Bad release?</strong> Keep published states and return clients to a previous repo snapshot.</p></article>
      <article class="card"><span class="tag">Static</span><p><strong>Host it like a blog.</strong> Publish signed repo files to GitHub Pages, S3, nginx, or serve them from the same <code>arx</code> binary.</p></article>
    </section>

    <section class="split">
      <div class="card feature">
        <h2>Import an upstream repo</h2>
        <p>Start with packages you already publish elsewhere. Filter the first migration, verify apt/dnf clients, then regenerate static repo files under your own stable signing key.</p>
        <div class="terminal"><pre>arx init ./repo
arx import https://packages.example.com --apt \
  --dist stable --component main \
  --match-name myapp --limit 20
arx publish --root ./repo
arx serve --root ./repo  # 127.0.0.1:8080</pre></div>
      </div>
      <div class="card feature">
        <h2>Create and serve a new repo</h2>
        <p>Use packages you already built, or package from a manifest or Cargo.toml, then publish a signed repo you can host anywhere static files work.</p>
        <div class="terminal"><pre>arx init ./repo
arx add dist/*.deb dist/*.rpm --root ./repo
arx publish --root ./repo
arx serve --root ./repo  # 127.0.0.1:8080</pre></div>
      </div>
    </section>

    <section class="split">
      <div class="card feature">
        <h2>Docker users get Compose</h2>
        <p>Generate deploy files from the same repo. Compose binds inside the container to <code>0.0.0.0:8080</code> so Docker port publishing works.</p>
        <div class="terminal"><pre>arx compose --root ./repo --out ./deploy
cd ./deploy
docker compose up -d</pre></div>
      </div>
      <div class="card feature">
        <h2>Operators get API + systemd</h2>
        <p><code>arx serve</code> defaults to localhost. Put systemd and a reverse proxy around it; use <code>ARX_SERVE_TOKEN</code> only when write APIs are enabled.</p>
        <div class="terminal"><pre>arx serve --root /var/lib/arx/repo
curl -fsS http://127.0.0.1:8080/api/v1/health
journalctl -u arx -f</pre></div>
      </div>
    </section>
  </main>

  <footer class="wrap">
    <p>ArtifactX: turn Linux package repository publishing into a static-site workflow — import, package, sign, host, promote, prune, and roll back from one binary. Need production details? Read the <a href="https://github.com/${REPOSITORY}/blob/main/docs/README.md">docs</a> or the <a href="https://github.com/${REPOSITORY}/blob/main/docs/how-to/publish-with-github-pages.md">Pages deployment guide</a>.</p>
  </footer>
</body>
</html>
HTML


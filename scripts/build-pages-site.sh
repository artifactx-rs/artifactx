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
if ./build/arx add --help | grep -q 'directories'; then
  ./build/arx add dist --root public
else
  ./build/arx add dist/*.deb dist/*.rpm --root public
fi
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
export REPO_URL REPOSITORY
python3 - <<'PY'
from pathlib import Path
import os

replacements = {
    '__REPO_URL__': os.environ['REPO_URL'],
    '__REPOSITORY__': os.environ['REPOSITORY'],
    '__ARX_VERSION__': os.environ['ARX_VERSION'],
}
files = {
    'site/install.sh.in': 'public/install.sh',
    'site/robots.txt.in': 'public/robots.txt',
    'site/sitemap.xml.in': 'public/sitemap.xml',
    'site/index.html': 'public/index.html',
}
for source, target in files.items():
    rendered = Path(source).read_text()
    if source == 'site/index.html' and '__ARX_VERSION__' in rendered:
        raise SystemExit('landing page must not hardcode the Cargo version; use current/latest copy instead')
    for needle, value in replacements.items():
        rendered = rendered.replace(needle, value)
    Path(target).write_text(rendered)
PY
chmod +x public/install.sh

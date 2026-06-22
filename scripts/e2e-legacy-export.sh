#!/usr/bin/env bash
set -euo pipefail

# End-to-end legacy-public-layout test for production cutovers.
# It validates that ArtifactX can export:
#   /deb  -> apt layout: dists/stable + pool/main
#   /repo -> flat yum layout: *.rpm + repodata/*.xml.gz
# The yum check intentionally uses CentOS 7 by default because that client line
# only understands gzip metadata in this deployment.

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ARX_BIN=${ARX_BIN:-"$ROOT_DIR/target/debug/arx"}
WORK=${WORK:-"$(mktemp -d "${TMPDIR:-/tmp}/arx-legacy-export-e2e.XXXXXX")"}
APT_IMAGE=${APT_IMAGE:-debian:12-slim}
YUM_IMAGE=${YUM_IMAGE:-quay.io/centos/centos:7}
DOCKER_PLATFORM=${DOCKER_PLATFORM:-linux/amd64}
KEEP_WORK=${KEEP_WORK:-0}

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  if [[ "$KEEP_WORK" != 1 ]]; then rm -rf "$WORK"; else printf 'kept workdir: %s\n' "$WORK" >&2; fi
}
trap cleanup EXIT

if [[ ! -x "$ARX_BIN" ]]; then
  cargo build -p artifactx --bin arx
fi
command -v docker >/dev/null || { echo "docker is required for this e2e" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required for the static test server" >&2; exit 1; }

repo="$WORK/repo"
public="$WORK/public"
mkdir -p "$public"

make_deb() {
  local out=$1 name=$2 version=$3 arch=$4
  local tmp="$WORK/debbuild-$name"
  rm -rf "$tmp" && mkdir -p "$tmp/DEBIAN" "$tmp/usr/bin"
  cat >"$tmp/DEBIAN/control" <<CONTROL
Package: $name
Version: $version
Architecture: $arch
Maintainer: ArtifactX E2E <e2e@artifactx.local>
Description: ArtifactX legacy export e2e package
CONTROL
  printf '#!/bin/sh\necho %s\n' "$name" >"$tmp/usr/bin/$name"
  chmod 0755 "$tmp/usr/bin/$name"
  dpkg-deb --build "$tmp" "$out" >/dev/null
}

payload="$WORK/rpm-payload.sh"
manifest="$WORK/rpm.toml"
dist="$WORK/dist"
printf '#!/bin/sh\necho rpmexport\n' >"$payload"
cat >"$manifest" <<EOF_MANIFEST
name = "rpmexport"
version = "1.0.0"
arch = "x86_64"
maintainer = "ArtifactX E2E <e2e@artifactx.local>"
description = "ArtifactX legacy export rpm"
license = "MIT"
[[files]]
source = "$payload"
dest = "/usr/bin/rpmexport"
mode = "0755"
EOF_MANIFEST

make_deb "$WORK/hello_1.0-1_amd64.deb" hello 1.0-1 amd64
"$ARX_BIN" pack "$manifest" --out "$dist" --rpm
"$ARX_BIN" init "$repo" --no-key
"$ARX_BIN" add "$WORK/hello_1.0-1_amd64.deb" "$dist/rpmexport-1.0.0-1.x86_64.rpm" \
  --root "$repo" --component main --repo qgnet
"$ARX_BIN" publish --root "$repo" --full
"$ARX_BIN" export --root "$repo" --apt-out "$public/deb" --yum-flat-out "$public/repo" --repo qgnet --arch x86_64

# Static checks before containers.
test -f "$public/deb/dists/stable/Release"
test -f "$public/deb/dists/stable/main/binary-amd64/Packages.gz"
test -f "$public/repo/repodata/repomd.xml"
test -f "$public/repo/repodata/sha256-primary.xml.gz"
if grep -q '\.xml\.xz' "$public/repo/repodata/repomd.xml"; then
  echo "repomd.xml references xz metadata; CentOS 7 compatibility requires gzip" >&2
  exit 1
fi

port=${PORT:-18080}
(
  cd "$public"
  exec python3 -m http.server "$port" --bind 0.0.0.0
) >/tmp/arx-legacy-export-e2e-http.log 2>&1 &
SERVER_PID=$!
sleep 1

host_url="http://host.docker.internal:$port"
network_args=()
if [[ "$(uname -s)" == Linux ]]; then
  host_url="http://127.0.0.1:$port"
  network_args=(--network host)
fi

# apt layout e2e. Use trusted=yes here so this test focuses on public layout,
# Packages hashes, and payload fetches; signature verification is covered by
# publish/signing tests and production key checks.
docker run --rm --platform "$DOCKER_PLATFORM" "${network_args[@]}" "$APT_IMAGE" bash -ceu "
  rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
  echo 'deb [trusted=yes] $host_url/deb stable main' >/etc/apt/sources.list.d/arx.list
  apt-get update -o Acquire::Retries=0
  apt-cache policy hello | grep -q '1.0-1'
  apt-get download hello
  test -s hello_1.0-1_amd64.deb
"

# CentOS 7 yum layout e2e. Disable all image repos so EOL upstream mirrors do
# not affect the local repo check.
docker run --rm --platform "$DOCKER_PLATFORM" "${network_args[@]}" "$YUM_IMAGE" bash -ceu "
  cat >/etc/yum.repos.d/arx.repo <<REPO
[arx]
name=ArtifactX legacy export
baseurl=$host_url/repo
enabled=1
gpgcheck=0
repo_gpgcheck=0
REPO
  yum --disablerepo='*' --enablerepo=arx makecache fast
  yum --disablerepo='*' --enablerepo=arx list rpmexport | grep -q '1.0.0'
"

printf 'legacy export e2e passed: %s\n' "$WORK"

#!/usr/bin/env python3
"""Keep release-facing version pins in sync with the Cargo package version."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CARGO_MANIFESTS = [
    ROOT / "crates/arx-debrepo/Cargo.toml",
    ROOT / "crates/arx-pack/Cargo.toml",
    ROOT / "crates/arx/Cargo.toml",
]
SYNCED_FILES = [
    ROOT / "packaging/arx.toml",
    ROOT / "docker-compose.yml",
    ROOT / "flake.nix",
]


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def package_version(manifest: Path) -> str:
    text = read(manifest)
    in_package = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("["):
            break
        if in_package:
            match = re.match(r'version\s*=\s*"([^"]+)"\s*$', stripped)
            if match:
                return match.group(1)
    raise SystemExit(f"version field not found in [package]: {manifest.relative_to(ROOT)}")


def replace_once(text: str, pattern: str, replacement: str, path: Path) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"expected exactly one match for {pattern!r} in {path.relative_to(ROOT)}; found {count}")
    return updated


def expected_versions() -> str:
    versions = {manifest.relative_to(ROOT): package_version(manifest) for manifest in CARGO_MANIFESTS}
    unique = set(versions.values())
    if len(unique) != 1:
        details = ", ".join(f"{path}={version}" for path, version in versions.items())
        raise SystemExit(f"crate package versions are not aligned: {details}")
    return unique.pop()


def sync_text(path: Path, version: str) -> str:
    text = read(path)
    if path.match("packaging/arx.toml"):
        return replace_once(text, r'^version\s*=\s*"[^"]+"', f'version = "{version}"', path)
    if path.match("docker-compose.yml"):
        return replace_once(text, r'ghcr\.io/artifactx-rs/arx:v[^\s]+', f'ghcr.io/artifactx-rs/arx:v{version}', path)
    if path.match("flake.nix"):
        return replace_once(text, r'version\s*=\s*"[^"]+";', f'version = "{version}";', path)
    raise SystemExit(f"no sync rule for {path.relative_to(ROOT)}")


def check_path_dependency_versions(version: str) -> list[str]:
    errors: list[str] = []
    arx_manifest = ROOT / "crates/arx/Cargo.toml"
    text = read(arx_manifest)
    for dep in ("arx-debrepo", "arx-pack"):
        pattern = rf'{dep}\s*=\s*\{{[^}}]*version\s*=\s*"([^"]+)"[^}}]*path\s*='
        match = re.search(pattern, text)
        if not match:
            errors.append(f"{arx_manifest.relative_to(ROOT)}: dependency {dep} must declare version + path")
        elif match.group(1) != version:
            errors.append(
                f"{arx_manifest.relative_to(ROOT)}: dependency {dep} version {match.group(1)} != package version {version}"
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if synced files would change")
    parser.add_argument("--version", help="override Cargo-derived version when syncing")
    args = parser.parse_args()

    version = args.version or expected_versions()
    errors = check_path_dependency_versions(version)
    changed: list[Path] = []

    for path in SYNCED_FILES:
        original = read(path)
        updated = sync_text(path, version)
        if original != updated:
            changed.append(path)
            if not args.check:
                write(path, updated)

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    if args.check and changed:
        print(f"version sync check failed; expected {version} in:", file=sys.stderr)
        for path in changed:
            print(f"  - {path.relative_to(ROOT)}", file=sys.stderr)
        print("run: scripts/sync-version.py", file=sys.stderr)
        return 1

    if changed:
        for path in changed:
            print(f"updated {path.relative_to(ROOT)} -> {version}")
    else:
        print(f"version sync ok ({version})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

# ADR-0020: Ship a packaged systemd service unit without auto-starting it

- Status: Proposed
- Date: 2026-06-22

## Context

ArtifactX documents a production `arx serve` systemd unit, but the release `.deb`
and `.rpm` packages only installed `/usr/bin/arx`. During dogfood this made package
install feel incomplete: the binary was present, but the operator still had to copy
service boilerplate by hand.

The unit is useful, but it is also operationally sensitive:

- repositories, signing keys, and passphrases are site-specific;
- public exposure should normally happen through a reverse proxy;
- package installation must not unexpectedly start a write-capable service;
- Debian and RPM systems both consume systemd units, but package-manager service
  lifecycle helpers are ecosystem-specific.

## Decision

Ship a conservative `arx.service` file in release packages at
`/usr/lib/systemd/system/arx.service`.

The packaged unit:

- runs `/usr/bin/arx serve --root /var/lib/arx/repo --addr 127.0.0.1:8080`;
- uses an optional `/etc/arx/arx.env` for `ARX_SERVE_TOKEN` and key passphrases;
- assumes an `arx` system user/group and `/var/lib/arx/repo` repository root;
- applies basic systemd hardening (`NoNewPrivileges`, `PrivateTmp`,
  `ProtectSystem=strict`, `ProtectHome=true`, `ReadWritePaths=/var/lib/arx/repo`);
- is installed but not automatically enabled or started by maintainer scripts.

Operators still explicitly create the user/repo, review secrets, then run
`systemctl enable --now arx.service` when ready.

## Consequences

- Good: release packages include the expected service file; operators no longer
  have to copy it from docs.
- Good: installing the package remains safe and quiet; no surprise daemon starts,
  no accidental public bind, no default write token.
- Good: the same unit works for the documented reverse-proxy-first deployment.
- Cost: package install does not fully provision the service user/repo yet.
- Cost: richer Debian/RPM lifecycle integration (`sysusers.d`, `tmpfiles.d`,
  `postinst`, `%post`) remains future work.

## Alternatives considered

1. **Do not package the unit; keep docs only.** Rejected: dogfood showed this feels
   broken when users install a package and expect a service to be available.
2. **Auto-enable and auto-start after install.** Rejected: unsafe without local
   repo/key/auth decisions; could expose stale or unauthenticated endpoints.
3. **Generate separate Debian/RPM service paths.** Deferred: current pack manifest
   is cross-format; `/usr/lib/systemd/system` is the least surprising shared path
   for modern systemd systems.

## Future improvements

- Add optional `sysusers.d` / `tmpfiles.d` support once `arx pack` can express
  package-manager-specific helper files cleanly.
- Add explicit maintainer-script examples for teams that want auto-provisioning.
- Add `arx doctor service` or `arx init --systemd` to validate the user, repo root,
  env file permissions, and reverse proxy health.

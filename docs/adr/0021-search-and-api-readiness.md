# ADR-0021: Add package search before calling the HTTP API stable

- Status: Proposed
- Date: 2026-06-22

## Context

ArtifactX already exposes a useful `/api/v1` surface: health, package list,
upload/delete, GC, publish, history, rollback, import, and promote. That is enough
for friendly early adopters and internal automation.

It is not yet enough to call the API stable for general developer consumption.
Real operators need to answer package questions before mutating a repository:

- Which versions of `demo` are present?
- Is a package in apt, yum, staging, prod, or a specific architecture?
- What would `gc`, `rm`, or `promote` affect?

Today that requires fetching a broad list and post-filtering locally.

## Decision

Add a first-class read model around package search:

- CLI: `arx search <query>` with filters for apt/yum, scope/component/repo, arch,
  version, and `--json`.
- API: expose an equivalent read-only query endpoint, or document a deliberately
  CLI-only v0.2 cut if the server-side shape is not ready.
- Implementation: reuse package metadata scanners from list/rm/gc; do not rely on
  filename parsing when package metadata is available.

Until search/filtering, pagination considerations, and error-shape consistency are
reviewed, `/api/v1` should be described as suitable for early developer use but not
as a stable long-term compatibility contract.

## Consequences

- Good: users can inspect before destructive actions, reducing GC/RM mistakes.
- Good: scripts can consume `--json` instead of scraping human list output.
- Good: API stability becomes an explicit checklist rather than an implied promise.
- Cost: this adds one more public command/API shape that needs compatibility care.
- Cost: metadata scanning must remain fast enough for large pools or gain paging.

## Alternatives considered

1. **Tell users to use `arx list | grep`.** Rejected: weak for scripts, mixed
   apt/yum scopes, versions, and future large repos.
2. **Only add API filtering.** Rejected: operators need the same affordance locally
   before service/API rollout.
3. **Make search regex-only.** Rejected for the default path: prefix/substring is
   friendlier; regex can be added carefully if it stays documented and optional.

## Future improvements

- Add pagination or streaming if large repos make full JSON responses too heavy.
- Add saved queries or output columns if operator workflows justify them.
- Publish small SDK/client examples after `/api/v1` compatibility policy is clear.

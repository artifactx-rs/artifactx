# Architecture Decision Records

An ADR captures **one decision**: the context, what we chose, the consequences, and
how we'd improve it later. They exist so a new contributor can understand *why*
ArtifactX is the way it is — and disagree with evidence, not guesswork.

## Process: documentation-first

> **Design first → review → then build.** Non-trivial features start as a
> `Proposed` ADR, not as code.

1. **Propose** — open an ADR with status `Proposed`. Describe the problem and the
   chosen approach *before* writing the feature.
2. **Review** — discuss it (PR review, or an adversarial pass against the
   [charter](../../CLAUDE.md) and [`COMPETITORS.md`](../../COMPETITORS.md)). Does it
   serve Build/Package/Publish? Keep the 5-minute rule? Could two things be deleted?
3. **Accept** — mark it `Accepted` and implement. The ADR is the contract.
4. **Supersede** — never rewrite history. If a decision changes, add a new ADR and
   set the old one's status to `Superseded by ADR-XXXX`.

Trivial changes (typos, refactors, dep bumps) skip the ADR.

## Lifecycle

`Proposed` → `Accepted` → (`Superseded` | `Deprecated`)

## Template

```markdown
# ADR-NNNN: Short title

- Status: Proposed | Accepted | Superseded by ADR-XXXX
- Date: YYYY-MM-DD

## Context
What problem are we solving? What forces are at play (the charter, a competitor, a
constraint)?

## Decision
What we chose, stated plainly.

## Consequences
- Good: …
- Bad / cost: …

## Alternatives considered
What else we looked at and why we passed.

## Future improvements
How we'd make this better later, and what would trigger that.
```

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-workspace-and-licensing.md) | Cargo workspace + split licensing | Accepted |
| [0002](0002-in-house-apt-generator.md) | In-house apt generator (`debrepo`) | Accepted |
| [0003](0003-v4-rsa-signing.md) | v4 RSA PGP signing | Accepted |
| [0004](0004-atomic-publish-by-hash.md) | Atomic publish via staging + by-hash | Accepted |
| [0005](0005-pack-manifest-native.md) | `pack`: manifest → native, never conversion | Accepted |
| [0006](0006-http-api-and-push.md) | HTTP write API + `arx push` | Accepted |
| [0007](0007-retention-and-gc.md) | Pool retention & GC | Accepted |
| [0008](0008-atomic-rollback.md) | Atomic rollback (pointer-flip) | Proposed |

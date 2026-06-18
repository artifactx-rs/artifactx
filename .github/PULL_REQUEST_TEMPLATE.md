## What changed

-

## Why

-

## AI assistance

- [ ] This PR was AI-assisted; `AI_RULES.md` was followed
- [ ] No generated secret, private key, token, or release credential is included

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] If CI/release changed: `actionlint .github/workflows/*.yml`
- [ ] If docs changed: links and commands were checked for accuracy

## Release / operator impact

- [ ] No secret material is printed or packaged
- [ ] No accidental version bump, tag, release, or GHCR publish path changed
- [ ] Backward compatibility is preserved or migration notes are included

## Notes

Known gaps or follow-up work:

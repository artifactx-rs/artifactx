# Run local E2E checks

Use these checks before changing import, publish, export, API, or cutover
behavior. They exercise the migration fixtures that protect v0.2 publish/API
completeness.

## Fast migration fixture checks

Run individual fixtures while iterating:

```bash
cargo test -p artifactx --test full_cli_regression import_accepts_aptly_hash_prefixed_deb_filenames -- --nocapture
cargo test -p artifactx --test full_cli_regression yum_import_accepts_noncanonical_rpm_filenames_and_xz_metadata -- --nocapture
cargo test -p artifactx --test full_cli_regression yum_import_skips_invalid_metadata_entries_and_keeps_importing -- --nocapture
cargo test -p artifactx --test full_cli_regression import_api_publish_true_imports_and_publishes_apt_metadata -- --nocapture
cargo test -p artifactx --test full_cli_regression api_workflow_covers_documented_publish_history_rollback_and_promote -- --nocapture
cargo test -p artifactx --test full_cli_regression export_builds_legacy_apt_and_centos7_friendly_flat_yum_layout -- --nocapture
cargo test -p artifactx --test full_cli_regression cutover_ -- --nocapture
```

These cover:

| Fixture | Protected behavior |
| --- | --- |
| apt identity preservation | Imported apt `Release` identity remains stable after ArtifactX publish. |
| aptly hash-prefixed `.deb` import | Non-canonical pool filenames import and publish correctly. |
| dirty yum metadata | Invalid yum entries are reported clearly; strict mode fails. |
| yum xz import | Upstream xz metadata can be imported. |
| CentOS 7 gzip-only clients | Exported yum metadata remains gzip-compatible. |
| API workflow | Upload, list/search, GC dry-run, publish, history, rollback, and promote. |
| cutover preflight | Staged export validation, live symlink switch, rollback pointer, and RPM payload signature gate. |

## Full local gate

Before pushing a PR that changes Rust behavior, run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The repository pre-push hook runs clippy and the workspace test suite again.

## Legacy layout client check

If Docker is available, run the legacy public-layout E2E:

```bash
scripts/e2e-legacy-export.sh
```

This validates apt and flat yum public layouts through real clients. The yum
client path intentionally checks gzip metadata compatibility because older yum
clients cannot consume xz-only metadata.

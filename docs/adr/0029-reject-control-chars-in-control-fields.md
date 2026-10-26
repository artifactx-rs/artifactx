# ADR-0029: Reject control characters in Debian control fields

- Status: Accepted
- Date: 2026-10-26
- Target: v0.3.x ingestion

## Context

A package can ship a control file that `dpkg` refuses but a lenient reader
accepts. The case that surfaced in production: a `.deb` whose `Version` field
carried a stray CR from its packaging tool, i.e. the bytes were
`Version: 1.2.3\r-1`, with no LF involved.

Two things went wrong once that file reached a repository:

- `arx` indexed the field verbatim, so the generated `Packages` stanza held a
  carriage return inside `Version`. `dpkg-deb` rejects that package outright
  (`invalid character in version number`), and `apt` refuses the same stanza —
  the artifact was never installable, yet it was advertised.
- Anything that derived a version from the index inherited the stray byte. A
  downstream helper that trims `\r\n` off a version string is not enough: the
  byte sat *inside* the value, so it produced a "version" that matched no
  package, and every consumer of that derived version failed closed but
  confusingly.

`str::lines` strips exactly one trailing `\r` per line, which is why ordinary
CRLF-authored control files parsed fine and hid the difference: only an interior
control character survives.

## Decision

Validate control fields while parsing, in `arx-debrepo::parse_control`:

- reject ASCII control characters (`U+0000`–`U+0008`, `U+000B`, `U+000C`,
  `U+000D`, `U+000E`–`U+001F`, `U+007F`) in field names and values;
- keep newlines and tabs allowed, because folded values legitimately contain
  them;
- fail with the field name and the offending byte offset, so the operator can
  find the package instead of guessing which of a thousand artifacts is broken.

Rejection happens at read time. Every ingest path reads the control file before
the artifact becomes visible:

- `arx add` validates before copying, so the pool never receives it;
- `arx push` and `POST /api/v1/packages` validate the uploaded bytes before
  writing anything, so a rejected upload cannot overwrite a same-named valid
  package;
- `arx publish` (including pool rescans) skips or rejects it instead of
  advertising a stanza no client can parse.

A repository that already contains such a package stops advertising it on the
next publish, which is the intended behaviour: an index that no client can
parse is worse than a skipped package.

### Pre-rendered caches are re-validated

Incremental publish and `search` reuse stanzas from `.arx-manifest.toml`, keyed
by `(mtime, size)`, without re-opening the `.deb` (ADR-0013). A manifest written
by an older binary can therefore hold a stanza that already contains the stray
byte while the file itself is unchanged — exactly the case where a naive
"validate on parse" fix would look green and still republish the bad version.
Both cache readers re-validate what they reuse:

- `arx-debrepo` discards the **whole** cached entry (not just the identity) when
  its stanza or identity carries a control character. Filtering only the identity
  would not help: once the `.deb` parses, the slow path also reuses
  `cached.stanza`, so a clean file with a poisoned stanza would still publish the
  stray byte;
- `arx::pool` does the same for `search`, `rm`, and `gc`.

Pool *scans* skip a file the parser refuses (with a warning) instead of aborting
the whole command: otherwise a single legacy artifact would make `search`, `rm`,
`gc`, and the recovery path itself unusable. Operators remove such a file
directly, or with a binary predating this change.

## Consequences

- Bad artifacts are refused at ingest instead of being advertised and then
  failing on every client.
- CRLF control files keep working; the regression tests pin both directions
  (interior CR and NUL rejected, CRLF accepted).
- Parse errors now mention the field name and byte offset, so an operator can
  trace the offending package without a binary diff.
- A poisoned incremental cache cannot republish the byte: the regression test
  writes a CR-versioned stanza into `.arx-manifest.toml`, leaves the `.deb`
  untouched, and asserts that strict publish fails, default publish skips, and
  `search` reports nothing.
- `arx rm` cannot yank a package the parser refuses (it never appears in the
  scan). This is deliberate: yanking by parsed identity would require parsing it
  successfully.
- This validates the *control* channel only. An upstream index imported with
  `arx import` is parsed by the index reader, not by this control parser, so the
  same guarantee for imported metadata is a separate change.

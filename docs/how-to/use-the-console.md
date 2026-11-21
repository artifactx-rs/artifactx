# Use the A2UI console

`arx serve` exposes a read-only console at `/ui`:

```bash
arx serve --root ./repo
# http://127.0.0.1:8080/ui
```

The page is generated from A2UI v1.0 messages by the Rust server and rendered
with `@a2ui/react`. It shows repository counts, the latest publish time, and a
searchable package list. Search is an A2UI action: the client sends
`GET /api/v1/ui/search?q=…`, and the server responds with `/query` and
`/packages` data-model updates. Repository-wide metrics stay unchanged.

The console is read-only in this slice. Upload, delete, publish, cutover, and
rollback actions remain CLI/API operations. The embedded bundle is built from
`crates/arx/ui/`; see [`crates/arx/ui/README.md`](../../crates/arx/ui/README.md)
for the local build.

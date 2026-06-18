# Repository instructions for AI coding tools

Follow [`AI_RULES.md`](../AI_RULES.md) and [`CLAUDE.md`](../CLAUDE.md) before making changes.

Current product focus: import-first polish and feature freeze. Keep the two primary user paths simple:

1. Import existing apt/yum packages, regenerate signed metadata, serve the repo, and cut over clients when verified.
2. Create a new repo, add or pack packages, publish, serve, and install with apt/dnf.

Do not broaden scope without an explicit maintainer request and a design note or ADR.

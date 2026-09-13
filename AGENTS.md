# Nexus agent entry point

Read [CLAUDE.md](CLAUDE.md) before making changes. It is the shared contributor
and AI-agent guide for this repository, including Codex. Follow its working
protocol, approval gates, transmit-safety invariants, and verification requirements.

Read [ARCHITECTURE.md](ARCHITECTURE.md) to locate the relevant subsystem, then
read the module's own contract and the existing implementation before editing.
Use [.github/workflows/ci.yml](.github/workflows/ci.yml) for the current verification
commands and feature combinations.

For UI and layout work, also read
[the UI layout skill](.claude/skills/ui-layout/SKILL.md). For release documentation,
read [the release documentation skill](.claude/skills/release-docs/SKILL.md).

Keep the operating rules in their existing source. Do not duplicate them here or
hand-edit the generated rules inside `CLAUDE.md`. This file provides instruction
discovery; it does not change the project's rules or authorize any action.

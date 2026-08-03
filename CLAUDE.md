# Nexus — contributor & AI-agent guide

Nexus is a Rust + Tauri amateur-radio operations center (~200K lines own code, 16+ crates,
vendored WSJT-X DSP cores). This file is loaded by AI coding agents and read by humans; it holds
the invariants that are expensive to rediscover. Architecture map: [ARCHITECTURE.md](ARCHITECTURE.md).

## Build & verify — the traps

| Invariant | Detail |
|---|---|
| `cargo test --workspace` **excludes src-tauri** | src-tauri is not a workspace member. Also run: `cargo test --manifest-path src-tauri/Cargo.toml --lib --features radio` |
| src-tauri needs `--features radio` | Without it, ~13 phantom `tempo_audio` unresolved-crate errors. They are not real. |
| tempo-audio full tests | `cargo test -p tempo-audio --features device,serial` — and **clippy needs the same features**: CI lints `cargo clippy tempo-audio --features device,serial`; a plain `--workspace` clippy sweep misses feature-gated code (bit 2026-08-02) |
| propagation live fetchers | `--features live` |
| UI typecheck is `tsc -b` | Not `--noEmit` (project references). Build = `tsc -b && vite build`; tests = `npm test` (vitest) in `ui/` |
| Toolchain pinned 1.93.1 | CI pins exact stable; match it locally for clippy parity |
| CI is the source of truth | `.github/workflows/ci.yml` — its comments document known traps. Read them before changing build wiring. When you learn a new trap, encode it there (or here), not in session memory. |

## Working protocol (multiple agents/sessions run concurrently)

- **Before your first edit**: `git fetch`; check `git rev-list --count HEAD..origin/main` (a stale
  base wastes the whole change set) and `git status` — if the tree carries uncommitted work that
  is not yours, do not edit, stash, or commit it. Use a fresh worktree:
  `git worktree add -b <branch> <path> origin/main`.
- **Independent fixes get their own branch off origin/main.** Never fold unrelated work into a
  topic branch — it entangles unrelated approval decisions at push time.
- **Before any merge**: re-check `git status`; record the pre-merge SHA. Branches move while you
  work here.
- Land small and often. Long-lived branches merge `origin/main` forward regularly.
- Temporary/plan files: `tasks/` is gitignored and machine-local — never `git add -f` it.

## Hard rules (approval-gated — ask the maintainer, every time)

- **Never** push, tag, publish a release, deploy, or run a mirror/publish script without explicit
  maintainer approval for that specific action. Prior approval does not carry over.
- Commits use the **project identity only** (`KD9TAW <kd9taw@protonmail.com>`, repo-local git
  config). The pre-push hook enforces identity and a content scrub — enable it once per clone:
  `git config core.hooksPath .githooks` (see `.githooks/pre-push` header for the scrub-pattern file).
- **Vendored/OSS additions**: per-file license review, GPL-3.0-only compatibility, source headers
  intact, NOTICE entry, README credit — before commit, not after.
- **FT-mode TX/timing/QSO-sequencing changes** require explicit maintainer sign-off. WSJT-X
  behavior is a compatibility contract; on-air correctness cannot be verified in CI.
- Transmit-path safety invariants (TX enable latch, license-class gate, PTT watchdog) are
  load-bearing: never weaken one to fix a test.

## Conventions

- Comment-dense Rust (`//!` module headers everywhere) — match it. Read the module header before
  editing a file; they carry real contracts.
- Tests colocated (`#[cfg(test)] mod tests`) with fixtures in `<crate>/tests/fixtures/`. Bug fixes
  ship with a failing-first repro test.
- CHANGELOG: Keep-a-Changelog, operator-facing prose. New work goes under `## [Unreleased]`;
  `scripts/release-prep <version>` renames it and aligns all version manifests at release time.
- CSS/UI: theme via CSS variables (`--state-*`, `--snr-*`); check a variable exists in *both*
  themes before using it.

## UI layout contract (2026-07 overhaul — read before building any view or pane)

The window-sizing bug class that plagued 0.4–0.21 is dead only while these rules hold. The spec
is the header comment of `ui/src/cockpit-panes.css`; enforcement is `cockpit-panes.test.ts`,
`cockpit-shells.test.ts`, `responsive-vocab.test.ts` (they **compute cascade winners** — never
add a regex-presence CSS test, that is how dead fixes shipped twice).

- A cockpit shell has four child kinds only: header, scope, ONE pane region, one TX dock.
  Every operator-content block renders through `CockpitPaneFrame` with a **role**:
  `fit="content"` for control strips (exactly content height — a strip cannot use surplus),
  fill + `weight` for feeds and the log column. A pane never sizes itself; structural size
  lives in `cockpit-panes.css` (flat selectors, fenced) and only there.
- **The stop line** (2026-08-03): **the operator must never be unable to stop a transmission.**
  Two things hold that, and a hideable pane needs both: **(a)** every control that *stops* one —
  PTT, Stop TX, Tune, the TX-enable latch, abort — has **no id in any pane vocabulary**, so hiding
  it is unrepresentable rather than guarded; **(b)** a pane that may be hidden has a **hide path
  that is itself a stop** — unmounting it ends what it started — and its ⊞ entry says so before
  the tick. Hosting a ■ Stop of its own neither admits nor excludes a pane; (b) is the test.
  (Phone's voice keyer is admitted under (b): unmounting it calls `stopVoice`. The earlier wording
  — "a pane that can only start a transmission may be hidden" — excluded the very pane it was
  written to admit, because the keyer has a Stop button.)
  Two guards, and neither is the rule alone: `panelState.test.ts` checks **names** across every
  vocabulary (`ALL_PANEL_VOCABULARIES`, itself checked against every vocabulary the module
  exports); `components/stop-line.test.tsx` (+ `OperateCockpit.structure.test.tsx` for Operate)
  checks **wiring** — with every id in a cockpit's vocabulary removed, every stop control must
  still be in the document, found by accessible name. A stop control gated on an id called `dsp`
  is caught only by the second; a dead `ptt` entry only by the first. Neither computes that a
  *newly added* stop control was added to its cockpit's sweep list — that step is human.
- Responsive behavior: `[data-viewport='xs|sm|md|lg|xl']` + `--vh-eff`/`--vw-eff` only. Never a
  size-based `@media`, never raw `vh/vw` inside `.app` (zoom-blind) — the portaled
  `.ui-dialog`/`.ui-tooltip` are the one permanent exception (their content re-applies
  `--ui-zoom`; their boxes measure the real window).
- Anything persisted that encodes a size/position/scale is **clamped on load** against the
  current window/monitors, not just at drag time.
- Auto-following feeds use `usePinnedScroll` (never a bare `scrollTop = scrollHeight`);
  programmatic `focus()` uses `preventScroll` + `scrollIntoView({block:'nearest'})`.
- Flex/grid growers must point at content that can actually stretch; a container that clips
  (`overflow:hidden`) may never have hard-floored descendants without an interposed scroller.

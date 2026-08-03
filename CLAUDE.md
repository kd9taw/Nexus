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
- **The stop line** (2026-08-03, fifth wording). **The rule, and it is safety:** *the operator must
  never be unable to stop a transmission.* Mechanically, and this is the whole of it: **in every
  cockpit, at least one control that stops a transmission renders OUTSIDE every ⊞-removable pane.**
  Hide every id in that cockpit's vocabulary, singly and all at once, and those controls are still on
  screen and no more disabled than they were — no vocabulary id reaches them, so hiding one is
  unrepresentable rather than guarded. **The controls that hold it up:** Phone — PTT (dock), Stop TX
  + Tune (header); CW — Stop TX + Tune (header), Esc; Operate — TX On/Off, Tune, Stop TX, S&P (all in
  `.cockpit-qso`), Esc; RTTY — Stop TX + the TX-enable latch (header), the dock's Esc/Stop macro and
  the sequencer's Abort; SSTV — Stop (`.sstv-tx-bar`) + the TX-enable latch. APRS is a sixth cockpit
  with no vocabulary at all. The TopBar's TX cluster backstops none of them — App hides it in Operate
  and in Phone/CW/RTTY/SSTV/APRS — so each cockpit stands on its own. Each cockpit's sweep list is
  exactly this set, which is what makes the rule checkable in minutes.
  **Nothing else about a pane bears on whether it may be hidden.** A pane *may* host a stop control
  of its own, and it goes away with the pane: **two do** — Phone's `voiceKeyer` (■ Stop → `stopVoice`
  → `Engine::stop_voice`, which flushes the output ring and unkeys) and RTTY's `stream` (the "Auto on"
  toggle: off-click → `seq.abort()` + `Engine::rtty_stop()`, queue cleared and unkeyed). Those are
  **conveniences built on the guarantee, never what holds it up.** A pane *may* start a transmission:
  **six do** — the voice keyer, Operate's Tx messages (Tx6 = Call CQ → `startCq`) and its two decode
  panes and two rosters (double-click → `call_station_ctx`, which enables TX and keys the period);
  all hideable, correctly. A pane's hide *may* end something in flight: **one does** (the voice
  keyer), and that earns a note, not a refusal.
  **What is forbidden — four things, read off that census:** (1) giving a listed control an id, or
  moving one *into* a ⊞-removable pane, which is how it would get one; (2) gating one on a pane id
  without moving it (`disabled={!shown('dsp')}`) — mounted and dead is the same loss as gone;
  (3) shipping a cockpit whose *only* stop control is inside a removable pane (none does; RTTY is
  closest and survives on four outside `stream`); (4) adding a **pane-resident** stop control to a
  sweep's `stopControls`, which would make the sweep demand its pane be unhideable.
  **The practice, and it is courtesy, not safety:** if hiding a pane *ends* something in flight, its
  ⊞ entry should say so before the tick — a stop the operator did not ask for reads as a dropout.
  That is why the voice keyer's entry warns (its unmount aborts a message and discards a recording);
  a pane whose hide ends nothing must carry no warning. Nothing is admitted or refused on it.
  *Four earlier wordings were falsified, and the record is kept in `panelState.ts` and
  `cockpit-panes.css` because it is what stops a fifth proxy:* "a pane that can only start a
  transmission may be hidden" excluded the pane it was written to admit (the keyer has a Stop
  button); "a pane that *may be hidden* has a hide path that is itself a stop" bound every hideable
  pane, forbidding 23 of the 24 entries in `ALL_PANEL_VOCABULARIES`; "a pane that *can start* a
  transmission may be hidden only if its hide is a stop" was violated by shipped code — Operate's
  `txmsgs` starts a CQ, is hideable, and its hide stops nothing and says nothing; and "every control
  that *stops* a transmission has no id in any pane vocabulary" was falsified twice over by the
  keyer's own ■ Stop and by RTTY's Auto toggle, both stop controls living inside panes that *have*
  ids. All four named a property of a pane or of a control; the guarantee is a property of **the
  screen that remains**.
  Two guards, and neither is the rule alone: `panelState.test.ts` checks **names** across every
  vocabulary (`ALL_PANEL_VOCABULARIES`, itself checked against every vocabulary the module
  exports); `components/stop-line.test.tsx` checks **wiring** for Phone/CW/RTTY/SSTV — with every id
  in a cockpit's vocabulary removed, singly and all at once, every stop control **on that cockpit's
  list** must still be in the document, found by accessible name, and no more disabled than it was.
  Operate is swept in `OperateCockpit.structure.test.tsx`, and that sweep is **presence-only** (see
  the next bullet) — not the same check. A stop control gated on an id called `dsp` is caught only by
  the wiring sweeps; a dead `ptt` entry only by the name guard. Render each cockpit with **the props
  App gives it** — the TX-enable latch only exists when `onSetTxEnabled` is passed, and omitting it
  made the RTTY/SSTV sweeps blind to a control on both their lists.
- **What the stop-line guards do NOT prove.** Written down rather than chased with more guards:
  neither sweep can see a stop control that is **present, enabled and inert** (an
  `onClick={() => {}}` on Stop TX passed the whole suite — 2106 tests at the time); the name backstop
  is **exact-word** on whole normalised ids, so `txStop`/`pttRow`/`killTx` pass it — substring
  matching is not an option, it rejects `voiceKeyer` for containing `keyer`; the **practice
  note-pairing is computed for Phone only**, with no coverage test across vocabularies of the kind
  the name guard has (that costs courtesy, not the guarantee); **Operate's sweep is presence-only**
  (no baseline, no `disabled` comparison, no one-id-at-a-time pass) and is not the equivalent of the
  four-cockpit sweep; and that a *newly added* stop control reached its cockpit's sweep list is a
  human step.
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

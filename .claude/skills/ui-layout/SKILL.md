---
name: ui-layout
description: Use when building or modifying ANY Nexus UI — a new view, pane, dialog, pop-out, or responsive behavior — and when investigating a layout/sizing/clipping bug. Loads the pane-grid contract, the sizing decision tree, and the verification requirements from the 2026-07 layout overhaul.
---

# Nexus UI layout — the build procedure

The 2026-07 overhaul killed a five-week class of sizing bugs (unreachable controls, blank-space
hoarding, dead scrollbars). Those bugs came back repeatedly because each fix was a local patch
into a global cascade with no enforcement. This skill is the procedure that keeps the class dead.

The law: `CLAUDE.md` §"UI layout contract" + the header comment of `ui/src/cockpit-panes.css`.
The enforcement: `cockpit-panes.test.ts`, `cockpit-shells.test.ts`, `responsive-vocab.test.ts`
(they COMPUTE cascade winners and rendered structure). Forensic history + design rationale:
`resolution/assessment-2026-07-30/` (machine-local) and the memory `reference-cockpit-pane-grid`.

## 1. Route the surface first

| Building a… | Do this |
|---|---|
| Block inside a cockpit | Render through `CockpitPaneFrame` in the cockpit's `.cockpit-panes` region. Never a bare sibling of the shell. |
| New full view | Bounded shell with ONE scroll owner (either the view scrolls, or exactly one designated inner scroller does). Copy Operate/Connect/Stats patterns, not Phone-pre-overhaul. |
| Modal / dialog | `ui/Dialog` (portaled; its content auto-applies `--ui-zoom`, its box measures the real window — do not fight either half). Give any new modal a `--vh-eff` max-height + internal scroll. |
| Pop-out window | `DetachedPanel` branch with className `app detached …` (the `app` class carries zoom — its omission was a shipped bug). Min sizes live in `open_panel_window` (src-tauri). |
| TX-adjacent control | Default to the cockpit TX dock or header. THE STOP LINE is the rule (CLAUDE.md, `panelState.ts`): *the operator must never be unable to stop a transmission* — mechanically, in every cockpit at least one control that STOPS one renders outside every ⊞-removable pane, so those controls have no id in any vocabulary. A pane that merely SENDS may be hidden (six ship that way), and a pane may host a stop of its own (two do). Adding a stop control? Put it outside the panes and add it to that cockpit's census + sweep. |

## 2. The role question (ask it of every pane, every time)

**Can this content actually stretch to use surplus height?**
- NO (chip rows, control strips, fixed-height band strips, bounded echoes) → `fit="content"`.
- YES (transcripts, feeds, the log column whose recall history wants room) → fill + `weight`.

A grower pointed at fixed-height content is the app's most-recurred bug (the empty black box —
it shipped twice, once per abstraction level). Feeds put their comfortable minimum on their
content wrapper inside `.pane-body` (the sanctioned place), never on the frame.

## 3. Hard rules (CI-enforced — do not relitigate)

- Structural size lives ONLY in `cockpit-panes.css` (flat single-class selectors, fenced).
  A pane never sizes itself; the frame takes no className/style prop.
- Responsive behavior: `[data-viewport='xs|sm|md|lg|xl']` + `--vh-eff`/`--vw-eff` only.
  NEVER a size-based `@media` (zoom-blind: misfires at the window minimum, dead for
  pinned-zoom accessibility users). NEVER raw `vh/vw` inside `.app`. Portaled
  `.ui-dialog`/`.ui-tooltip` boxes are the one permanent raw-unit exception.
- An `overflow:hidden` ancestor may never contain hard-floored descendants without an
  interposed scroller. Grid growers are `minmax(0,1fr)`, never bare `1fr`. A floor that must
  exist under a bounded parent is written to yield: `min(Xem, share)`.
- Anything persisted that encodes size/position/scale/share is CLAMPED ON LOAD against the
  current window and monitors — not only at drag time (patterns: `Splitter.tsx`,
  `usePaneWidths.ts`, `capPinnedScale`, the band-map work-area check in src-tauri).
- Auto-following feeds use `usePinnedScroll` — never a bare `scrollTop = scrollHeight`
  (it makes scroll-back impossible during live copy). Programmatic `focus()` takes
  `{ preventScroll: true }` (+ `scrollIntoView({ block: 'nearest' })` for roving lists).
- Canvas sizing uses the `devicePixelContentBoxSize` pattern (`Waterfall.tsx` is the
  reference); a canvas in a measured box goes out-of-flow (`absolute; inset:0`); transient
  markers draw on a separate overlay canvas, never into a scrolling bitmap.
- Theme variables must exist in BOTH themes; sticky/pinned surfaces need an opaque background.

## 4. Tests you must ship with the change

- Extend the computing guards (`cockpit-panes.test.ts` / `cockpit-shells.test.ts` /
  `responsive-vocab.test.ts`) or add a structure test that RENDERS the component and asserts
  region/dock/roles. **Never add a regex-presence CSS test** — dead selectors pass those;
  that is how two dead fixes shipped pre-overhaul.
- Failing-first: watch the new guard go red before the fix/feature makes it green.
- Run gates with UNMASKED exit codes — `cmd | grep …` swallows a red suite (this bit us
  the same day the guard style was invented). `tsc -b` (not `--noEmit`) + full vitest.

## 5. Pre-commit review sweep

1. Mentally lay out at: 1200×750 (zoom 80), 1366×768, 1200×1390, 3440×1440, 900×600 (zoom 65),
   and pinned 175% — nothing unreachable, nothing hoarding blank space at any of them.
2. Any new/changed styles.css rule: recompute its cascade against the whole sheet — a later or
   more specific rule may silently win (the historical failure mode).
3. If restructuring: behavior preservation is a review dimension of its own (PTT/send handlers,
   panelState gating, keyed columns so tier flips never remount the log form or the keyer).
4. Keep-alive hosts (`[hidden]` display:none): guard ResizeObserver 0×0 first-fires; re-stamp
   on re-show.

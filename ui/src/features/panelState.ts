// Panel VISIBILITY record — which panels a surface renders docked, which are torn off
// into their own window, and which the operator deleted. Pure (no JSX) so it unit-tests
// without React, and modelled on features/paneLayout.ts: the same coerce/load/save
// shape, defaults fill, unknown-id drop, and try/catch on storage.
//
// Deliberately SEPARATE from paneLayout. Placement decides WHICH pane occupies a grid
// cell; visibility decides WHETHER it renders. Because coercePlacement never sees a
// removal it cannot resurrect a panel you deleted — the hazard is unrepresentable
// rather than guarded — and the Classic/Roster presets stop fighting the operator for
// the same reason.
//
// TX-safety — THE STOP LINE (2026-08-03 operator ruling). This is the FIFTH wording; the four
// it replaces are kept below, each with the shipped code that falsified it, because that record
// is what stops a sixth attempt at a proxy. Every one of the four named a property of a PANE or
// of a CONTROL; the guarantee is a property of THE SCREEN THAT REMAINS, and that is the change.
//
// THE RULE (safety, enforced):
//
//   THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION.
//
//   Mechanically, and this is the whole of it: IN EVERY COCKPIT, AT LEAST ONE CONTROL THAT
//   STOPS A TRANSMISSION RENDERS OUTSIDE EVERY ⊞-REMOVABLE PANE. Hide every id in that
//   cockpit's vocabulary — singly and all at once — and those controls are still in the
//   document and no more disabled than they were, because no vocabulary id can reach them.
//   Hiding one is UNREPRESENTABLE rather than guarded: no menu entry, no stored value and no
//   coercion rule reaches it, which is strictly stronger than a `removable: false` flag a bad
//   code path or a hand-edited blob could bypass.
//
//   THE CONTROLS THAT HOLD IT UP, re-verified entry by entry against shipped code 2026-08-03.
//   Each one names WHAT it stops, because "a control that stops a transmission" is not one
//   thing — `halt_tx` is universal and everything else has a scope — and because two entries
//   that read like stops turned out not to be:
//     · Phone  — PTT (the dock's .ph-ptt-row): release, or Lock-toggle-off, → setPtt(false) →
//                Engine::set_ptt, dropping the mic key it is holding, and only that.
//                · Stop TX (CockpitHeader) → haltTx → Engine::halt_tx — the universal kill:
//                slot/cw/rtty/sstv aborts armed, voice_tx dropped, every queue cleared, manual
//                and broker PTT released. · Tune (CockpitHeader) → Engine::set_tune(false),
//                which ends the tune carrier and only that. · the Space bar (window keyup) →
//                the same setPtt(false) as PTT-release, and ONLY while `lock` is off: both
//                Space handlers early-return in Lock (hands-free) mode.
//     · CW     — Stop TX (CockpitHeader) → stopCw + haltTx. · Tune → set_tune (the carrier
//                only). · Esc (window keydown) → the same abort() Stop TX calls.
//     · Operate— Stop TX (.op-btn.stop in the merged .cockpit-qso strip) → onHaltTx →
//                Engine::halt_tx. It is the ONLY control in this cockpit that cuts an over
//                already in flight. · Tune (same strip) → set_tune, the carrier only.
//                · Esc (window keydown, while the view is active) → the same halt.
//                TWO ENTRIES WERE REMOVED FROM THIS LIST 2026-08-03, both falsified by code:
//                  – TX On/Off is Engine::set_tx_enabled, which by the operator's own
//                    2026-07-31 ruling deliberately does NOT arm slot_tx_abort (engine.rs
//                    ~6826 and ~7106): "TX Off should disable TX for the next cycle, but allow
//                    any ongoing TX to complete." The button's own tooltip says exactly that.
//                    It stops the NEXT over, never the one on the air.
//                  – S&P is onSetMode('qso-monitor'): engine.rs ~5957 builds a fresh
//                    QsoStation::monitoring with running:false and clears tx_queue /
//                    broadcast_queue / own_tx. It arms no abort and does not touch tx_enabled.
//                    It ends the CQ RUN and drops what is queued behind the over still keying.
//     · RTTY   — Stop TX (CockpitHeader) → rttyStop + haltTx; carries no `disabled`, so it is
//                always operable. · the dock's Esc/Stop macro → the same pair,
//                `disabled={!sending}`, so it is live exactly while an over is on the air.
//                · the TX-enable latch (CockpitHeader) — a REAL stop here, unlike in Operate:
//                set_tx_enabled(false) clears rtty_queue and arms rtty_abort (engine.rs ~7120,
//                "a disarm must abort the over in flight AND drop the queue"), and
//                tempo-audio/service.rs ~3826 consumes it — FSK keyer cleared, output ring
//                flushed, rig.ptt(false) — whenever `now < rtty_busy_until`, i.e. an over is
//                actually keying. CockpitHeader draws it as a BUTTON only while
//                radio.transmitting is false; radio.transmitting is the SLOT-TX indicator
//                alone (Engine::set_transmitting is written by plan_beacon_tx, plan_tx,
//                commit_tx and halt_tx — the slot machinery and the universal kill, never the
//                RTTY path, which reports through rtty_sending instead), so the latch stays
//                a button through every RTTY over. · the auto-sequencer's Esc/Abort →
//                seq.abort() + Engine::rtty_stop(), rendered only inside
//                `{auto && seqState !== 'idle'}`.
//     · SSTV   — Stop (.sstv-tx-bar) → sstvStop, `disabled={!sending}`. · the TX-enable latch,
//                a real stop here for the same reason as RTTY: set_tx_enabled(false) drops
//                sstv_tx and arms sstv_abort (engine.rs ~7124), which service.rs ~4174 turns
//                into feed dropped + output flushed + unkey while an image is in flight.
//     · APRS   — a sixth cockpit with NO vocabulary at all: no ⊞ menu, nothing hideable, so
//                the rule holds there by construction. Say what that does and does not buy:
//                APRS renders NO stop control. Its TX On/Off is an arm latch —
//                set_tx_enabled does not clear aprs_tx_queue (only halt_tx does, engine.rs
//                ~6856) and arms no APRS abort; poll_aprs_tx merely HOLDS the queue while the
//                latch is down. Nothing on that screen cuts a beacon already keying.
//   The app-wide TopBar TX cluster is NOT a backstop for any of them: App hides it in Operate
//   (hideTxControls) and in Phone/CW/RTTY/SSTV/APRS (hideDigitalChrome). Every cockpit stands
//   on its own controls.
//
//   HOW THE SWEEPS LINE UP WITH THAT CENSUS. They do NOT match it one for one, and the claim
//   that they did — "each cockpit's sweep list is the same set, which is how this is checkable
//   in minutes" — was false for four of the five swept cockpits. What is true:
//     · Phone  — swept: PTT, Stop TX, Tune. Space is census-only: a window key handler has no
//                accessible name, and the sweep queries buttons BY name.
//     · CW     — swept: Stop TX, Tune. Esc is census-only, same reason.
//     · Operate— its guard's list is not a stop-control list at all. PROTECTED in
//                OperateCockpit.structure.test.tsx is the whole TX/sequencer surface of the
//                strip (Call CQ, S&P, TX On/Off, Tune, Stop TX, Hold Tx, TX auto, Skip Tx1),
//                asserted present inside .cockpit-qso with every id removed — the dock law,
//                which is a wider claim than this rule. Stop TX is the stop control in it;
//                Tune ends its own carrier; the rest are there for other reasons. Esc is
//                census-only.
//     · RTTY   — swept: Stop TX, the Esc/Stop macro, the TX-enable latch. The sequencer's
//                Abort is census-only: it renders only inside `{auto && seqState !== 'idle'}`
//                and the sweep's fixture is auto:false / seqState:'idle', so there is nothing
//                on screen to find.
//     · SSTV   — swept: Stop, the TX-enable latch. An exact match, and the only one.
//   A control that is KEYBOARD-ONLY or CONDITIONALLY RENDERED is outside both sweeps by
//   construction. What is checkable in minutes is not list equality: it is that every swept
//   list is a non-empty subset of its cockpit's census.
//
//   NOTHING ELSE ABOUT A PANE BEARS ON WHETHER IT MAY BE HIDDEN:
//     · A PANE MAY HOST A STOP CONTROL OF ITS OWN, and it goes away with the pane. TWO DO.
//       Phone's `voiceKeyer` hosts ■ Stop ("Abort playback (Esc)") → stopVoice →
//       Engine::stop_voice, whose flag makes the radio loop flush the output ring and unkey.
//       RTTY's `stream` hosts the "Auto on" toggle, whose off-click is rttySetAuto(false) →
//       seq.abort() + Engine::rtty_stop() (queue cleared, rtty_abort + slot_tx_abort → unkey)
//       — and `stream` is the ENTIRE RTTY vocabulary. Both are CONVENIENCES built on the
//       guarantee, never what holds it up, so hiding either pane is fine.
//     · A PANE MAY START A TRANSMISSION. SIX DO: Phone's voice keyer (F1–F6 → playVoiceMessage
//       keys the rig) and Operate's `txmsgs` (Tx6 = "Call CQ (Alt+6)" → startCq) plus its two
//       decode panes and two rosters (double-click → call_station_ctx, which ENABLES TX and
//       keys the current period). All hideable, correctly. Phone's and CW's `bandActivity` are
//       NOT senders, though they look like it — work_spot QSYs, splits and notes the call.
//     · A PANE'S HIDE MAY END SOMETHING IN FLIGHT. ONE DOES (the voice keyer). That earns a
//       note, not a refusal — see THE PRACTICE.
//
//   WHAT IS FORBIDDEN — exactly four things, all read off the census above:
//     1. Giving any control on a cockpit's list an id in a pane vocabulary. Equivalently:
//        MOVING one into a ⊞-removable pane, which is how it would acquire one.
//     2. Gating one on a pane id without moving it (`disabled={!shown('dsp')}`). Mounted and
//        dead is the same loss as gone — which is why the wiring sweep compares a
//        nothing-hidden BASELINE rather than `disabled === false`.
//     3. Shipping a cockpit whose ONLY stop control sits inside a ⊞-removable pane. None does.
//        RTTY is the closest — `stream` is its ENTIRE vocabulary — and it survives because
//        Stop TX, the Esc/Stop macro, the TX-enable latch and the sequencer's Abort all sit
//        outside it. Count what is LIVE, not what is listed: while an RTTY over is actually
//        keying OUTSIDE an auto sequence, THREE of those four are operable — Stop TX (never
//        disabled), the Esc/Stop macro (`disabled={!sending}`, so live exactly then) and the
//        latch (a button, because radio.transmitting is false) — and the sequencer's Abort is
//        not rendered at all.
//     4. Adding a PANE-RESIDENT stop control to a sweep's `stopControls`. That makes the sweep
//        demand its pane be unhideable — which is precisely how the first wording excluded the
//        pane it was written to admit. The keyer's ■ Stop and RTTY's Auto toggle stay off
//        those lists deliberately.
//
// THE PRACTICE (courtesy, not safety — and it must be read as courtesy):
//
//   If hiding a pane ENDS something already in flight, its ⊞ entry should say so before the
//   tick, because a stop the operator did not ask for reads as a dropout. That is why the
//   voice keyer's entry warns: its unmount aborts a message and discards a recording (the
//   second is destruction rather than safety, so it is named separately). A pane whose hide
//   ends nothing needs no warning — one the operator cannot act on teaches him to ignore
//   the next one. Computed in PhoneCockpit.keyerHide.test.tsx, which asks the WIRE which
//   hides stopped something and requires exactly those entries to carry a note. Courtesy is
//   worth computing; it is still not the safety rule, and nothing may be admitted or
//   refused on it.
//
// FOUR EARLIER WORDINGS, ALL FALSIFIED. Each was a PROXY for the guarantee rather than the
// guarantee, and a proxy has to be re-argued the moment something new turns up at the line:
//   1. "A pane that can only START a transmission may be hidden; anything that can STOP one
//      may never be." — EXCLUDED THE VOICE KEYER, the pane it was written to admit, because
//      the keyer hosts a ■ Stop button of its own. A pane's own Stop is a convenience; the
//      operator's last resort is the dock.
//   2. "A pane that MAY BE HIDDEN has a hide path that is itself a stop — unmounting it ends
//      what it started — and its ⊞ entry says so." — By its own words it bound EVERY
//      hideable pane, and only the voice keyer's hide is a stop, so it forbade 23 of the 24
//      entries in ALL_PANEL_VOCABULARIES: every pane that ships except the one it was
//      written for. The first defect, pointing the other way.
//   3. "A pane THAT CAN START A TRANSMISSION may be hidden only if its hide path is itself a
//      stop AND its ⊞ entry says so." — VIOLATED BY SHIPPED CODE. Operate's `txmsgs` hosts
//      Tx1–Tx6, and Tx6 is "Call CQ (Alt+6)" → `startCq`: it can start a transmission, it is
//      hideable, its hide stops nothing and says nothing, and it has shipped that way for
//      months. Operate's bandActivity/rxfreq/callRoster/stations are four more of the same.
//   4. "Every control that STOPS a transmission — PTT, Stop TX, Tune, the TX-enable latch,
//      abort — has NO id in any pane vocabulary." — FALSIFIED BY SHIPPED CODE TWICE, the
//      first time by wording 1's own counter-example. The voice keyer's ■ Stop → stopVoice →
//      Engine::stop_voice (flush the output ring, unkey) is a control that stops a
//      transmission, and it sits inside `voiceKeyer`, which HAS an id — so ticking that entry
//      hides a stop control. RTTY's `stream` hosts a second, unnoticed until this pass: the
//      "Auto on" toggle, off-click → rttySetAuto(false) → seq.abort() + Engine::rtty_stop()
//      (queue cleared, rtty_abort + slot_tx_abort → unkey), and `stream` is the entire RTTY
//      vocabulary. Read strictly, the wording forbids both of those entries; read loosely, as
//      "no id is NAMED for a stop control", it is just the name backstop — a guard, not the
//      rule.
//   THE DIAGNOSIS behind 1–3: "the hide is itself a stop" was never a safety requirement. It
//   is a property the voice keyer HAPPENS to have — its unmount cleanup calls stopVoice —
//   generalised into a rule. Hide Operate's Tx messages mid-CQ and the over continues; Stop TX
//   is still in the QSO strip, so the operator can still stop it.
//   THE DIAGNOSIS behind 4: it protected CONTROLS, one at a time, when the guarantee is about
//   THE OPERATOR HAVING ONE LEFT. A pane's own stop may go away with the pane; the ones
//   outside every pane may not. THE GUARANTEE HOLDS EITHER WAY — all it ever needed was that a
//   stop control still be REACHABLE with everything hidden, which is the rule above.
//
// WHAT HAS NO ID HERE, and therefore cannot be hidden: the dial, the band/mode pickers, the
// Rx/Tx offset spinners, the QSO strip's TX On / Tune / Stop TX / Hold Tx, Phone's PTT row,
// CW's F-key macros and send bar, RTTY's header Stop TX + TX latch and both dock aborts,
// SSTV's Send/Stop bar. Recounted 2026-08-16: 28 entries across five vocabularies (20
// distinct ids), of which six can start a transmission, TWO host a stop control of their own
// (voiceKeyer, stream) and exactly one — voiceKeyer — has a hide that stops anything. The
// four added that day are the spectrum strips (SCOPE_PANEL_ID) — Phone, CW, RTTY and SSTV
// each gained the entry Operate has had since 0.15.0 — and they move none of those three
// counts: a strip sends nothing, hosts no stop and ends nothing on its way out. Recount
// from ALL_PANEL_VOCABULARIES at the foot of this file; do not trust the numbers, they are
// here to be checked.
//
// ENFORCEMENT — two guards, and neither is the rule on its own:
//   · the NAME backstop (panelState.test.ts, driven by ALL_PANEL_VOCABULARIES at the foot
//     of this file) — no vocabulary may contain an id NAMED for a stop control. It reads
//     names, never wiring: a stop control gated on an id called `dsp` walks past it.
//   · the RENDERED sweep in components/stop-line.test.tsx (Phone/CW/RTTY/SSTV, with the
//     REAL CockpitHeader and the props App passes) — with EVERY id in that cockpit's
//     vocabulary removed, singly and all at once, every stop control ON THAT COCKPIT'S LIST
//     must still be in the document, found by its accessible name and no more disabled than
//     it was. The list is the OUTSIDE-EVERY-PANE set and only that: a pane-resident stop (the
//     keyer's ■ Stop, RTTY's Auto toggle) is deliberately absent, because listing one would
//     make the sweep demand its pane be unhideable — forbidden item 4 above. Operate is
//     swept in OperateCockpit.structure.test.tsx, and that one is PRESENCE-ONLY: every id
//     removed at once, no baseline, no `disabled` comparison, no one-id-at-a-time pass. It
//     is NOT the four-cockpit sweep's equivalent. Both read wiring, never names: a dead
//     `ptt` id that gates nothing walks past THEM.
//
// WHAT THE GUARDS DO NOT PROVE — the full list is in the cockpit-panes.css header and
// CLAUDE.md, and it is short enough to carry the headline here: NEITHER SWEEP CAN SEE A STOP
// CONTROL THAT IS PRESENT, ENABLED AND INERT (an `onClick={() => {}}` on Stop TX passes the
// whole suite); the name backstop is EXACT-WORD, so `txStop`/`pttRow`/`killTx` walk past it;
// the PRACTICE note-pairing is computed for Phone only; Operate's sweep is presence-only and
// is not the equivalent of the four-cockpit one; NO SWEEP CAN SEE A KEYBOARD-ONLY OR
// CONDITIONALLY RENDERED STOP (Phone's Space, CW's and Operate's Esc, RTTY's sequencer Abort
// are all census-only for that reason); and that a NEWLY ADDED stop control reached its
// cockpit's sweep list is a human step, named in each sweep.
import { useCallback, useMemo, useState } from 'react'
import { durableGet, durableSet } from './durableStore'
import { windowInstance } from './windowScope'

export type PanelState = 'docked' | 'popped' | 'removed'

const PANEL_STATES = ['docked', 'popped', 'removed'] as const

export interface PanelLayout<P extends string> {
  v: 1
  /** Absent ⇒ docked. Partial is deliberate: a panel added in a later release ships
   *  visible with no migration, and a removal is always an EXPLICIT stored value, so it
   *  can never be confused with "new". */
  state: Partial<Record<P, PanelState>>
  /** Flex/fr share within its region. Nothing writes it yet (seam resize is a later
   *  step); it rides in the record from commit one so that step needs no version bump. */
  share: Partial<Record<P, number>>
}

/** A view's panel vocabulary: its storage namespace plus the coercion whitelist. */
export interface PanelVocabulary<P extends string> {
  /** View id — the `<view>` in `nexus.panels.<view>.<instance>`. */
  readonly view: string
  /** Every panel this view can hide. The controls on this cockpit's stop-line census are
   *  deliberately absent — NOT "TX chrome", which is a wider and falsified claim: `voiceKeyer`
   *  transmits and `stream` hosts a stop control, and both are listed here on purpose. */
  readonly panelIds: readonly P[]
}

export function isPanelState(v: unknown): v is PanelState {
  return typeof v === 'string' && (PANEL_STATES as readonly string[]).includes(v)
}

/** Stock layout: nothing stored, so every panel is docked. */
export function emptyPanelLayout<P extends string>(): PanelLayout<P> {
  return { v: 1, state: {}, share: {} }
}

/** `nexus.panels.<view>.<instance>` — one record per SURFACE (see windowScope).
 *
 *  Built here rather than via `surfaceKey`, and deliberately: this key shipped in 0.15.0
 *  ALREADY suffixed on the main window (`nexus.panels.operate.main`), so for it the
 *  byte-identical-across-upgrade string is the SUFFIXED one. Every other per-surface key
 *  predates instances and must stay bare on `main`, which is the rule `surfaceKey`
 *  encodes. Routing this key through it would rename it and lose saved layouts. */
export function panelStorageKey(view: string, instance?: string): string {
  return `nexus.panels.${view}.${instance ?? windowInstance()}`
}

/**
 * A valid record from any input. Unknown panel ids and unknown state strings are
 * dropped, a junk blob coerces to the stock layout, and a share that isn't a finite
 * positive number is discarded — one that is gets CLAMPED into the writers' own range.
 * Mirrors coercePlacement's "missing → safe default".
 */
export function coercePanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  raw: unknown,
): PanelLayout<P> {
  const out = emptyPanelLayout<P>()
  if (!raw || typeof raw !== 'object') return out
  const obj = raw as { state?: unknown; share?: unknown }
  if (obj.state && typeof obj.state === 'object') {
    const src = obj.state as Record<string, unknown>
    for (const id of spec.panelIds) {
      const v = src[id]
      if (isPanelState(v)) out.state[id] = v
    }
  }
  if (obj.share && typeof obj.share === 'object') {
    const src = obj.share as Record<string, unknown>
    for (const id of spec.panelIds) {
      const v = src[id]
      // Clamp into [MIN_SHARE, 2 − MIN_SHARE] — the exact range the writers enforce
      // (setShare/setShares floor, seamShares two-pane cap). Load used to accept any
      // v > 0, so a hand-edited/foreign 1e-9 collapsed a pane to ~0 height on the one
      // path the setters cannot guard.
      if (typeof v === 'number' && Number.isFinite(v) && v > 0) {
        out.share[id] = Math.min(2 - MIN_SHARE, Math.max(MIN_SHARE, v))
      }
    }
  }
  return out
}

export function savePanelLayout<P extends string>(key: string, layout: PanelLayout<P>): void {
  try {
    // The MAIN window's layout is durable (it survives a reinstall); a detached panel's is
    // not. `durableSet` decides from the key itself — see `isMainWindowPanelLayout` — so this
    // call site does not have to know which surface it is on, and cannot get it wrong.
    durableSet(key, JSON.stringify(layout))
  } catch {
    /* full/unavailable — in-memory state still applies this session */
  }
}

/** The app-global pop-out flag this record replaces. Still written by the torn-off
 *  waterfall window as its open/close signal until Step 4's real window event lands. */
export const WATERFALL_DETACHED_KEY = 'nexus.waterfall.detached'
/** Marker so the bridge below runs exactly once, ever (migrateChaseDefault idiom). */
const WATERFALL_MIGRATED_KEY = 'nexus.panels.wfDetached.v1'

/**
 * One-time bridge: an operator whose waterfall was popped out when this record landed
 * keeps it popped out instead of getting a surprise docked strip. Guarded by a
 * persisted marker so a later re-dock can never be undone by a stale global flag —
 * after this the record is the only source of truth.
 */
function migrateWaterfallDetached<P extends string>(
  spec: PanelVocabulary<P>,
  key: string,
  layout: PanelLayout<P>,
): PanelLayout<P> {
  const wf = 'waterfall' as P
  if (!(spec.panelIds as readonly string[]).includes(wf)) return layout
  let popped = false
  try {
    if (localStorage.getItem(WATERFALL_MIGRATED_KEY)) return layout
    popped = localStorage.getItem(WATERFALL_DETACHED_KEY) === '1'
    localStorage.setItem(WATERFALL_MIGRATED_KEY, '1')
  } catch {
    return layout // storage blocked — leave the layout untouched
  }
  if (!popped || layout.state[wf]) return layout
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  state[wf] = 'popped'
  const next: PanelLayout<P> = { ...layout, state }
  savePanelLayout(key, next)
  return next
}

export function loadPanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): PanelLayout<P> {
  const key = panelStorageKey(spec.view, instance)
  let layout = emptyPanelLayout<P>()
  try {
    const raw = durableGet(key)
    if (raw != null) layout = coercePanelLayout(spec, JSON.parse(raw))
  } catch {
    /* malformed — fall through (matches loadPlacement) */
  }
  return migrateWaterfallDetached(spec, key, layout)
}

/**
 * Fresh main-window boot: a torn-off panel window never survives an app restart (only
 * the main window is restored), so a stored 'popped' is stale — re-dock it, or the
 * operator relaunches to a re-dock bar and no window to re-dock from. This is the
 * record-level version of the boot-clear the app-global flag already had. A 'removed'
 * panel is an explicit choice and is left exactly as it is.
 *
 * Call it through `redockAllStalePopouts` below rather than per-vocabulary — see there.
 */
export function redockStalePopouts<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): void {
  const layout = loadPanelLayout(spec, instance)
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  let changed = false
  for (const id of spec.panelIds) {
    if (state[id] === 'popped') {
      state[id] = 'docked'
      changed = true
    }
  }
  if (changed) savePanelLayout(panelStorageKey(spec.view, instance), { ...layout, state })
}

/** A pane can never be resized below this flex share — a seam drag clamps here so a pane
 *  stays grabbable and can't vanish. Removal (via the Panels menu) is the ONLY route to
 *  gone, which keeps the "a removed panel is unrepresentable-as-resurrectable" guarantee. */
export const MIN_SHARE = 0.15

/**
 * Split a region between two adjacent panes from the seam position, given as a fraction
 * (0..1) of the region from the top/left. Returns `[aboveShare, belowShare]` normalized to
 * sum to 2 — so the centre (0.5) is the stock `[1, 1]` — and clamped so neither pane drops
 * below `MIN_SHARE`. Pure + monotonic in `fraction`, which is why it can't oscillate: the
 * output depends only on the pointer position, never on the current size it is setting.
 */
export function seamShares(fraction: number): [number, number] {
  const f = Math.min(1, Math.max(0, fraction))
  // Clamp BOTH output shares to the MIN_SHARE floor (not the fraction) so the floor holds
  // exactly with no float-rounding slack; the pair still sums to ~2 (relative flex, so the
  // region stays full).
  const above = Math.max(MIN_SHARE, Math.min(2 - MIN_SHARE, 2 * f))
  const below = Math.max(MIN_SHARE, 2 - above)
  return [above, below]
}

export interface PanelLayoutApi<P extends string> {
  layout: PanelLayout<P>
  /** Absent ⇒ docked. */
  stateOf: (id: P) => PanelState
  setPanelState: (id: P, state: PanelState) => void
  /** Flex-grow share for a pane within its region (default 1). A sole surviving pane
   *  auto-fills because grow is relative; two co-resident panes split by their ratio. */
  shareOf: (id: P) => number
  /** Set one pane's flex share (clamped to at least MIN_SHARE). */
  setShare: (id: P, value: number) => void
  /** Set several panes' shares in ONE undoable step — a seam drag redistributes the two
   *  adjacent panes it sits between atomically (a single save + one undo entry). */
  setShares: (updates: Partial<Record<P, number>>) => void
  /** Restore the layout as it was before the last change (one level deep). */
  undo: () => void
  canUndo: boolean
  /** The ids the next `undo()` would REMOVE — the panes that call would unmount. Empty when
   *  there is nothing to undo, or when the undo only puts panes back.
   *
   *  It exists for THE PRACTICE half of THE STOP LINE: a hide that ENDS something in flight
   *  should say so BEFORE the act, and ⊞ Undo is a second button that reaches the same
   *  teardown as the tick (untick the keyer, tick it back, record, Undo — the take was
   *  binned with no warning). Courtesy, not safety: the operator's way to stop is in the
   *  dock either way. ⊞ Reset needs no such list — it applies `emptyPanelLayout()` and
   *  `stateOf` defaults to 'docked', so reset can only ever MOUNT a pane. */
  undoRemoves: readonly P[]
  /** Back to stock — every panel docked, every share reset. Undoable like any change. */
  reset: () => void
}

/**
 * The visibility record for one surface. MUST be owned by a host that outlives the
 * view it describes (App's `.operate-host` keep-alive, not the cockpit itself) — and
 * every change SAVES SYNCHRONOUSLY INSIDE THE STATE UPDATER, never from a useEffect,
 * which is the exact shape of this app's remount-state-loss bugs.
 */
export function usePanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): PanelLayoutApi<P> {
  const key = useMemo(() => panelStorageKey(spec.view, instance), [spec.view, instance])
  // Current + previous in ONE state so the undo snapshot is taken by the same updater
  // that saves — two useStates could not do that atomically.
  const [hist, setHist] = useState<{ cur: PanelLayout<P>; prev: PanelLayout<P> | null }>(() => ({
    cur: loadPanelLayout(spec, instance),
    prev: null,
  }))
  const apply = useCallback(
    (next: (cur: PanelLayout<P>) => PanelLayout<P>) =>
      setHist((h) => {
        const cur = next(h.cur)
        savePanelLayout(key, cur)
        return { cur, prev: h.cur }
      }),
    [key],
  )
  const stateOf = useCallback((id: P) => hist.cur.state[id] ?? 'docked', [hist.cur])
  const setPanelState = useCallback(
    (id: P, s: PanelState) =>
      apply((cur) => {
        const state: Partial<Record<P, PanelState>> = { ...cur.state }
        state[id] = s
        return { ...cur, state }
      }),
    [apply],
  )
  const shareOf = useCallback((id: P) => hist.cur.share[id] ?? 1, [hist.cur])
  const setShare = useCallback(
    (id: P, value: number) =>
      apply((cur) => {
        const share: Partial<Record<P, number>> = { ...cur.share }
        share[id] = Math.max(MIN_SHARE, value)
        return { ...cur, share }
      }),
    [apply],
  )
  const setShares = useCallback(
    (updates: Partial<Record<P, number>>) =>
      apply((cur) => {
        const share: Partial<Record<P, number>> = { ...cur.share }
        for (const [id, v] of Object.entries(updates)) {
          if (typeof v === 'number' && Number.isFinite(v)) {
            share[id as P] = Math.max(MIN_SHARE, v)
          }
        }
        return { ...cur, share }
      }),
    [apply],
  )
  const undo = useCallback(
    () =>
      setHist((h) => {
        if (!h.prev) return h
        savePanelLayout(key, h.prev)
        return { cur: h.prev, prev: null }
      }),
    [key],
  )
  const reset = useCallback(() => apply(() => emptyPanelLayout<P>()), [apply])
  // Which panes the pending undo would UNMOUNT: removed in the snapshot, present now.
  // Computed from the same history the undo restores, so it cannot describe a different
  // click than the one the button makes.
  const undoRemoves = useMemo(() => {
    const prev = hist.prev
    if (!prev) return [] as P[]
    return spec.panelIds.filter(
      (id) => (prev.state[id] ?? 'docked') === 'removed' && (hist.cur.state[id] ?? 'docked') !== 'removed',
    )
  }, [hist, spec.panelIds])
  return {
    layout: hist.cur,
    stateOf,
    setPanelState,
    shareOf,
    setShare,
    setShares,
    undo,
    canUndo: hist.prev != null,
    undoRemoves,
    reset,
  }
}

/** The Operate cockpit's removable panels — the first consumer's vocabulary.
 *
 *  FIVE OF THESE SEVEN ARE SENDERS, and they are the shipped counter-example that falsified
 *  the third wording of THE STOP LINE (see the header). `txmsgs` hosts Tx1–Tx6, Tx6 being
 *  "Call CQ (Alt+6)" → `startCq`; `bandActivity`, `rxfreq`, `callRoster` and `stations` all
 *  work a station on double-click, which arms TX and keys the current period. Every one is
 *  hideable, none of their hides stops anything, and none of them warns — correctly. Hide
 *  Tx messages mid-CQ and the over continues, with TX On / Tune / Stop TX / Hold Tx still
 *  in the merged QSO strip, where no ⊞ id can reach them. That is the rule holding. */
export const OPERATE_PANEL_IDS = [
  'waterfall',
  'bandActivity',
  'callRoster',
  'rxfreq',
  'txmsgs',
  'stations',
  'txmeters',
] as const
export type OperatePanelId = (typeof OPERATE_PANEL_IDS)[number]

export const OPERATE_PANELS: PanelVocabulary<OperatePanelId> = {
  view: 'operate',
  panelIds: OPERATE_PANEL_IDS,
}

/**
 * THE SPECTRUM STRIP'S ID, shared by the four cockpits that gained one on 2026-08-16
 * (operator: "add the waterfall in each window as an option to remove in the panels
 * section — leave it ON by default, give me the option to turn it off").
 *
 * ONE id across Phone/CW/RTTY/SSTV, because it is one thing wherever it renders: a
 * spectrum display the operator either wants or does not. What it HOLDS differs per
 * cockpit and never matters here — Operate's strip swaps FastGraph in for MSK144, Phone's
 * and CW's is the rig scope, RTTY's and SSTV's the band waterfall. The ⊞ label is the
 * cockpit's own word for it ("Scope" in Phone/CW, "Waterfall" in RTTY/SSTV); the id is not.
 *
 * WHY IT MAY BE HIDDEN, under THE STOP LINE and not around it: verified per cockpit
 * 2026-08-16, the strip hosts NO control that stops a transmission in any of them. Its
 * whole control surface is display — palette, floor/zero, resolution window, 2D/3D, pause
 * (a RENDER pause, not a TX one), scroll direction, Operate's pop-out — plus click-to-tune,
 * which moves the dial and keys nothing. Every stop control stays where it was: Stop TX and
 * Tune in the CockpitHeader, Phone's PTT in the dock, RTTY's Esc/Stop macro and the
 * TX-enable latch, SSTV's `.sstv-tx-bar` Stop. Hiding the strip is therefore
 * unrepresentable-as-harmful in the same way every other entry is, and the four-cockpit
 * wiring sweep in components/stop-line.test.tsx now hides this id too (it is driven off
 * these arrays), which is where that claim is COMPUTED rather than asserted here.
 *
 * IT IS NOT A SENDER AND ITS HIDE ENDS NOTHING, so it carries no ⊞ note — THE PRACTICE
 * requires a note of a hide that stops something in flight, and one the operator cannot act
 * on teaches him to ignore the next.
 *
 * OPERATE IS NOT ON THIS LIST and keeps its own `waterfall` id. That entry shipped in
 * 0.15.0, already hides the strip, and carries two things `scope` does not: the 'popped'
 * pop-out state and migrateWaterfallDetached, which keys on the literal id. Renaming it
 * would drop every stored preference and break the re-dock; giving RTTY/SSTV the name
 * `waterfall` instead would pull them into that one-time migration and make which cockpit
 * consumed its marker an ordering accident. Two names, one behaviour, and the reason is
 * storage compatibility rather than taste.
 */
const SCOPE_PANEL_ID = 'scope'

/** SSTV view's removable panels (Phase 3). The RX image stage and the TX bar
 *  (mode/Send/Stop/progress) and all header chrome are NOT panels — so SSTV's two stop
 *  controls, the TX bar's Stop and the header's TX-enable latch, are outside every
 *  ⊞-removable pane, which is THE STOP LINE. `txcompose` is the image chooser only; Send
 *  does not live in it.
 *
 *  `scope` here is THE BAND WATERFALL ONLY, and the distinction is structural rather than
 *  cosmetic: SSTV has one RX stage that is the band until a VIS lands and the PICTURE after
 *  it (SstvView's `inFlight` branch). The tick hides the band half; an arriving picture
 *  still takes the stage whatever the box says, because that is the decode itself and not a
 *  panel. See the gate at the render site. */
export const SSTV_PANEL_IDS = [SCOPE_PANEL_ID, 'txcompose', 'gallery'] as const
export type SstvPanelId = (typeof SSTV_PANEL_IDS)[number]

export const SSTV_PANELS: PanelVocabulary<SstvPanelId> = {
  view: 'sstv',
  panelIds: SSTV_PANEL_IDS,
}

/** Phone cockpit's removable panels (Phase 3) — the scope strip plus the panes under it.
 *  The whole CockpitHeader (mode/band/power/Tune/StopTX/split/CAT/mic/BW) and the PTT row
 *  are NOT panels: each hosts a way to STOP a transmission, which is THE RULE. The log strip
 *  is not one either, for a different reason — it holds a QSO the operator is part-way
 *  through typing, and a tick that drops unsaved work is its own kind of harm.
 *
 *  THE SCOPE ITSELF IS ONE, since 2026-08-16 (see SCOPE_PANEL_ID). An earlier version of
 *  this note grouped it with the header and the PTT row as "NOT panels: each hosts a way to
 *  STOP a transmission" — which was FALSE OF THE SCOPE and is the reason it is called out
 *  here rather than quietly moved. PhoneScope hosts a floor slider, a resolution/2D-3D/pause
 *  trio and click-to-tune; not one of them stops anything. The header and the PTT row are
 *  what that sentence was ever true of, and they are still outside the menu's reach.
 *  `rigscope` remains a SEPARATE and differently-scoped entry: it is the pane of controls
 *  that command the RADIO's own panadapter, and it lives in the region under the strip.
 *
 *  `voiceKeyer` IS one, since 2026-08-03. Nothing about it needed admitting: it transmits,
 *  and under the rule that has no bearing on whether it may be hidden — Stop TX and PTT are
 *  not panels, so the operator's last resort is out of the menu's reach whatever he ticks.
 *  Its own ■ Stop → stopVoice IS a control that stops a transmission, and it goes away with
 *  the pane: a convenience built on the guarantee, never what holds it up. The first wording
 *  turned on that button and thereby forbade the pane it was written to admit; the fourth
 *  tripped over it from the other side (see the header). RTTY's `stream` is the only other
 *  pane in the app like it.
 *
 *  Its entry DOES carry a warning, and that is THE PRACTICE, not the rule: VoiceKeyer's
 *  unmount cleanup calls stopVoice, so unticking it ends a message in flight, and the same
 *  cleanup discards a recording in progress. Both are named before the tick because a stop
 *  the operator did not ask for reads as a dropout. A hideable sender whose hide ends
 *  nothing — Operate's Tx messages, its decode panes, its rosters — warns about nothing,
 *  and must not. */
export const PHONE_PANEL_IDS = [
  SCOPE_PANEL_ID,
  'rigscope',
  'txmeters',
  'dsp',
  'dspLevels',
  'bandActivity',
  'voiceKeyer',
] as const
export type PhonePanelId = (typeof PHONE_PANEL_IDS)[number]

export const PHONE_PANELS: PanelVocabulary<PhonePanelId> = {
  view: 'phone',
  panelIds: PHONE_PANEL_IDS,
}

/** CW cockpit's removable panels (Phase 3) — the scope strip plus the panes under it. The
 *  whole CockpitHeader + keyer (WPM/backend/pitch/BW/Tune/StopTX), the F-key macros, the
 *  type-to-send input, and the log strip are NOT panels (TX-safety by construction).
 *
 *  THE SCOPE ITSELF IS ONE, since 2026-08-16 (see SCOPE_PANEL_ID) — the same correction as
 *  in the Phone twin: it was listed as unhideable "by TX-safety", and it hosts no stop
 *  control. `scopeCtl` is a different entry for a different thing (the controls that command
 *  the rig's own panadapter, in the region below), and both keep their own box. */
export const CW_PANEL_IDS = [
  SCOPE_PANEL_ID,
  'scopeCtl',
  'dsp',
  'txmeters',
  'rxdsp',
  'bandActivity',
  'copilot',
  'decode',
  'sent',
] as const
export type CwPanelId = (typeof CW_PANEL_IDS)[number]

export const CW_PANELS: PanelVocabulary<CwPanelId> = {
  view: 'cw',
  panelIds: CW_PANEL_IDS,
}

/** RTTY cockpit's removable panels (Phase 3). The CockpitHeader + StopTX + TX-arm, the
 *  auto-sequence QSO strip, the F-key macros, and the type-to-send compose are NOT panels.
 *
 *  THE WATERFALL IS ONE, since 2026-08-16 (see SCOPE_PANEL_ID) — it headed the unhideable
 *  list above until then, and nothing about it belonged there: its mark/space cursors and
 *  click-to-net are the decoder's tuning aid, not a stop. `stream` is unchanged and is still
 *  the pane that hosts a stop control of its own. With both ticked off this cockpit renders
 *  no ⊞-reachable content at all, and THE STOP LINE holds exactly as before: Stop TX and the
 *  TX-enable latch are in the header, the Esc/Stop macro and the sequencer's Abort in the
 *  dock, none of them with an id.
 *
 *  `stream` IS THE SECOND PANE IN THE APP THAT HOSTS A STOP CONTROL, and it is admitted for
 *  the same reason the voice keyer is. Its "Auto on" toggle, clicked off, is
 *  rttySetAuto(false) → seq.abort() + Engine::rtty_stop(): the queue is cleared and
 *  rtty_abort + slot_tx_abort unkey the rig. Hide `stream` and that toggle goes with it —
 *  allowed, because Stop TX and the TX-enable latch are in the header and the Esc/Stop macro
 *  and the sequencer's Abort are in the dock, none of them with an id. Its hide ENDS nothing
 *  (unmounting the pane calls no wire), so it correctly carries no ⊞ note. This is the pane
 *  that falsified the FOURTH wording a second time — see the header. */
export const RTTY_PANEL_IDS = [SCOPE_PANEL_ID, 'stream'] as const
export type RttyPanelId = (typeof RTTY_PANEL_IDS)[number]

export const RTTY_PANELS: PanelVocabulary<RttyPanelId> = {
  view: 'rtty',
  panelIds: RTTY_PANEL_IDS,
}

/** PSK cockpit's removable panels (Keyboard Modes; TX since Phase 2).
 *  RTTY's shape: `scope` is the band waterfall (see SCOPE_PANEL_ID), `stream` the decoded
 *  transcript. The CockpitHeader and the TX dock are not panels.
 *
 *  THE STOP LINE holds here the RTTY way (the Phase 1 "by construction" census is gone,
 *  as its own comment demanded when TX arrived). The census — every holder OUTSIDE every
 *  ⊞-removable pane, none with an id in this vocabulary: Stop TX (header, never
 *  disabled), the dock's Esc/Stop macro (`disabled={!(sending || latched)}`, live from
 *  the instant the continuous-TX latch goes up), the TX-enable latch (header arm —
 *  `set_tx_enabled(false)` arms `psk_abort`, so it is a real stop here exactly as in
 *  RTTY/SSTV), and Esc (keyboard-only ⇒ census-only, outside both sweeps by
 *  construction). Swept in stop-line.test.tsx's PSK case, rendered with App's props
 *  (`onSetTxEnabled` — the documented latch blindness). The continuous-TX ("TX") button
 *  is a SENDER, not a stop (RTTY's ruling: off lets what was typed finish keying) and
 *  must never be added to the sweep's stopControls. A fifth stop reaches a latched over
 *  with no control pressed: the engine's per-tick gate re-check (`poll_psk_stream`),
 *  which unkeys within one tick on a section change, a QSY out of privileges, a tune,
 *  or a radio handoff. */
export const PSK_PANEL_IDS = [SCOPE_PANEL_ID, 'stream'] as const
export type PskPanelId = (typeof PSK_PANEL_IDS)[number]

export const PSK_PANELS: PanelVocabulary<PskPanelId> = {
  view: 'psk',
  panelIds: PSK_PANEL_IDS,
}

/**
 * EVERY vocabulary in the app, so the stop-line name backstop cannot silently miss one.
 * It missed the Operate cockpit for the whole life of the rule — the guard listed the four
 * cockpits added in Phase 3 and never the first consumer — which meant `'ptt'` could be
 * added to OPERATE_PANEL_IDS and the full suite stayed green (proven by mutation,
 * 2026-08-03).
 *
 * A list can go stale the same way, so panelState.test.ts does not trust this one: it scans
 * the module's own exports for anything shaped like a PanelVocabulary and fails if this
 * array does not contain it. Adding a sixth cockpit therefore cannot ship unguarded — the
 * export itself is the trigger, not somebody remembering this line.
 */
export const ALL_PANEL_VOCABULARIES: readonly PanelVocabulary<string>[] = [
  OPERATE_PANELS,
  SSTV_PANELS,
  PHONE_PANELS,
  CW_PANELS,
  RTTY_PANELS,
  PSK_PANELS,
]

/**
 * The boot clear, over EVERY vocabulary. main.tsx calls exactly this on a fresh main-window
 * boot; it must not call `redockStalePopouts` per-record, because that is how the bug it
 * fixes got in — it ran on OPERATE_PANELS alone.
 *
 * Operate is the only cockpit that ever WRITES 'popped' into a visibility record —
 * OperateCockpit's waterfall pop-out holds the app's single `setPanelState(id, 'popped')`
 * call — and the only one with a re-dock bar. Pop-out AFFORDANCES are not rare, and an earlier
 * version of this note implied they were: DetachedPanel dispatches ten panel kinds (waterfall,
 * needed, memories, connect, dxped, sats, fieldday, operate, bandmapPhone, bandmapCw), and
 * Phone's and CW's band-map panes each carry one. Those call `openPanelWindow` directly and
 * never touch the record, which is exactly why they leave no stale 'popped' behind. In the
 * other four vocabularies a stored 'popped' renders the pane DOCKED while its ⊞ entry reads
 * "popped out", with no window to re-dock from and nothing that ever clears it, so the record
 * said it at every launch, forever. Driving the loop off ALL_PANEL_VOCABULARIES covers a sixth
 * cockpit by its being exported, the same way the stop-line name backstop does.
 */
export function redockAllStalePopouts(): void {
  for (const vocab of ALL_PANEL_VOCABULARIES) redockStalePopouts(vocab)
}

/**
 * The stop-line NAME backstop's word list — an id whose name means "this stops a
 * transmission" may not appear in any vocabulary. Compared after normalising to lower-case
 * letters only, so `stopTx`, `stop_tx` and `STOPTX` are all the same word.
 *
 * ITS LIMITS, stated plainly because the rule was once claimed to be "enforced by
 * computation" when this list was all there was:
 *   · it reads NAMES. It cannot see that a control is wired to an id, so an id called `dsp`
 *     gating the PTT row passes it. components/stop-line.test.tsx is what reads wiring.
 *   · the comparison is WHOLE-WORD, not substring, so `txStop`, `pttRow`, `stopButton` and
 *     `killTx` all pass. Substring matching is not the fix: it rejects `voiceKeyer` on the
 *     spot for containing `keyer` — the pane this whole round of rewrites was about, and one
 *     the rule has no quarrel with. This is a backstop against the obvious, and it is worth
 *     exactly that.
 * Keep both guards.
 */
export const STOP_CONTROL_WORDS = [
  'stop',
  'stoptx',
  'halt',
  'halttx',
  'abort',
  'ptt',
  'tune',
  'arm',
  'enabletx',
  'txenable',
  'txbar',
  'header',
  'keyer',
  'unkey',
  'kill',
  'panic',
] as const

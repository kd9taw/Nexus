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
//                Space handlers early-return in Lock (hands-free) mode. · Esc (App's window
//                keydown while Phone is shown, N71) → the same halt_tx as Stop TX; the voice
//                keyer's own Esc still stops its playback.
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
//                · Esc (App's window keydown while SSTV is shown, N71) → halt_tx, the halt the
//                TX strip's Stop TX sends.
//     · APRS   — a sixth cockpit with NO vocabulary at all: no ⊞ menu, nothing hideable, so
//                the rule holds there by construction. Its one stop control is the TX On/Off
//                latch, and from N71 Esc (App's window keydown while APRS is shown → halt_tx)
//                is a stop there too. set_tx_enabled(false) arms no APRS abort, but the radio loop's TX-off
//                cut (service.rs `tx_off_cut`) unkeys and flushes a beacon on the air once the
//                latch is down (measured 2026-09-30), and set_tx_enabled(false) also drops
//                what aprs_tx_queue still holds, with a notice (poll_aprs_tx drops it the same
//                way on a refusal; halt_tx clears it).
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
//     · Operate— swept (in both layouts, since ⊞ Arrange came to it on 2026-10-07): Stop TX,
//                Tune. Esc is census-only. A wider guard sits beside it: PROTECTED in
//                OperateCockpit.structure.test.tsx is the whole TX/sequencer surface of the
//                strip (Call CQ, S&P, TX On/Off, Tune, Stop TX, Hold Tx, TX auto, Skip Tx1),
//                held inside .cockpit-qso and no more disabled with every id removed, singly and
//                all at once — the dock law, a wider claim than this rule. Stop TX is the stop
//                control in it; Tune ends its own carrier; the rest are there for other reasons.
//     · RTTY   — swept: Stop TX, the Esc/Stop macro, the TX-enable latch. The sequencer's
//                Abort is census-only: it renders only inside `{auto && seqState !== 'idle'}`
//                and the sweep's fixture is auto:false / seqState:'idle', so there is nothing
//                on screen to find.
//     · SSTV   — swept: Stop, the TX-enable latch. An exact match apart from Esc (keyboard-only,
//                census-only), and the only one.
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
//       NOT senders, though they look like it — work_spot QSYs, splits and notes the call —
//       and nor are Phone's `spots` and `needed`, whose rows QSY through the Spots and Needed
//       views' own handlers.
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
// CW's F-key macros and send bar, RTTY's TX-strip Stop TX + TX latch and both dock aborts,
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
//   · the RENDERED sweep in components/stop-line.test.tsx (Phone/CW/RTTY/PSK/SSTV/JS8 and FT, with
//     the REAL header or QSO strip and the props App passes) — with EVERY id in that cockpit's
//     vocabulary removed, singly and all at once, every stop control ON THAT COCKPIT'S LIST
//     must still be in the document, found by its accessible name and no more disabled than
//     it was. The list is the OUTSIDE-EVERY-PANE set and only that: a pane-resident stop (the
//     keyer's ■ Stop, RTTY's Auto toggle) is deliberately absent, because listing one would
//     make the sweep demand its pane be unhideable — forbidden item 4 above. (Operate's was a
//     presence-only sweep of its own until 2026-10-07; it is one of these cases now, and the
//     cockpits that arrange are swept over their arrangements too.) It reads wiring, never
//     names: a dead `ptt` id that gates nothing walks past it.
//
// WHAT THE GUARDS DO NOT PROVE — the full list is in the cockpit-panes.css header and
// CLAUDE.md, and it is short enough to carry the headline here: NEITHER SWEEP CAN SEE A STOP
// CONTROL THAT IS PRESENT, ENABLED AND INERT (an `onClick={() => {}}` on Stop TX passes the
// whole suite); the name backstop is EXACT-WORD, so `txStop`/`pttRow`/`killTx` walk past it;
// the PRACTICE note-pairing is computed for Phone only; NO SWEEP CAN SEE A KEYBOARD-ONLY OR
// CONDITIONALLY RENDERED STOP (Phone's Space, CW's and Operate's Esc, RTTY's sequencer Abort
// are all census-only for that reason); and that a NEWLY ADDED stop control reached its
// cockpit's sweep list is a human step, named in each sweep.
import { useCallback, useMemo, useState } from 'react'
import { durableGet, durableSet } from './durableStore'
import { windowInstance } from './windowScope'
import { SLOT_IDS, type SlotId } from './connectConfig'
import { DASH_SLOT_IDS, type DashSlotId } from './dashRail'
import {
  coerceColumnOrder,
  coerceLeftSide,
  coercePlacement,
  moveArranged,
  placeAtFoot,
  stockColumn,
  stockPlacement,
  type ArrangeSpec,
  type PaneColumn,
  type PaneMove,
  type PanePlacement,
} from './panelPlace'
import { SHARED_PANES } from './sharedPanes'

export type PanelState = 'docked' | 'popped' | 'removed'

const PANEL_STATES = ['docked', 'popped', 'removed'] as const

export interface PanelLayout<P extends string> {
  /** 2 since the grid cockpits' column dividers (layout L2) put `cols` in the record. A v1 record
   *  reads as a v2 one with no columns stored; this build writes 2. */
  v: 1 | 2
  /** Absent ⇒ the vocabulary's DEFAULT (`panelStateIn`): docked, unless the vocabulary lists
   *  the id in `defaultRemoved`. Partial is deliberate: a panel added in a later release ships
   *  at its default with no migration — visible, or hidden when its vocabulary says so, which
   *  is how a pane can be added without changing anybody's screen on update — and an explicit
   *  choice is always a STORED value, so it can never be confused with "new". */
  state: Partial<Record<P, PanelState>>
  /** Flex/fr share within its region: what a divider between two panes (PaneSeam) wrote. */
  share: Partial<Record<P, number>>
  /** The COLUMN WIDTHS the operator set with the dividers between a grid cockpit's columns
   *  (Phone, CW, JS8 — layout L2). Absent, or any one of them absent, is the sheet's default.
   *
   *  An older build reading this record copies only `state` and `share` (its coercion never reads
   *  `cols`), so it opens on the default columns and loses nothing else. */
  cols?: PanelCols
  /** WHERE EACH PANE STANDS (layout L3, ⊞ Panels ▸ Arrange): its column and its place in it, for the
   *  panes the vocabulary's `arrange` lists (features/panelPlace). Absent is the stock grouping, and
   *  a pane it does not name stands at the end of its stock column. Same rule for older builds as
   *  `cols`: their coercion copies only `state` and `share`, so they open on the stock grouping. */
  place?: PanePlacement<P>
  /** The columns' order on screen ("swap columns"): kept by the record, rendered by no cockpit yet
   *  (see features/panelPlace). Absent is a | b | log. */
  colOrder?: PaneColumn[]
  /** THE LEFT SIDE (2026-10-03, ⊞ Panels ▸ Arrange's fourth place; Phone): the panes the operator put
   *  there, top to bottom — only those the vocabulary's `arrange.leftSide` lists. Each keeps its
   *  `place`, where it stands whenever the side does not show (a narrower window), so this is never
   *  rewritten by a window's width. Absent is nothing there. Older builds' coercion never reads it:
   *  they open those panes in their columns and lose nothing else. */
  leftSide?: P[]
  /** A CONNECT BOX'S OWN TEXT SIZE (⋯ ▸ A− / A+), per slot: a factor on the app's Text size,
   *  BOX_SCALE_MIN–BOX_SCALE_MAX (the section at the foot of this file). Absent, or 1, is the app's
   *  size — so an older record, and every cockpit's (none writes one), reads exactly as before. An
   *  older build's coercion copies none of it: every box opens at the app's size, nothing else lost. */
  scale?: Partial<Record<P, number>>
  /** WHAT EACH BOX SHOWS (2026-10-07, ⊞ Panels ▸ Arrange's "+ Add a box"): the shared list's entry
   *  (features/sharedPanes `id`) for each of the vocabulary's boxes. Whether a box is on screen is its
   *  `state` (every box ships hidden), where it stands its `place` / `leftSide`, like any pane. Kept
   *  consistent with the screen on load and after every change (`boxEntries`, THE BOXES at the foot of
   *  this file). An older build's coercion never reads it, and every box stays hidden there. */
  boxes?: Partial<Record<P, string>>
  /** WHERE EACH PANE STANDS IN EACH LAYOUT, for a vocabulary arranged per layout (`arrangeBy`: FT's
   *  Classic and Roster, 2026-10-07): that layout's placement, by the layout's name, as `place` is for a
   *  cockpit with one layout. Absent, or a layout absent, is that layout's stock arrangement. An older
   *  build's coercion never reads it, so FT opens on its stock columns there and loses nothing else. */
  places?: Partial<Record<string, PanePlacement<P>>>
  /** THE OTHER LAYOUT'S PANES ADDED TO A LAYOUT (FT's "Every FT pane in both", 2026-10-07): by the
   *  layout's name, the panes of its ArrangeSpec `extra` the operator added there (the Call Roster to
   *  Classic, the Tx messages or Stations to Roster). Such a pane is on screen in that layout while it is
   *  listed here and its `state` is not 'removed', so a hide in either layout hides it, as every FT pane's
   *  does, and taking it out of a layout leaves the other layout alone (`extrasIn`). Absent is none, which
   *  is each layout as it shipped; Reset clears it. An older build's coercion never reads it, so those
   *  panes stay where each layout always drew them there and nothing else is lost. */
  extras?: Partial<Record<string, P[]>>
}

/** A grid cockpit's column widths, as its column dividers write them (PanelLayout.cols):
 *    · `a`, `b` — the two feed columns' fr shares at three columns: the divider between them
 *      splits the pair the way a divider between two panes does (seamShares, summing to 2);
 *    · `log` — the log column's width in CSS px at two and three columns (the width divider on its
 *      left edge). The sheet caps it at half the region and floors it at 24em, so a width stored on
 *      a wide window is clamped by the layout itself wherever it is read (features/paneColumns);
 *    · `leftSide` — the left side's width in CSS px (the divider on its right edge). The same
 *      discipline: the sheet clamps it into its em floor … share of the row on load and on every
 *      resize, and the stored preference is never rewritten (features/paneColumns). */
export interface PanelCols {
  a?: number
  b?: number
  log?: number
  leftSide?: number
}
export type PanelColId = keyof PanelCols

/** A share, clamped into the writers' range [MIN_SHARE, 2 − MIN_SHARE]. */
const clampShare = (v: number) => Math.min(2 - MIN_SHARE, Math.max(MIN_SHARE, v))

/** One stored column value, or null for a value no writer could have produced. */
function colValue(id: PanelColId, v: unknown): number | null {
  if (typeof v !== 'number' || !Number.isFinite(v) || v <= 0) return null
  return id === 'log' || id === 'leftSide' ? Math.round(v) : clampShare(v)
}

/** A view's panel vocabulary: its storage namespace plus the coercion whitelist. */
export interface PanelVocabulary<P extends string> {
  /** View id — the `<view>` in `nexus.panels.<view>.<instance>`. */
  readonly view: string
  /** Every panel this view can hide. The controls on this cockpit's stop-line census are
   *  deliberately absent — NOT "TX chrome", which is a wider and falsified claim: `voiceKeyer`
   *  transmits and `stream` hosts a stop control, and both are listed here on purpose. */
  readonly panelIds: readonly P[]
  /** The panels this view SHIPS HIDDEN: with nothing stored for one, it reads 'removed' rather
   *  than 'docked', so the operator adds it from ⊞ Panels and nobody's screen changes on the
   *  update that introduces it (Phone's Spots and Needed, #345). A tick stores 'docked' like any
   *  other choice, and ⊞ Reset — which applies the empty record — hides it again.
   *
   *  ⚠️ Read an absent entry ONLY through `panelStateIn`, never as `state[id] ?? 'docked'`: the
   *  second spelling is how a default-hidden pane would appear on every screen. A default-hidden
   *  pane must also be one whose hide ENDS nothing, because Reset hides it with no note (THE
   *  PRACTICE, in the header). */
  readonly defaultRemoved?: readonly P[]
  /** The panes ⊞ Panels ▸ Arrange may move, their stock columns and the pinned ones (layout L3).
   *  Only a vocabulary with this may carry a `place` in its record. */
  readonly arrange?: ArrangeSpec<P>
  /** A cockpit with more than one stock layout arranges each on its own (FT's Classic and Roster, the
   *  operator's "Two saved arrangements", 2026-10-07): one ArrangeSpec per layout, by the layout's name,
   *  and the record keeps one placement per layout (`places`). Such a vocabulary has no `arrange`, and
   *  every layout lists the same boxes. */
  readonly arrangeBy?: Readonly<Record<string, ArrangeSpec<P>>>
  /** The cockpit's own panes that ARE an entry of the shared list (Phone's and CW's Spots and Needed
   *  boards), by that entry's id: while one is on screen, no box shows the same board (once per
   *  screen, 2026-10-07). */
  readonly sharedAs?: Partial<Record<P, string>>
}

export function isPanelState(v: unknown): v is PanelState {
  return typeof v === 'string' && (PANEL_STATES as readonly string[]).includes(v)
}

/** Stock layout: nothing stored, so every panel is at its vocabulary's default. */
export function emptyPanelLayout<P extends string>(): PanelLayout<P> {
  return { v: 2, state: {}, share: {} }
}

/** A panel's state in a record: what is stored for it, else its vocabulary's default — docked,
 *  or removed for an id in `defaultRemoved`. The ONE reading of an absent entry, so the menu
 *  tick, the pane, Undo's warning and Reset cannot disagree about a pane nobody has touched. */
export function panelStateIn<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  id: P,
): PanelState {
  return layout.state[id] ?? (spec.defaultRemoved?.includes(id) ? 'removed' : 'docked')
}

/** The ArrangeSpec a layout of the vocabulary is arranged by: its own (`arrange`), or, for a vocabulary
 *  arranged per layout, the named layout's (`arrangeBy`). Undefined where it arranges nothing. */
export function arrangeSpecOf<P extends string>(spec: PanelVocabulary<P>, layoutName?: string): ArrangeSpec<P> | undefined {
  return spec.arrangeBy ? (layoutName != null ? spec.arrangeBy[layoutName] : undefined) : spec.arrange
}

/** Where a layout's panes stand: the record's `place`, or the named layout's (`places`). */
export function placeOf<P extends string>(spec: PanelVocabulary<P>, layout: PanelLayout<P>, layoutName?: string): PanePlacement<P> | undefined {
  return spec.arrangeBy ? (layoutName != null ? layout.places?.[layoutName] : undefined) : layout.place
}

/** The record with a layout's placement replaced (`placeOf`'s writer); undefined clears it. */
function withPlace<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  layoutName: string | undefined,
  place: PanePlacement<P> | undefined,
): PanelLayout<P> {
  if (!spec.arrangeBy) {
    const { place: _was, ...rest } = layout
    return place ? { ...rest, place } : rest
  }
  if (layoutName == null) return layout
  const places: Partial<Record<string, PanePlacement<P>>> = { ...layout.places }
  if (place) places[layoutName] = place
  else delete places[layoutName]
  const { places: _was, ...rest } = layout
  return Object.keys(places).length > 0 ? { ...rest, places } : rest
}

/** The vocabulary's boxes, however it arranges: its ArrangeSpec's, or the first layout's for one arranged
 *  per layout (every layout lists the same boxes). */
export function boxIdsOf<P extends string>(spec: PanelVocabulary<P>): readonly P[] {
  return spec.arrange?.boxes ?? (spec.arrangeBy ? Object.values(spec.arrangeBy)[0]?.boxes : undefined) ?? []
}

/** A pane's state in a layout, for the layout on screen: a pane the layout lists in its ArrangeSpec
 *  `extra` is 'removed' there until the operator adds it to that layout (the record's `extras`), and then
 *  reads as every pane does: `stateOf`, the record's own reading (`panelStateIn`) unless a host gives its
 *  own, as for `boxEntries`. */
export function extrasIn<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  layoutName: string,
  id: P,
  stateOf: (id: P) => PanelState = (id) => panelStateIn(spec, layout, id),
): PanelState {
  const extra = arrangeSpecOf(spec, layoutName)?.extra
  if (extra?.includes(id) && !layout.extras?.[layoutName]?.includes(id)) return 'removed'
  return stateOf(id)
}

/**
 * The record with one of a layout's `extra` panes added to it (`on`) or taken out. Added, it is shown,
 * which a hide in the other layout may have cleared, and stands where the layout's placement puts it:
 * where it was, or at the foot of its column in the layout's stock arrangement, which is then written,
 * so the layout keeps its columns until Reset (a box's rule). Taken out, only that layout loses it.
 * Null for a pane the layout does not list in `extra`, and for a change that changes nothing.
 */
export function withExtra<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  layoutName: string,
  id: P,
  on: boolean,
): PanelLayout<P> | null {
  const arrange = arrangeSpecOf(spec, layoutName)
  if (!arrange?.extra?.includes(id)) return null
  const had = layout.extras?.[layoutName] ?? []
  if (on === had.includes(id) && (!on || panelStateIn(spec, layout, id) !== 'removed')) return null
  const extras: Partial<Record<string, P[]>> = { ...layout.extras }
  const ids = on ? [...had.filter((x) => x !== id), id] : had.filter((x) => x !== id)
  if (ids.length > 0) extras[layoutName] = ids
  else delete extras[layoutName]
  const { extras: _was, ...rest } = layout
  let next: PanelLayout<P> = Object.keys(extras).length > 0 ? { ...rest, extras } : rest
  if (on) {
    const state: Partial<Record<P, PanelState>> = { ...next.state }
    if (panelStateIn(spec, next, id) === 'removed') state[id] = 'docked'
    next = { ...next, v: 2, state }
    if (!placeOf(spec, next, layoutName)) next = withPlace(spec, next, layoutName, stockPlacement(arrange))
  }
  return next
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
 * Column widths the same way (a v1 record has none, so it loads with the default columns).
 * Mirrors coercePlacement's "missing → safe default".
 */
export function coercePanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  raw: unknown,
): PanelLayout<P> {
  const out = emptyPanelLayout<P>()
  if (!raw || typeof raw !== 'object') return out
  const obj = raw as { state?: unknown; share?: unknown; cols?: unknown; place?: unknown; colOrder?: unknown; leftSide?: unknown; scale?: unknown; boxes?: unknown; places?: unknown; extras?: unknown }
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
  if (obj.cols && typeof obj.cols === 'object') {
    const src = obj.cols as Record<string, unknown>
    const cols: PanelCols = {}
    for (const id of ['a', 'b', 'log', 'leftSide'] as const) {
      const v = colValue(id, src[id])
      if (v != null) cols[id] = v
    }
    if (Object.keys(cols).length > 0) out.cols = cols
  }
  // Placement (layout L3) only where the vocabulary arranges: an id outside its ArrangeSpec is
  // dropped, a pinned pane is held in its column, the orders are re-numbered (features/panelPlace).
  if (spec.arrange) {
    const place = coercePlacement(spec.arrange, obj.place)
    if (place) out.place = place
    const colOrder = coerceColumnOrder(obj.colOrder)
    if (colOrder) out.colOrder = colOrder
    // The left side only where the vocabulary has one, and only its listed panes (panelPlace).
    const leftSide = coerceLeftSide(spec.arrange, obj.leftSide)
    if (leftSide) out.leftSide = leftSide
  }
  // A placement per layout where the vocabulary arranges per layout: each by that layout's spec, and
  // a layout the vocabulary does not have is dropped.
  if (spec.arrangeBy && obj.places && typeof obj.places === 'object') {
    const src = obj.places as Record<string, unknown>
    const places: Partial<Record<string, PanePlacement<P>>> = {}
    for (const [name, arrange] of Object.entries(spec.arrangeBy)) {
      const place = coercePlacement(arrange, src[name])
      if (place) places[name] = place
    }
    if (Object.keys(places).length > 0) out.places = places
  }
  // The other layout's panes added to a layout: by that layout's `extra` only, each once.
  if (spec.arrangeBy && obj.extras && typeof obj.extras === 'object') {
    const src = obj.extras as Record<string, unknown>
    const extras: Partial<Record<string, P[]>> = {}
    for (const [name, arrange] of Object.entries(spec.arrangeBy)) {
      const allowed: readonly string[] = arrange.extra ?? []
      const v = src[name]
      if (!Array.isArray(v)) continue
      const ids = [...new Set(v)].filter((id): id is P => typeof id === 'string' && allowed.includes(id))
      if (ids.length > 0) extras[name] = ids
    }
    if (Object.keys(extras).length > 0) out.extras = extras
  }
  // A box's text size (Connect's A− / A+), clamped on read into the range the menu writes; a factor
  // of 1 is no entry at all, and junk is dropped rather than guessed at.
  if (obj.scale && typeof obj.scale === 'object') {
    const src = obj.scale as Record<string, unknown>
    const scale: Partial<Record<P, number>> = {}
    for (const id of spec.panelIds) {
      const v = boxScaleValue(src[id])
      if (v != null) scale[id] = v
    }
    if (Object.keys(scale).length > 0) out.scale = scale
  }
  // What each box shows: only the vocabulary's boxes and the shared list's entries, then made
  // consistent with the screen (a duplicate repaired, a box on screen with nothing given an entry).
  const boxIds = boxIdsOf(spec)
  if (boxIds.length > 0 && obj.boxes && typeof obj.boxes === 'object') {
    const src = obj.boxes as Record<string, unknown>
    const boxes: Partial<Record<P, string>> = {}
    for (const b of boxIds) {
      const v = src[b]
      if (typeof v === 'string' && SHARED_IDS.has(v)) boxes[b] = v
    }
    if (Object.keys(boxes).length > 0) out.boxes = boxes
  }
  return withScreenBoxes(spec, out)
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

/**
 * `inherit` names ANOTHER instance whose record this surface reads while it has none of its
 * own — `surfaceGet`'s contract, for the panel record. A torn-off Connect opens on the main
 * window's layout instead of first-run defaults; its first change writes its own key and the
 * two diverge from there. Omitted ⇒ no inheritance, which is every cockpit's behaviour.
 */
export function loadPanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
  inherit?: string,
): PanelLayout<P> {
  const key = panelStorageKey(spec.view, instance)
  let layout = emptyPanelLayout<P>()
  try {
    let raw = durableGet(key)
    if (raw == null && inherit != null && inherit !== (instance ?? windowInstance())) {
      raw = durableGet(panelStorageKey(spec.view, inherit))
    }
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

/** Where a ⊞ Arrange move happens, beyond the pane and the way (PanelLayoutApi `movePane`): the layout,
 *  for a vocabulary arranged per layout, and the columns' order on this window where it is not their
 *  stock one (FT's side rail on the left). */
export interface PaneMoveAt {
  layout?: string
  order?: readonly PaneColumn[]
}

export interface PanelLayoutApi<P extends string> {
  layout: PanelLayout<P>
  /** Absent ⇒ the vocabulary's default (`panelStateIn`): docked, or removed for a pane the
   *  vocabulary ships hidden. */
  stateOf: (id: P) => PanelState
  setPanelState: (id: P, state: PanelState) => void
  /** Flex-grow share for a pane within its region (default 1). A sole surviving pane
   *  auto-fills because grow is relative; two co-resident panes split by their ratio. */
  shareOf: (id: P) => number
  /** Set one pane's flex share (clamped to at least MIN_SHARE). */
  setShare: (id: P, value: number) => void
  /** Set several panes' shares in ONE undoable step — a seam drag redistributes the two
   *  adjacent panes it sits between atomically (a single save + one undo entry). A `null`
   *  CLEARS a pane's share, so it goes back to its stock proportion: the sheet's default,
   *  which the record never holds (Operate's Band Activity is 1.6 : 1) — a divider's reset. */
  setShares: (updates: Partial<Record<P, number | null>>) => void
  /** Set a grid cockpit's column widths in ONE undoable step — a column divider's release, key
   *  or reset (components/panes/RegionColumnSeams). `a`/`b` are fr shares, `log` CSS px; a `null`
   *  clears one back to the sheet's default. Optional: only the cockpits with column dividers
   *  (Phone, CW, JS8) read it, and a host without them never needs it. usePanelLayout always
   *  provides it. */
  setCols?: (updates: Partial<Record<PanelColId, number | null>>) => void
  /** Move one pane in the three-column placement (⊞ Panels ▸ Arrange, layout L3): up or down in its
   *  column past the panes `shown` says are on screen, or into the column beside it. ONE undoable
   *  step; a move that would change nothing (features/panelPlace `moveArranged` → null) is no step at
   *  all, so it cannot spend the one Undo. Optional: only a vocabulary with `arrange` has a
   *  placement. `sideShows` (the LEFT SIDE, 2026-10-03): whether the cockpit's left side is on screen
   *  on this window — then ◀ ▶ and ▲ ▼ reach it as well (panelPlace `moveArranged`); absent, false.
   *  `at` (FT, 2026-10-07): the layout the move is in, for a vocabulary arranged per layout, and the
   *  columns' order on this window where it is not their stock one. */
  movePane?: (id: P, move: PaneMove, shown: (id: P) => boolean, sideShows?: boolean, at?: PaneMoveAt) => void
  /** ⊞ Arrange's "+ Add a box" (2026-10-07): the first hidden box, on screen at the foot of `area` (a
   *  column, or the left side), showing the first entry of the shared list not on screen (`addBoxTo`).
   *  ONE undoable step, and none at all when every box is on screen. Optional: only a vocabulary with
   *  boxes has it. `layout`: the layout it is added in, for a vocabulary arranged per layout. */
  addBox?: (area: PaneColumn | 'side', layout?: string) => void
  /** A box's picker: show `entry` in `box` (`showInBox`) — an entry on screen moves here. ONE undoable
   *  step. Optional, as `addBox`. */
  setBox?: (box: P, entry: string) => void
  /** One of a layout's `extra` panes added to that layout or taken out of it (`withExtra`): the ⊞ tick
   *  and the ✕ of the Call Roster in Classic, and of the Tx messages or Stations in Roster. ONE undoable
   *  step, and none at all when it changes nothing. Optional: only a vocabulary arranged per layout. */
  setExtra?: (layoutName: string, id: P, on: boolean) => void
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
   *  dock either way. ⊞ Reset needs no such list, though it CAN hide a pane now: it applies
   *  `emptyPanelLayout()`, so every pane goes to its default, and the only panes whose default
   *  is 'removed' are the ones a vocabulary ships hidden — which must be panes whose hide ends
   *  nothing (see `defaultRemoved`). So Reset can only ever MOUNT a pane that ends something. */
  undoRemoves: readonly P[]
  /** Back to stock — every panel at its default (docked, or hidden for a pane the vocabulary
   *  ships hidden), every share and column width reset. Undoable like any change. */
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
  /** Read this instance's record while this surface has none of its own (loadPanelLayout). */
  inherit?: string,
): PanelLayoutApi<P> & {
  /** Replace the whole record in ONE undoable step — a layout preset (Connect's ⊞ Layout), whose
   *  visibility and splits must come back with a single Undo, as Reset's do. Coerced like a load,
   *  so an id outside the vocabulary cannot be written. Off PanelLayoutApi on purpose: cockpits
   *  receive that as a prop and have no presets to apply. */
  setLayout: (next: PanelLayout<P>) => void
  /** A Connect box's text size (⋯ ▸ A− / A+): 1 unless the record holds one. Off PanelLayoutApi for
   *  setLayout's reason — only Connect's boxes have one. */
  scaleOf: (id: P) => number
  /** Set a box's text size, clamped into BOX_SCALE_MIN–BOX_SCALE_MAX; 1 clears the entry. ONE
   *  undoable step, like a split. */
  setScale: (id: P, value: number) => void
} {
  const key = useMemo(() => panelStorageKey(spec.view, instance), [spec.view, instance])
  // Current + previous in ONE state so the undo snapshot is taken by the same updater
  // that saves — two useStates could not do that atomically.
  const [hist, setHist] = useState<{ cur: PanelLayout<P>; prev: PanelLayout<P> | null }>(() => ({
    cur: loadPanelLayout(spec, instance, inherit),
    prev: null,
  }))
  const apply = useCallback(
    (next: (cur: PanelLayout<P>) => PanelLayout<P>) =>
      setHist((h) => {
        // Every change keeps what the boxes show consistent with the screen: ticking Phone's own Spots
        // while a box shows the Spots board gives that box another entry, as a load would.
        const cur = withScreenBoxes(spec, next(h.cur))
        savePanelLayout(key, cur)
        return { cur, prev: h.cur }
      }),
    [key, spec],
  )
  const stateOf = useCallback((id: P) => panelStateIn(spec, hist.cur, id), [spec, hist.cur])
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
    (updates: Partial<Record<P, number | null>>) =>
      apply((cur) => {
        const share: Partial<Record<P, number>> = { ...cur.share }
        for (const [id, v] of Object.entries(updates)) {
          if (v === null) {
            delete share[id as P]
          } else if (typeof v === 'number' && Number.isFinite(v)) {
            share[id as P] = Math.max(MIN_SHARE, v)
          }
        }
        return { ...cur, share }
      }),
    [apply],
  )
  const setCols = useCallback(
    (updates: Partial<Record<PanelColId, number | null>>) =>
      apply((cur) => {
        const cols: PanelCols = { ...cur.cols }
        for (const [id, v] of Object.entries(updates) as [PanelColId, number | null | undefined][]) {
          if (v === null) {
            delete cols[id]
          } else {
            const c = colValue(id, v)
            if (c != null) cols[id] = c
          }
        }
        // Everything else in the record rides along (a placement, a column order); only `cols` changes.
        const { cols: _was, ...rest } = cur
        const next: PanelLayout<P> = { ...rest, v: 2 }
        if (Object.keys(cols).length > 0) next.cols = cols
        return next
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
  const movePane = useCallback(
    (id: P, move: PaneMove, shown: (id: P) => boolean, sideShows = false, at?: PaneMoveAt) =>
      setHist((h) => {
        const arrange = arrangeSpecOf(spec, at?.layout)
        if (!arrange) return h
        // ◀ ▶ go to the neighbouring column ON SCREEN: the stock order, unless the cockpit says its
        // columns stand in another on this window (`at.order`). No cockpit renders a stored column
        // order yet (features/panelPlace).
        const arr = { place: placeOf(spec, h.cur, at?.layout), leftSide: h.cur.leftSide }
        const next = moveArranged(arrange, arr, id, move, shown, sideShows, at?.order)
        if (!next) return h
        // Everything else in the record rides along; the placement and the left side are the move's.
        const { leftSide: _side, ...rest } = withPlace(spec, h.cur, at?.layout, next.place)
        const cur: PanelLayout<P> = { ...rest, v: 2 }
        if (next.leftSide && next.leftSide.length > 0) cur.leftSide = next.leftSide
        savePanelLayout(key, cur)
        return { cur, prev: h.cur }
      }),
    [key, spec],
  )
  // The boxes' two acts: each one undoable step, and a step that would change nothing is none.
  const boxStep = useCallback(
    (make: (cur: PanelLayout<P>) => PanelLayout<P> | null) =>
      setHist((h) => {
        const next = make(h.cur)
        if (!next) return h
        const cur = withScreenBoxes(spec, next)
        savePanelLayout(key, cur)
        return { cur, prev: h.cur }
      }),
    [key, spec],
  )
  const addBox = useCallback(
    (area: PaneColumn | 'side', layout?: string) => boxStep((cur) => addBoxTo(spec, cur, area, layout)),
    [boxStep, spec],
  )
  const setBox = useCallback((box: P, entry: string) => boxStep((cur) => showInBox(spec, cur, box, entry)), [boxStep, spec])
  const setExtra = useCallback(
    (layoutName: string, id: P, on: boolean) => boxStep((cur) => withExtra(spec, cur, layoutName, id, on)),
    [boxStep, spec],
  )
  const reset = useCallback(() => apply(() => emptyPanelLayout<P>()), [apply])
  const setLayout = useCallback(
    (next: PanelLayout<P>) => apply(() => coercePanelLayout(spec, next)),
    [apply, spec],
  )
  const scaleOf = useCallback((id: P) => hist.cur.scale?.[id] ?? 1, [hist.cur])
  const setScale = useCallback(
    (id: P, value: number) =>
      apply((cur) => {
        const scale: Partial<Record<P, number>> = { ...cur.scale }
        const v = boxScaleValue(value)
        if (v == null) delete scale[id]
        else scale[id] = v
        // Everything else in the record rides along; only `scale` changes.
        const { scale: _was, ...rest } = cur
        const next: PanelLayout<P> = { ...rest }
        if (Object.keys(scale).length > 0) next.scale = scale
        return next
      }),
    [apply],
  )
  // Which panes the pending undo would UNMOUNT: removed in the snapshot, present now.
  // Computed from the same history the undo restores, so it cannot describe a different
  // click than the one the button makes — and read through the same default `stateOf` uses,
  // or undoing the tick that docked a default-hidden pane would claim to remove nothing.
  const undoRemoves = useMemo(() => {
    const prev = hist.prev
    if (!prev) return [] as P[]
    return spec.panelIds.filter(
      (id) => panelStateIn(spec, prev, id) === 'removed' && panelStateIn(spec, hist.cur, id) !== 'removed',
    )
  }, [hist, spec])
  return {
    layout: hist.cur,
    stateOf,
    setPanelState,
    shareOf,
    setShare,
    setShares,
    setCols,
    movePane: spec.arrange || spec.arrangeBy ? movePane : undefined,
    addBox: boxIdsOf(spec).length > 0 ? addBox : undefined,
    setBox: boxIdsOf(spec).length > 0 ? setBox : undefined,
    setExtra: spec.arrangeBy ? setExtra : undefined,
    undo,
    canUndo: hist.prev != null,
    undoRemoves,
    reset,
    setLayout,
    scaleOf,
    setScale,
  }
}

/**
 * THE BOXES (2026-10-07: any pane in any area). FT, Phone, CW and JS8 each have six box slots, `box1`
 * to `box6`: a box shows one entry of the shared list (features/sharedPanes — the Conditions boxes and
 * Needed) and stands in the cockpit's columns, or on Phone's left side, wherever ⊞ Panels ▸ Arrange
 * puts it. Six, the operator's pick; slots rather than an id per entry, so each box costs the stop-line
 * sweeps one hide pass rather than twenty-nine. Which entry a box shows is the record's `boxes`
 * (`boxEntries`, at the foot of this file); whether it is on screen, its `state` — and every box SHIPS
 * HIDDEN (`defaultRemoved`), so nobody's screen changes on the update that brings them.
 *
 * Under THE STOP LINE they are the plainest entries there are. A box can show only an entry of the
 * shared list, and none of those hosts a control that stops or starts a transmission (▶ Work and HUNT
 * move the rig and open a cockpit; they key nothing). A box's hide ends nothing — its body stops its
 * own polls when it unmounts, and nothing else — so no box carries a note and Reset hides them
 * silently. `box` is no stop-control word, and the sweeps hide every box with every other id, because
 * they are driven off these arrays.
 */
export const BOX_IDS = ['box1', 'box2', 'box3', 'box4', 'box5', 'box6'] as const
export type BoxId = (typeof BOX_IDS)[number]

/** Whether a vocabulary id is one of the boxes. */
export function isBoxId(id: string): id is BoxId {
  return (BOX_IDS as readonly string[]).includes(id)
}

/** Every entry a box may show. */
const SHARED_IDS: ReadonlySet<string> = new Set(SHARED_PANES.map((e) => e.id))

/** The Operate cockpit's removable panels — the first consumer's vocabulary.
 *
 *  FIVE OF THESE ARE SENDERS, and they are the shipped counter-example that falsified
 *  the third wording of THE STOP LINE (see the header). `txmsgs` hosts Tx1–Tx6, Tx6 being
 *  "Call CQ (Alt+6)" → `startCq`; `bandActivity`, `rxfreq`, `callRoster` and `stations` all
 *  work a station on double-click, which arms TX and keys the current period. Every one is
 *  hideable, none of their hides stops anything, and none of them warns — correctly. Hide
 *  Tx messages mid-CQ and the over continues, with TX On / Tune / Stop TX / Hold Tx still
 *  in the merged QSO strip, where no ⊞ id can reach them. That is the rule holding. */
export const OPERATE_PANEL_IDS = [
  'waterfall',
  // The RF scope pane (RF_SCOPE_PANEL_ID, below): beside the waterfall in its strip, and hidden
  // until ticked.
  'rfScope',
  'bandActivity',
  'callRoster',
  'rxfreq',
  'txmsgs',
  'stations',
  'txmeters',
  // #204 (KR4FQG): the callsign card, a panel of its own. It neither starts nor stops a
  // transmission — it shows who a station is — so it is hideable under THE STOP LINE with no note.
  'recall',
  // THE BOXES (BOX_IDS, above), shipped hidden like everywhere else.
  ...BOX_IDS,
] as const
export type OperatePanelId = (typeof OPERATE_PANEL_IDS)[number]

/** FT's two layouts, the cockpit header's Classic / Roster. */
export type OperateLayout = 'classic' | 'roster'

/**
 * WHERE FT'S PANES STAND (⊞ Panels ▸ Arrange, 2026-10-07: the operator's "Arrange on FT's grid" and "Two
 * saved arrangements"). Each layout is arranged on its own, over its own panes, and the record keeps one
 * placement per layout (`places`), so switching Classic ↔ Roster keeps both. The stock arrangements are
 * the columns FT has always drawn:
 *   · Classic: Band Activity | Rx Frequency over the Tx1–Tx6 machine | the callsign card over Stations;
 *   · Roster: the Call Roster | the card over Band Activity over Rx Frequency.
 * The columns are FT's own grid's, not the pane grid's: `a` is the first column, `b` Classic's second,
 * and `log` is the side rail in both layouts (Roster has no `b`). The rail keeps the side the operator
 * put it on (⊞ Panels ▸ Side rail on the left), and the dividers between the columns stay where they are.
 * Nothing is pinned: no FT pane holds a transmission or a half-typed contact in its own state — the
 * decode windows' history and the Tx1–Tx6 texts are the cockpit's — so a pane that changes column loses
 * only its sort (the Stations list, its filter too).
 * EVERY FT PANE IN BOTH LAYOUTS (the operator's pick, 2026-10-07): each layout also lists the panes only the
 * other draws (`extra`): Classic the Call Roster, under Band Activity; Roster the Tx messages and Stations,
 * in the side rail under Rx Frequency. Each layout's ⊞ Panels offers them unticked, so each layout opens
 * exactly as it always drew; ticked, one stands there and moves like any pane (the record's `extras`). The
 * Tx messages start a transmission (Tx6 is Call CQ) wherever they stand, as they always could, and hold no
 * stop control; Stop TX stays in the QSO strip, which no layout lists.
 * THE BOXES stand at the foot of the side rail until ⊞ Arrange puts them elsewhere.
 *
 * THE STOP LINE is not near any of this: Stop TX, Tune and the rest of the QSO strip have no id, so no
 * placement can name them, and every pane here is one the ⊞ menu could already hide.
 */
export const OPERATE_ARRANGE: Readonly<Record<OperateLayout, ArrangeSpec<OperatePanelId>>> = {
  classic: {
    columns: { a: ['bandActivity', 'callRoster'], b: ['rxfreq', 'txmsgs'], log: ['recall', 'stations', ...BOX_IDS] },
    pinned: [],
    boxes: BOX_IDS,
    extra: ['callRoster'],
  },
  roster: {
    columns: { a: ['callRoster'], b: [], log: ['recall', 'bandActivity', 'rxfreq', 'txmsgs', 'stations', ...BOX_IDS] },
    pinned: [],
    boxes: BOX_IDS,
    cols: ['a', 'log'],
    extra: ['txmsgs', 'stations'],
  },
}

export const OPERATE_PANELS: PanelVocabulary<OperatePanelId> = {
  view: 'operate',
  panelIds: OPERATE_PANEL_IDS,
  defaultRemoved: ['rfScope', ...BOX_IDS],
  arrangeBy: OPERATE_ARRANGE,
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

/**
 * THE RF SCOPE PANE'S ID, shared by the five digital cockpits — FT (Operate), JS8, RTTY, PSK and
 * SSTV — and listed in each one's `defaultRemoved`: it SHIPS HIDDEN. The operator's pick
 * (2026-10-03, "Yes, opt-in pane"): an RF scope pane, off by default, with the audio waterfall
 * staying the default, so nobody's screen changes on the update that adds it.
 *
 * It is the radio's own panadapter (components/RfScopePane — PhoneScope with `feed="rf"`), a
 * different thing from SCOPE_PANEL_ID's audio waterfall in the same cockpits, so it has a name of
 * its own. ONE id across the five for SCOPE_PANEL_ID's reason: one thing wherever it renders.
 *
 * Under THE STOP LINE it is the plainest kind of entry. Its whole surface is display — G and Z, the
 * ⚙ strip, 3D, pause, the scroll direction — with no click-to-tune at all, so it hosts no control
 * that stops a transmission and none that starts one, and its hide ends nothing in flight (the
 * radio's scope stream stopping two seconds after it is hidden is not a transmission), so it
 * carries no ⊞ note. That last property is also what lets it ship hidden: ⊞ Reset hides it again
 * with no warning. The sweeps in components/stop-line.test.tsx and OperateCockpit.structure.test.tsx
 * hide it with every other id, because they are driven off these arrays.
 */
const RF_SCOPE_PANEL_ID = 'rfScope'

/** SSTV view's removable panels (Phase 3). The RX image stage and the TX bar
 *  (mode/Send/Stop/progress) and all header chrome are NOT panels — so SSTV's two stop
 *  controls, the TX bar's Stop and the TX strip's TX-enable latch, are outside every
 *  ⊞-removable pane, which is THE STOP LINE. `txcompose` is the image chooser only; Send
 *  does not live in it.
 *
 *  `scope` here is THE BAND WATERFALL ONLY, and the distinction is structural rather than
 *  cosmetic: SSTV has one RX stage that is the band until a VIS lands and the PICTURE after
 *  it (SstvView's `inFlight` branch). The tick hides the band half; an arriving picture
 *  still takes the stage whatever the box says, because that is the decode itself and not a
 *  panel. See the gate at the render site. */
export const SSTV_PANEL_IDS = [SCOPE_PANEL_ID, RF_SCOPE_PANEL_ID, 'txcompose', 'gallery'] as const
export type SstvPanelId = (typeof SSTV_PANEL_IDS)[number]

export const SSTV_PANELS: PanelVocabulary<SstvPanelId> = {
  view: 'sstv',
  panelIds: SSTV_PANEL_IDS,
  defaultRemoved: [RF_SCOPE_PANEL_ID],
}

/** Phone cockpit's removable panels (Phase 3) — the scope strip plus the panes under it.
 *  The whole CockpitHeader (mode/band/power/Tune/StopTX/split/CAT) and the PTT row
 *  are NOT panels: each hosts a way to STOP a transmission, which is THE RULE. (The header's
 *  list is shorter than it was: mic gain, AF gain and BW moved into the two chain panes in
 *  the 2026-09-20 rebuild. None of the three ever stopped anything, so the rule is untouched
 *  — but a stale list here is how somebody later "restores" a control into the header.) The log strip
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
/*  ⭐ `receiver` AND `transmitter` REPLACED `dsp` AND `dspLevels` (operator, 2026-09-20).
 *
 *  The two they replace were named after their IMPLEMENTATION — which Hamlib call a function
 *  and which a level — so the receive chain was split across both of them (NB/NR/notch in one,
 *  the NR level/AGC/RF/squelch in the other) while the two TRANSMIT controls that happened to
 *  be Hamlib "functions" sat among the receive ones. A voice operator asks two questions and
 *  no others: what goes out when I key, and what am I hearing. One pane each, and nothing in
 *  this vocabulary is named after the wire any more.
 *
 *  Nothing about the STOP LINE changes: both are ordinary removable panes, neither holds a
 *  stop control, and the ids are swept the moment they are listed here — `stop-line.test.tsx`
 *  drives Phone's case off `PHONE_PANEL_IDS` itself rather than a copy of it. */
/*  ⭐ `spots` AND `needed` (#345, operator 2026-09-27) are the Spots and Needed boards as Phone
 *  FEEDS — "much empty real estate" was the tester's report, and a phone operator's band map is
 *  what fills it. They were the first ids in the app to SHIP HIDDEN (`defaultRemoved`; CW's twins
 *  below are the only others), by the operator's pick: "Hidden, add via ⊞ Panels — nobody's Phone
 *  screen changes on update."
 *
 *  Under THE STOP LINE they are the plainest entries on the list. Neither holds a stop control;
 *  neither is a sender (working a row QSYs and opens a cockpit through the boards' own handlers,
 *  exactly as from the Spots and Needed views, and keys nothing — Band Activity's shape); and a
 *  hide ends nothing, so their entries carry no note. That last property is ALSO what lets them
 *  be default-hidden: ⊞ Reset hides them with no warning, which is only right for a hide that
 *  ends nothing. */
export const PHONE_PANEL_IDS = [
  SCOPE_PANEL_ID,
  'rigscope',
  'txmeters',
  'receiver',
  'transmitter',
  'bandActivity',
  'voiceKeyer',
  'spots',
  'needed',
  ...BOX_IDS,
] as const
export type PhonePanelId = (typeof PHONE_PANEL_IDS)[number]

export const PHONE_PANELS: PanelVocabulary<PhonePanelId> = {
  view: 'phone',
  panelIds: PHONE_PANEL_IDS,
  defaultRemoved: ['spots', 'needed', ...BOX_IDS],
  // ⊞ Arrange (layout L3): the pane region's stock grouping, as PhoneCockpit renders it — Band
  // Activity, the voice keyer and Spots lead; the rig strips and Needed in the middle; the log form
  // (no id) alone in the last column. The scope above the region and the meters in the dock are not
  // the region's, so they are not here and can never be given a place. THE VOICE KEYER IS PINNED
  // (D9): it transmits, and a pane that changes column is remounted, which would stop its over and
  // discard its recording. Below three tracks the feeds follow the rig strips (`stockMerged`).
  // THE LEFT SIDE (operator's pick, 2026-10-03): Band Activity, Spots and Needed may stand in a
  // full-height column beside the scope. Never the voice keyer or the log form: the side comes and
  // goes with the window's width (about 1280 px), and a pane that changes parent is remounted, which
  // would cut off a voice message or a half-typed contact. The rig strips stay in the region.
  // THE BOXES (BOX_IDS) stand at the foot of the leading column until ⊞ Arrange puts them elsewhere,
  // the left side included; where they stand is no part of the stock grouping (features/panelPlace).
  arrange: {
    columns: {
      a: ['bandActivity', 'voiceKeyer', 'spots', ...BOX_IDS],
      b: ['rigscope', 'receiver', 'transmitter', 'needed'],
      log: [],
    },
    pinned: ['voiceKeyer'],
    stockMerged: ['bandActivity', 'voiceKeyer', 'rigscope', 'receiver', 'transmitter', 'spots', 'needed'],
    leftSide: ['bandActivity', 'spots', 'needed', ...BOX_IDS],
    boxes: BOX_IDS,
  },
  // Its Spots and Needed panes are the shared list's boards: while one shows, no box shows it too.
  sharedAs: { spots: 'spotsBoard', needed: 'neededBoard' },
}

/** CW cockpit's removable panels (Phase 3) — the scope strip plus the panes under it. The
 *  whole CockpitHeader + keyer (WPM/backend/pitch/BW/Tune/StopTX), the F-key macros, the
 *  type-to-send input, and the log strip are NOT panels (TX-safety by construction).
 *
 *  THE SCOPE ITSELF IS ONE, since 2026-08-16 (see SCOPE_PANEL_ID) — the same correction as
 *  in the Phone twin: it was listed as unhideable "by TX-safety", and it hosts no stop
 *  control. `scopeCtl` is a different entry for a different thing (the controls that command
 *  the rig's own panadapter, in the region below), and both keep their own box.
 *
 *  `spots` AND `needed` are Phone's two feeds (#345) in CW, the same boards with the same wiring
 *  (plan piece H8, operator's pick "CW gets Phone's Spots/Needed panes"), and they ship the same
 *  way: HIDDEN (`defaultRemoved`), one tick in ⊞ Panels, so nobody's CW screen changes on update.
 *  Under THE STOP LINE they are Phone's plainest entries again: neither holds a stop control,
 *  neither sends (a row QSYs through the board's own handler and keys nothing), and a hide ends
 *  nothing, so neither carries a note — which is also what lets Reset hide them silently. */
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
  'spots',
  'needed',
  ...BOX_IDS,
] as const
export type CwPanelId = (typeof CW_PANEL_IDS)[number]

export const CW_PANELS: PanelVocabulary<CwPanelId> = {
  view: 'cw',
  panelIds: CW_PANEL_IDS,
  defaultRemoved: ['spots', 'needed', ...BOX_IDS],
  // ⊞ Arrange (layout L3): the pane region's stock grouping, as CwCockpit renders it — the decode
  // and the sent echo lead; Band Activity and the copilot in the middle, under the Rig controls
  // frame; the log form (no id) alone in the last column. The three rig-control groups (`scopeCtl`,
  // `dsp`, `rxdsp`) share ONE frame, which is not a vocabulary pane: it keeps its place at the head
  // of the middle column, and is not listed. Nothing here is pinned (CW has no voice keyer; the log
  // form cannot move). Below three tracks the middle column simply follows the first. The two feeds
  // take Phone's places: Spots at the foot of the leading column, Needed at the foot of the middle,
  // and below three tracks both after every strip (`stockMerged`, Phone's rule), so ticking one never
  // pushes the Rig controls (which CwCockpit keeps ahead of them), Band Activity or the copilot down.
  // THE BOXES (BOX_IDS) stand at the foot of the leading column until ⊞ Arrange puts them elsewhere
  // (Phone's rule).
  arrange: {
    columns: {
      a: ['decode', 'sent', 'spots', ...BOX_IDS],
      b: ['bandActivity', 'copilot', 'needed'],
      log: [],
    },
    pinned: [],
    stockMerged: ['decode', 'sent', 'bandActivity', 'copilot', 'spots', 'needed'],
    boxes: BOX_IDS,
  },
  // Phone's: its Spots and Needed panes are the shared list's boards.
  sharedAs: { spots: 'spotsBoard', needed: 'neededBoard' },
}

/** RTTY cockpit's removable panels (Phase 3). The CockpitHeader + StopTX + TX-arm, the
 *  auto-sequence QSO strip, the F-key macros, and the type-to-send compose are NOT panels.
 *
 *  THE WATERFALL IS ONE, since 2026-08-16 (see SCOPE_PANEL_ID) — it headed the unhideable
 *  list above until then, and nothing about it belonged there: its mark/space cursors and
 *  click-to-net are the decoder's tuning aid, not a stop. `stream` is unchanged and is still
 *  the pane that hosts a stop control of its own. With both ticked off this cockpit renders
 *  no ⊞-reachable content at all, and THE STOP LINE holds exactly as before: Stop TX and the
 *  TX-enable latch are in the TX strip, the Esc/Stop macro and the sequencer's Abort in the
 *  dock, none of them with an id.
 *
 *  `stream` IS THE SECOND PANE IN THE APP THAT HOSTS A STOP CONTROL, and it is admitted for
 *  the same reason the voice keyer is. Its "Auto on" toggle, clicked off, is
 *  rttySetAuto(false) → seq.abort() + Engine::rtty_stop(): the queue is cleared and
 *  rtty_abort + slot_tx_abort unkey the rig. Hide `stream` and that toggle goes with it —
 *  allowed, because Stop TX and the TX-enable latch are in the TX strip and the Esc/Stop macro
 *  and the sequencer's Abort are in the dock, none of them with an id. Its hide ENDS nothing
 *  (unmounting the pane calls no wire), so it correctly carries no ⊞ note. This is the pane
 *  that falsified the FOURTH wording a second time — see the header. */
export const RTTY_PANEL_IDS = [SCOPE_PANEL_ID, RF_SCOPE_PANEL_ID, 'stream'] as const
export type RttyPanelId = (typeof RTTY_PANEL_IDS)[number]

export const RTTY_PANELS: PanelVocabulary<RttyPanelId> = {
  view: 'rtty',
  panelIds: RTTY_PANEL_IDS,
  defaultRemoved: [RF_SCOPE_PANEL_ID],
}

/** PSK cockpit's removable panels (Keyboard Modes; TX since Phase 2).
 *  RTTY's shape: `scope` is the band waterfall (see SCOPE_PANEL_ID), `stream` the decoded
 *  transcript. The CockpitHeader and the TX dock are not panels.
 *
 *  THE STOP LINE holds here the RTTY way (the Phase 1 "by construction" census is gone,
 *  as its own comment demanded when TX arrived). The census — every holder OUTSIDE every
 *  ⊞-removable pane, none with an id in this vocabulary: Stop TX (TX strip, never
 *  disabled), the dock's Esc/Stop macro (`disabled={!(sending || latched)}`, live from
 *  the instant the continuous-TX latch goes up), the TX-enable latch (TX strip arm —
 *  `set_tx_enabled(false)` arms `psk_abort`, so it is a real stop here exactly as in
 *  RTTY/SSTV), and Esc (keyboard-only ⇒ census-only, outside both sweeps by
 *  construction). Swept in stop-line.test.tsx's PSK case, rendered with App's props
 *  (`onSetTxEnabled` — the documented latch blindness). The continuous-TX ("TX") button
 *  is a SENDER, not a stop (RTTY's ruling: off lets what was typed finish keying) and
 *  must never be added to the sweep's stopControls. A fifth stop reaches a latched over
 *  with no control pressed: the engine's per-tick gate re-check (`poll_psk_stream`),
 *  which unkeys within one tick on a section change, a QSY out of privileges, a tune,
 *  or a radio handoff. */
export const PSK_PANEL_IDS = [SCOPE_PANEL_ID, RF_SCOPE_PANEL_ID, 'stream'] as const
export type PskPanelId = (typeof PSK_PANEL_IDS)[number]

export const PSK_PANELS: PanelVocabulary<PskPanelId> = {
  view: 'psk',
  panelIds: PSK_PANEL_IDS,
  defaultRemoved: [RF_SCOPE_PANEL_ID],
}

/** JS8 cockpit's removable panels (the JS8 programme, 2026-09). CW's region shape:
 *  `scope` is the band waterfall (see SCOPE_PANEL_ID), `activity` every decoded frame at
 *  every enabled speed, `offsets` the same decodes collapsed to one row per frequency offset,
 *  `stations` the heard list, `inbox` the directed / store-and-forward messages, `log` the
 *  LogEntry strip. The CockpitHeader and the TX dock are not panels.
 *
 *  `offsets` is JS8Call's second decode surface (`tableWidgetRXAll`, mainwindow.ui:989 — the
 *  offset-bucketed table with the Time Delta column), which the first build fused into the
 *  chronological `activity` transcript. It is an ordinary pane with an ordinary id: it renders
 *  no sender and no stop, so it is ⊞-hideable exactly like its siblings and needs no special
 *  case in the census below.
 *
 *  THE STOP LINE holds here the Operate way (a slotted mode): the census — every holder
 *  OUTSIDE every ⊞-removable pane, none with an id in this vocabulary: Stop TX (TX strip →
 *  halt_tx, never disabled), Tune (TX strip; the carrier it started), and Esc (window keydown
 *  bound only while JS8 is the visible view → the same halt; keyboard-only ⇒ census-only).
 *  The TX-enable latch is NOT a stop control here: `set_tx_enabled(false)` deliberately does
 *  not arm `slot_tx_abort` (the operator's 2026-07-31 Operate ruling — a frame in flight
 *  completes), so it is not on the sweep list. The dock's "Drop queue" is a SENDER-class
 *  control (it empties the queue; a frame already keyed finishes) and must never be added to
 *  stopControls. Swept in stop-line.test.tsx's JS8 case, rendered with App's props. */
export const JS8_PANEL_IDS = [SCOPE_PANEL_ID, RF_SCOPE_PANEL_ID, 'activity', 'offsets', 'stations', 'inbox', 'log', ...BOX_IDS] as const
export type Js8PanelId = (typeof JS8_PANEL_IDS)[number]

export const JS8_PANELS: PanelVocabulary<Js8PanelId> = {
  view: 'js8',
  panelIds: JS8_PANEL_IDS,
  defaultRemoved: [RF_SCOPE_PANEL_ID, ...BOX_IDS],
  // ⊞ Arrange (layout L3): the pane region's stock grouping, as Js8Cockpit renders it — the two
  // decode surfaces lead, Stations and the inbox in the middle, the log alone in the last column.
  // The RF scope pane, once ticked, heads the leading column: the first pane under the TX strip, as
  // in RTTY, PSK and SSTV.
  // JS8'S LOG HAS AN ID (it is ⊞-hideable), so it is listed, and it is PINNED (D9): the log form
  // holds a half-typed contact, which a change of column would remount and lose. It moves up and
  // down in its column only.
  // THE BOXES (BOX_IDS) stand at the foot of the leading column until ⊞ Arrange puts them elsewhere.
  arrange: {
    columns: {
      a: [RF_SCOPE_PANEL_ID, 'activity', 'offsets', ...BOX_IDS],
      b: ['stations', 'inbox'],
      log: ['log'],
    },
    pinned: ['log'],
    boxes: BOX_IDS,
  },
}

/** Connect's removable panes (close + resize, operator-approved 2026-09-13) — its seven
 *  wrap-the-globe SLOTS, not its pane ids. Which pane sits in a slot is PLACEMENT and stays in
 *  `nexus.connect.config` (features/connectConfig), exactly the split this module's header
 *  describes: closing a slot keeps its pane, and ⊞ Panels brings the same pane back.
 *
 *  THE STOP LINE holds here by the emptiest route there is: Connect renders NO transmit
 *  control, in a pane or out of one (its panes' ▶ Work QSYs and opens a cockpit; it keys
 *  nothing). There is no stop control for a hide to cost. And Connect is the stop line's ONE
 *  RULED EXCEPTION as a screen: App draws no top bar there, so no Stop TX either — the operator,
 *  2026-10-01: "remove all radio control from connect, reclaim that space". Transmit on Connect
 *  is stopped by Esc (App binds it while Connect is shown) or by leaving the screen
 *  (stop-control-wiring.test.tsx holds both). What IS swept is that a hide reaches only its own pane —
 *  ConnectView.panes.test.tsx closes every slot, singly and all at once, and requires every
 *  control outside the panes to still be on screen (declared in stop-line.test.tsx ELSEWHERE).
 *  No hide here ends anything in flight, so no entry carries a note.
 *
 *  Connect is ALSO the one vocabulary mounted in a pop-out that shares the main window's
 *  instance token (`?panel=connect` carries none), so ConnectView scopes the record by
 *  SURFACE and inherits the main window's on first open — see loadPanelLayout's `inherit`. */
export const CONNECT_PANELS: PanelVocabulary<SlotId> = {
  view: 'connect',
  panelIds: SLOT_IDS,
}

/** The dashboard rail's four slots (components/DashRail, a column of Connect boxes beside the
 *  cockpits). Connect's split exactly: which box sits in a slot is PLACEMENT, in
 *  `nexus.dashrail.config` (features/dashRail), and this record says whether a slot is shown and how
 *  it shares the column with its neighbours — so a closed slot keeps its box, and the rail's own ⊞
 *  brings the same box back.
 *
 *  THE STOP LINE holds here by Connect's route: the rail renders NO transmit control (its boxes are
 *  Connect's, which key nothing — ▶ Work moves the rig and opens a cockpit), and it is a sibling of
 *  the cockpit in the app shell, never inside a cockpit shell, so no id here can reach a cockpit's
 *  header or dock. Swept in DashRail.stopLine.test.tsx (declared in stop-line.test.tsx ELSEWHERE). No
 *  hide here ends anything in flight, so no entry carries a note. */
export const DASH_PANELS: PanelVocabulary<DashSlotId> = {
  view: 'dashrail',
  panelIds: DASH_SLOT_IDS,
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
  JS8_PANELS,
  CONNECT_PANELS,
  DASH_PANELS,
]

/**
 * The boot clear, over EVERY vocabulary. main.tsx calls exactly this on a fresh main-window
 * boot; it must not call `redockStalePopouts` per-record, because that is how the bug it
 * fixes got in — it ran on OPERATE_PANELS alone.
 *
 * Operate is the only cockpit that ever WRITES 'popped' into a visibility record —
 * OperateCockpit's waterfall pop-out holds the app's single `setPanelState(id, 'popped')`
 * call — and the only one with a re-dock bar. Pop-out AFFORDANCES are not rare, and an earlier
 * version of this note implied they were: DetachedPanel dispatches twelve panel kinds (waterfall,
 * needed, memories, connect, dxped, sats, pota, fieldday, fdclub, operate, bandmapPhone,
 * bandmapCw), and
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

// ── A CONNECT BOX'S OWN TEXT SIZE (⋯ ▸ A− / A+) ──────────────────────────────────────────────
// The operator's pick (2026-09-29): "Per-box text size (A−/A+)", 80–160 %. A factor on the app's
// Text size (Settings ▸ Workspace ▸ Text size), never a structural size: styles.css multiplies
// --text-scale inside the box body by it, and the frame, the rails and the strip keep their sizes.
// Stored per SLOT in the record (`PanelLayout.scale`), so it is part of the board, like a split.

export const BOX_SCALE_MIN = 0.8
export const BOX_SCALE_MAX = 1.6
/** One press of A− or A+. */
export const BOX_SCALE_STEP = 0.1

/** A stored or requested box text size as the record keeps it: clamped into the range, rounded to
 *  the hundredth (so three presses of A+ are 1.3, not 1.3000000000000003), and null for 1 (no entry)
 *  or for anything that is not a positive number. */
export function boxScaleValue(v: unknown): number | null {
  if (typeof v !== 'number' || !Number.isFinite(v) || v <= 0) return null
  const c = Math.round(Math.min(BOX_SCALE_MAX, Math.max(BOX_SCALE_MIN, v)) * 100) / 100
  return c === 1 ? null : c
}

// ── THE BOXES: what each one shows (BOX_IDS, above) ──────────────────────────────────────────────

/** The entries the cockpit's own panes show right now (`sharedAs`), by `stateOf` (the record's own
 *  reading unless a host gives its own). */
function ownShown<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  stateOf: (id: P) => PanelState = (id) => panelStateIn(spec, layout, id),
): string[] {
  return (Object.entries(spec.sharedAs ?? {}) as [P, string][])
    .filter(([own]) => stateOf(own) !== 'removed')
    .map(([, entry]) => entry)
}

/**
 * What each box ON SCREEN shows: the ONE reading of the record's `boxes`, for the cockpits, the picker,
 * "+ Add a box" and the record's own coercion. ONCE PER SCREEN (the operator's pick): a box keeps its
 * stored entry unless the list has no such entry, the cockpit already shows it as one of its own panes
 * (`sharedAs`), or an earlier box holds it; then, as for a box on screen with nothing stored, it shows
 * the first entry of the shared list that is not on screen — twenty-nine entries and six boxes, so
 * there always is one. A hidden box shows nothing. Whether a pane is on screen is `stateOf`: the record's
 * own reading (`panelStateIn`), or the host's — a cockpit passes its panel record's `stateOf`, so its
 * boxes are on screen exactly when the host says, as every other pane of it is.
 */
export function boxEntries<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  stateOf: (id: P) => PanelState = (id) => panelStateIn(spec, layout, id),
): Partial<Record<P, string>> {
  const ids = boxIdsOf(spec)
  const onScreen = (b: P) => stateOf(b) !== 'removed'
  const taken = new Set(ownShown(spec, layout, stateOf))
  const out: Partial<Record<P, string>> = {}
  for (const b of ids) {
    const e = layout.boxes?.[b]
    if (onScreen(b) && e != null && SHARED_IDS.has(e) && !taken.has(e)) {
      out[b] = e
      taken.add(e)
    }
  }
  for (const b of ids) {
    if (!onScreen(b) || out[b] != null) continue
    const free = SHARED_PANES.find((x) => !taken.has(x.id))
    if (!free) continue
    out[b] = free.id
    taken.add(free.id)
  }
  return out
}

/** The record with its boxes made what the screen shows: each box on screen its `boxEntries` entry, and
 *  a hidden one what it last showed. Every load and every change goes through it, so what is stored is
 *  what a reload shows. */
function withScreenBoxes<P extends string>(spec: PanelVocabulary<P>, layout: PanelLayout<P>): PanelLayout<P> {
  const ids = boxIdsOf(spec)
  if (ids.length === 0) return layout
  const boxes: Partial<Record<P, string>> = {}
  for (const b of ids) {
    const e = layout.boxes?.[b]
    if (e != null && SHARED_IDS.has(e)) boxes[b] = e
  }
  Object.assign(boxes, boxEntries(spec, layout))
  const { boxes: _was, ...rest } = layout
  return Object.keys(boxes).length > 0 ? { ...rest, boxes } : rest
}

/**
 * "+ Add a box" (⊞ Panels ▸ Arrange): the first hidden box, put on screen at the foot of `area` and
 * showing the first entry of the shared list not on screen. On the LEFT SIDE it also stands at the foot
 * of its stock column, which is where it is whenever the side does not show; in a column it leaves the
 * side. Null when every box is on screen, or for the side of a cockpit that has none. The cockpit's own
 * panes stay exactly where they were. `layoutName`: the layout it is added in, for a vocabulary arranged
 * per layout (FT), whose placement it then stands in.
 */
export function addBoxTo<P extends string>(
  spec: PanelVocabulary<P>,
  layout: PanelLayout<P>,
  area: PaneColumn | 'side',
  layoutName?: string,
): PanelLayout<P> | null {
  const arrange = arrangeSpecOf(spec, layoutName)
  if (!arrange?.boxes || (area === 'side' && !arrange.leftSide)) return null
  const box = arrange.boxes.find((b) => panelStateIn(spec, layout, b) === 'removed')
  if (box == null) return null
  const taken = new Set([...ownShown(spec, layout), ...Object.values(boxEntries(spec, layout))])
  const free = SHARED_PANES.find((e) => !taken.has(e.id))
  if (!free) return null
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  state[box] = 'docked'
  const boxes: Partial<Record<P, string>> = { ...layout.boxes }
  boxes[box] = free.id
  const col = area === 'side' ? (stockColumn(arrange, box) ?? 'a') : area
  const side = (layout.leftSide ?? []).filter((x) => x !== box)
  const { leftSide: _was, ...rest } = withPlace(spec, layout, layoutName, placeAtFoot(arrange, placeOf(spec, layout, layoutName), box, col))
  const next: PanelLayout<P> = { ...rest, v: 2, state, boxes }
  if (area === 'side') next.leftSide = [...side, box]
  else if (side.length > 0) next.leftSide = side
  return next
}

/**
 * A box's picker: `box` shows `entry`. An entry already on screen MOVES here (the operator's "Once per
 * screen"): from another box, which takes this box's entry in exchange (the rail's swap), or from the
 * cockpit's own pane that is that board (`sharedAs`), which is hidden in the same step — a hide that
 * ends nothing. Null for what is no entry, what the box already shows, or a `box` that is no box.
 */
export function showInBox<P extends string>(spec: PanelVocabulary<P>, layout: PanelLayout<P>, box: P, entry: string): PanelLayout<P> | null {
  if (!boxIdsOf(spec).includes(box) || !SHARED_IDS.has(entry)) return null
  const shown = boxEntries(spec, layout)
  const was = shown[box] ?? layout.boxes?.[box]
  if (was === entry) return null
  const boxes: Partial<Record<P, string>> = { ...layout.boxes }
  boxes[box] = entry
  const other = (Object.keys(shown) as P[]).find((b) => b !== box && shown[b] === entry)
  if (other != null) {
    if (was != null) boxes[other] = was
    else delete boxes[other]
  }
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  for (const [own, e] of Object.entries(spec.sharedAs ?? {}) as [P, string][]) {
    if (e === entry && panelStateIn(spec, layout, own) !== 'removed') state[own] = 'removed'
  }
  return { ...layout, state, boxes }
}

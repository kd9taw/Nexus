import { useRadioLevels } from '../remote-web/useRadioLevels'
import { useRemotePresentation } from '../remote-web/presentation'
import { useReceiverFilter } from '../remote-web/useReceiverFilter'
import { useReceiverDsp } from '../remote-web/useReceiverDsp'
import { usePhoneMode } from '../remote-web/usePhoneMode'
import { useRemoteScopeClick } from '../remote-web/useRemoteScopeClick'
import { RemoteRecallEntry } from '../remote-web/RemoteRecall'
import { CollectionStatus, useRemoteCollection } from '../remote-web/collections'
import { useStationCapability, useStationControl } from '../stationAccess'
// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), and for one
// reason only: THE PTT ROW, the pinned dock row this cockpit's stop line rests on, is still
// written in English here — the button's four labels (which ARE the accessible name
// components/stop-line.test.tsx matches), its three-armed tooltip, the Lock toggle that
// decides whether the Space bar is a stop at all, and the Field Day exchange chip that
// shares the row. It moves in the transmit-path batch, with the stop-line sweeps re-run.
// Everything else is in the catalog under `phone.*` — the refusal TOASTS that row raises
// included, a toast being neither a control nor anything a sweep can see. Every dial
// reading, split offset, filter and scope width, reference level, percentage, band and mode
// name and the rig's own group plates (DSP, NR, AGC, BW, REC, SPLIT) are invariant tokens
// and stay in the code.
import { useEffect, useState, useRef } from 'react'
import { PHONE_PANEL_IDS, type PhonePanelId, type PanelLayoutApi } from '../features/panelState'
import { panelHost } from '../features/panelHost'
import { composingText } from '../features/contestExchange'
import type { AppSnapshot, FieldDayStatus, NeedTag, SpotRow } from '../types'
import { PhoneScope } from './PhoneScope'
import { TxMeters, TX_METERS_WHEN } from './TxMeters'
import { BandStrip } from './BandStrip'
import { PanelsMenu } from './PanelsMenu'
import { SpotDialog } from './SpotDialog'
import { TuningStrip } from './TuningStrip'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PaneCloseButton } from './panes/PaneCloseButton'
import { Splitter, SCOPE_SPLIT_MAX, SCOPE_SPLIT_MIN } from './Splitter'
import { PalettePicker } from './PalettePicker'
import { BandPicker } from './BandPicker'
import { VoiceKeyer } from './VoiceKeyer'
import { LiveLevelMeter, useSmeterDb } from './LiveMeters'
import { formatDialMhz } from './FrequencyReadout'
// ⊘ — the shared unavailable mark. Its own module so `CockpitHeader` can print the same
// one without an import cycle through this file; see the note there.
import { Unavailable } from './UnavailableMark'
import {
  RIG_CONTROLS,
  causeFor,
  rendersRow,
  absentPlates,
  deadControlProps,
  guard,
  stepsFor,
  type ControlState,
  type RigCapsDto,
  type RigControl,
} from '../features/rigControls'
import { SMeter } from './SMeter'
import { SubReceiverStrip, MainReceiverPlate } from './SubReceiverStrip'
import { LogEntry } from './LogEntry'
import {
  setPtt,
  setRfPower,
  setScopeSpan,
  setYaesuScopeMode,
  setScopeRef,
  setFlexPanSpan,
  setFlexPanRef,
  startQsoRecording,
  stopQsoRecording,
  setTune,
  atuTune,
  haltTx,
  setTxEnabled,
  setAttDb,
  setPreampDb,
  setMonitorGain,
} from '../api'
import { pushToast } from '../toast'
import { controlFailureMessage } from '../remote-web/control-failure'
import { latestOnly } from '../remote-web/latest-only'
import { RotorStrip } from './RotorStrip'
import { MemoryStrip, MemoryStripUnavailable } from './MemoryStrip'
import type { Memory } from '../features/memories'
import { setFrequency, openPanelWindow, getSettings, pointRotatorAtCall } from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { isRfScopeSource, NO_NATIVE_SCOPE_REASON } from '../waterfall'
import { useWheelTune } from '../useWheelTune'
import { useScopeTune } from '../useScopeTune'
import { useRegionCols } from '../useRegionCols'
import { t } from '../i18n'
import { SplitControl } from './SplitControl'
import type { MessageKey } from '../i18n'

/** This cockpit's INVARIANT vocabulary — the words that are technical tokens rather than
 *  prose, gathered here so the i18n guard reads them as the deliberate constants they are:
 *  the rig's own control-group names, the units the readouts print, and the two plates. */
// `DSP` itself is gone with the pane that was named after it: the toggles now live under
// Receiver and Transmitter, which say what they are FOR rather than what they are made of.
const NR = 'NR'
const AGC = 'AGC'
// The rig's own names for the two controls #95 completed. COMP is the speech processor
// (PROC on a Yaesu front panel); NOTCH is the MANUAL notch, not the automatic one.
const COMP = 'COMP'
const NOTCH = 'NOTCH'
// The three analog levels, under the rig's own front-panel names.
const AF = 'AF'
const RF = 'RF'
const SQL = 'SQL'
const HZ = 'Hz'
const BW = 'BW'
/** The transmit monitor's own plate, as it is printed on a front panel. */
const MON = 'MON'
const DB = 'dB'
const DBM = 'dBm'
const REC = 'REC'
/** The transmit contract's own plates. `TX`, `SPLIT` and `XIT` are the rig's words and
 *  `simplex` is the mode term the DTO itself uses — all four are technical vocabulary rather
 *  than prose, so they are printed from here in every locale. */
const TX = 'TX'
const SPLIT = 'SPLIT'
const XIT = 'XIT'
const SIMPLEX = 'simplex'
/** The keying-source plate — the rig's own word, and a warning by its presence alone. */
const VOX_ON = 'VOX'
/** The mode picker's AUTO face — the word and the sideband it resolved to, both tokens. */
const autoPlate = (sideband: string) => `AUTO·${sideband}`

/** The ADIF **Mode** a phone mode word names, or `null` when it names no phone mode at all
 *  (CW, RTTY, a PKT/data submode, a rig that reported nothing).
 *
 *  USB and LSB deliberately answer SSB. They are ADIF SUBMODEs, not Modes — `<MODE:3>USB` gets
 *  the whole record rejected by TQSL on the closed Mode enumeration, the same trap
 *  `logbook::adif_submode` documents for a bare `<MODE:9>TempoFast`, and `LOG_MODES` in the log
 *  strip omits them for that reason. AM and FM are Modes in their own right and ride through:
 *  AM is the first row of the ADIF 3.1.7 Mode enumeration (adif.org/317, read 2026-09-10), with
 *  no submodes and no import-only marking. */
function phoneAdifMode(mode: string): 'SSB' | 'FM' | 'AM' | null {
  switch (mode.trim().toUpperCase()) {
    case 'USB':
    case 'LSB':
      return 'SSB'
    case 'FM':
      return 'FM'
    case 'AM':
      return 'AM'
    default:
      return null
  }
}
/** The nudges the two steppers take, as their tooltips print them — figures, so they are
 *  supplied to the message rather than written in it. */
const FILTER_STEP_HZ = 100

/** The AGC chips, in the order `Engine::AGC_SPEEDS` lists them: AUTO left of the three time
 *  constants, OFF right of them — most-automatic through to no AGC at all. The `id` is the
 *  token that goes on the wire and the label is a KEY, not a word: `t()` runs when the row
 *  RENDERS, so a locale switch relabels the chips and the chip is still compared on its id
 *  (the same split RF_SPANS makes, and for the same reason). */
const AGC_CHIPS = [
  { id: 'auto', labelKey: 'phone.rxDsp.agc.auto' },
  { id: 'fast', labelKey: 'phone.rxDsp.agc.fast' },
  { id: 'mid', labelKey: 'phone.rxDsp.agc.mid' },
  { id: 'slow', labelKey: 'phone.rxDsp.agc.slow' },
  { id: 'off', labelKey: 'phone.rxDsp.agc.off' },
] as const satisfies readonly { id: string; labelKey: MessageKey }[]

interface Props {
  /** Pause display subscriptions while a remote contact draft is kept hidden. */
  active?: boolean
  /** Which panels this cockpit shows or hides (⊞ Panels). Owned by the HOST (App), never
   * here — the cockpit remounts on nav and would drop the record. Absent ⇒ every panel shows
   * and the menu hides, so nothing here can hide a transmit control. */
  panels?: PanelLayoutApi<PhonePanelId>
  snap: AppSnapshot
  theme: string
  /** Click-to-work handoff from the Needed board: the callsign to prefill the log with.
   * `ts` changes on each click so re-working the same call refires the prefill. */
  pendingWork?: { call: string; ts: number } | null
  /** Called once the prefill has been applied, so the parent can clear it. */
  onConsumeWork?: () => void
  /** Apply a fresh snapshot returned by a command (so the REC toggle updates instantly
   * instead of waiting for the next poll). */
  onSnap?: (snap: AppSnapshot) => void
  /** Field Day status — when non-null the log strip switches to FD mode. */
  fieldDay?: FieldDayStatus | null
  /** Phone sub-mode from Settings ('ssb' | 'fm') — drives the mode badge, mirroring
   * the rig-mode policy's Phone arm (FM, else sideband by band). */
  phoneMode?: string
  /** Wheel-tune sensitivity (from Settings) — applied to the scope + readout wheel-tune. */
  wheelSensitivity?: number
  /** Live cluster spots (all bands/modes); the band-strip filters to SSB on the current band. */
  spots?: SpotRow[]
  /** Top need tag per heard call (UPPERCASE) — colours band-strip ticks by need tier. */
  needByCall?: Map<string, NeedTag>
  /** Activity type per heard call (UPPERCASE) — POTA/SOTA/DXped badges on the band strip. */
  typeByCall?: Map<string, 'Pota' | 'Sota' | 'Dxped'>
  /** Work a spotted station from the band-strip (QSY to its freq + prefill the log). */
  onWorkSpot?: (s: SpotRow) => void
  /** Recall a saved memory (App applies settings + retune + cockpit switch).
   * Absent when the Memories feature is disabled — the MEM strip then hides. */
  onRecallMemory?: (m: Memory) => void
  /** Open the Memories section (manage/groups/import). */
  onOpenMemories?: () => void
  /** Open Settings at a section id (see settings/registry.ts). Absent ⇒ the surfaces that
   * point at Settings stay plain text. */
  onOpenSettings?: (target: string) => void
  /** Open the Logbook filtered to a callsign (#192) — handed to the log strip's recall card,
   *  whose previous-contact rows become clickable when it is present. Omitted ⇒ inert rows. */
  onOpenLogbook?: (call: string) => void
}

/**
 * Phone (voice) operating cockpit — casual/ragchew. The voice is the signal, so the
 * app does rig control + PTT + logging (you talk into the rig's mic; the live-mic
 * audio bridge + voice keyer land in P3-b/c). Entering forces USB/LSB by band (the
 * rig-mode keystone, wired in App).
 */
/** ⊞ Panels menu labels. The vocabulary itself lives in panelState — note what is NOT in it:
 *  the CockpitHeader (Tune / Stop TX), the PTT row and the log strip are pinned by
 *  construction, so Phone's three stop controls render outside every ⊞-removable pane and no
 *  menu entry can put the operator out of reach of one. Not "no entry hides a stop control":
 *  unticking Voice Keyer hides its ■ Stop, which is allowed — a pane's own stop is a
 *  convenience, and PTT / Stop TX / Tune are what hold the guarantee up. THE STOP LINE lives
 *  in features/panelState.ts. */
/** Resolved when the menu is BUILT — a module constant would freeze the first locale loaded. */
const phonePanelLabels = (): Record<PhonePanelId, string> => ({
  // The strip itself, in this cockpit's own word for it. NOT "Waterfall": what Phone shows
  // there is a trace + waterfall of the passband (or the rig's RF panadapter when one is
  // streaming), and the header above it already calls the thing a scope. `rigscope` below is
  // a different entry — the controls that command the RADIO's scope — so the two labels have
  // to read as different things at a glance, which is why one says Controls and this does not.
  scope: t('phone.panel.scope'),
  rigscope: t('phone.panel.rigscope'),
  txmeters: t('phone.panel.txmeters'),
  receiver: t('phone.panel.receiver'),
  transmitter: t('phone.panel.transmitter'),
  bandActivity: t('phone.panel.bandActivity'),
  voiceKeyer: t('phone.panel.voiceKeyer'),
})

/** The one ⊞ entry whose tick has consequences beyond the pane going away, so the entry
 *  carries them BEFORE the tick rather than apologising after. Hiding the keyer unmounts
 *  it, and its cleanup does two things: stopVoice, which ends a message on the air, and
 *  cancelVoiceRecording, which throws away a capture in progress. The second one is
 *  destructive and is not implied by "hide a pane", so it is named here in the same breath.
 *
 *  NEITHER IS WHAT ADMITS THIS PANE TO THE VOCABULARY — nothing had to admit it. Under THE
 *  STOP LINE (features/panelState.ts) a pane is hideable unless it holds up the guarantee,
 *  and this one does not: Stop TX, Tune and PTT render outside every ⊞-removable pane. A
 *  falsified wording claimed the cleanup bought the entry; it bound every hideable pane in
 *  the app and forbade 23 of the 24. What the cleanup buys is THIS NOTE, and only it.
 *
 *  Both also announce themselves when they happen (VoiceKeyer's cleanup), for the operator
 *  who walked off the Phone screen and never opened a menu. PanelsMenu hangs this off
 *  aria-describedby, so it reaches a screen reader.
 *
 *  ⚠️ NOT MIGRATED, with the one below it. These two are ⊞ MENU NOTES, and the other four on
 *  this cockpit's menu live in `features/panelHost.ts` (the two DSP reasons), `waterfall.ts`
 *  (the native-scope reason) and `TxMeters.tsx` — none of which this batch owns. Moving the
 *  two that happen to sit in this file would leave one menu speaking two languages, so the
 *  five move together when the notes do. The announcements VoiceKeyer makes AFTER the fact
 *  did move; they are toasts, not menu prose.
 */
export const VOICE_KEYER_STOPS_ON_HIDE =
  'hiding this stops a voice message that is playing and throws away a recording in ' +
  'progress — the F-keys go with it'

/** The same consequence for the OTHER button in the ⊞ menu that reaches the same teardown.
 *  ⊞ Undo restores the layout as it was before the last change, so when that layout had the
 *  keyer unticked, pressing Undo unmounts the keyer exactly as a tick does — reproduced:
 *  untick, tick back, start a recording, press Undo, and the take is binned. The tick warns
 *  first; so does this.
 *
 *  ⊞ RESET LAYOUT DOES NOT REACH IT, and three places used to say it did. `reset` applies
 *  `emptyPanelLayout()` and `stateOf` reads an absent state as 'docked', so Reset can only
 *  ever MOUNT the keyer — it is Undo that is the second hide path, and it is the one that
 *  needed covering.
 *
 *  Hand-paired with `voiceKeyer` here rather than generalised through panelHost: there is
 *  exactly one pane in the app whose hide ends anything. The PAIRING is what is computed —
 *  PhoneCockpit.keyerHide.test.tsx drives every id through the undo path and asks the wire —
 *  so a second such pane fails the day it is added, and that is the day this moves into the
 *  spec beside `notes`. */
export const VOICE_KEYER_UNDO_ENDS =
  'undo hides Voice Keyer again — that stops a voice message that is playing and throws ' +
  'away a recording in progress'

/** Expert DSP-function toggles. `key` matches the RadioStatus field + the set_rig_func name; the
 * cockpit only renders those the rig reports as supported (field non-null), so no dead buttons.
 *
 * ⭐ THE TWO NOTCH LABELS ARE NOT THE RIG'S VOCABULARY, and that is deliberate — everything else
 * here is. NB/NR/COMP/VOX are printed on the radio's own panel and mean the same thing on every
 * one; "Notch" is not. A Yaesu calls the AUTOMATIC notch DNF and the MANUAL one NOTCH, so
 * Hamlib's names (ANF → "Notch", MN → "MN") read to a Yaesu operator as exactly INVERTED: they
 * pressed Notch for the one you park on a heterodyne and got the hunter. Reported twice — #95
 * (FT-991A, closed) and an open FTDX-10 report — against correctly wired buttons.
 *
 * OPERATOR RULING, 2026-09-20: plain function names, the SAME on every rig. Rejected: a
 * per-vendor label table (this project has been bitten by per-model tables — five rotator models
 * sat dead at the wrong baud) and keeping Hamlib's words (the report stands, twice).
 *
 * ⚠️ WHY THEY ARE STILL LITERALS and not catalog keys, now that they are plain English rather
 * than rig tokens: the ruling says the same name in all five locales, and a catalog entry is the
 * one thing that cannot promise that — five files, five translators, and the pair's whole job is
 * that AUTO and MANUAL are told apart at a glance. The tooltips beside them ARE translated and
 * carry the explanation; these two words are the invariant token the sentence refers to.
 * `PhoneCockpit.notch.test.tsx` pins the pair by pressing each and reading the rig function back,
 * so a future re-inversion is a red test rather than a third report. */
/** ⚠️ SPLIT BY THE QUESTION THEY ANSWER, not by what Hamlib calls them (2026-09-20). These
 *  six were one list because they are one Hamlib concept — a rig FUNCTION, a boolean at an
 *  index — and that is an implementation fact about the wire, not about operating. Four of
 *  them shape what the operator HEARS and two shape what GOES OUT when they key, so they sit
 *  in different panes now and the list is two lists.
 *
 *  `chain` is the control's anchor in its pane: the ⊘ mark hangs off it, so it must be ONE
 *  control per anchor. The notch pair is the reason that matters — `notch` (ANF) and
 *  `manualNotch` (MN) are separately reported, and a shared anchor would make a rig with the
 *  automatic one look exactly like a rig with neither. */
const RX_FUNCS = [
  { key: 'nb', label: 'NB', chain: 'NB', titleKey: 'phone.dsp.nb.title' },
  { key: 'nr', label: 'NR', chain: 'NR', titleKey: 'phone.dsp.nr.title' },
  { key: 'notch', label: 'Auto notch', chain: 'ANF', titleKey: 'phone.dsp.notch.title' },
  // #95. TWO notches, and they are different controls: `notch` above is the AUTOMATIC
  // notch (Hamlib ANF), which hunts a carrier on its own; this is the MANUAL one you
  // park on a heterodyne, and it is what most operators mean by "notch".
  { key: 'manualNotch', label: 'Manual notch', chain: 'MN', titleKey: 'phone.dsp.manualNotch.title' },
] as const satisfies readonly PhoneFunc[]

/** The two that key the transmitter's audio rather than filtering the receiver's. COMP is the
 *  speech processor (PROC on a Yaesu front panel); VOX is hands-free keying — a KEYING-SOURCE
 *  control, and deliberately NOT a stop: switching VOX off changes what keys the rig, it does
 *  not end an over in flight (THE STOP LINE, features/panelState.ts). */
const TX_FUNCS = [
  { key: 'comp', label: 'COMP', chain: 'COMP', titleKey: 'phone.dsp.comp.title' },
  { key: 'vox', label: 'VOX', chain: 'VOX', titleKey: 'phone.dsp.vox.title' },
] as const satisfies readonly PhoneFunc[]

interface PhoneFunc {
  key: 'nb' | 'nr' | 'notch' | 'manualNotch' | 'comp' | 'vox'
  label: string
  chain: string
  titleKey: MessageKey
}

/** What a stepped row needs said about it. The two WORDINGS arrive already resolved, from
 *  literal `t('phone.chain.att.*')` calls at the row's own JSX — deliberately, because a
 *  `t(table[id].aria)` lookup is invisible to `hardcoded-strings.test.ts` and its four keys
 *  read to that guard as entries nothing references (it caught exactly that here).
 *
 *  `unit` is the one behavioural difference and it is not cosmetic: an attenuator's steps
 *  are decibels, a preamp's are whatever THIS radio calls each position (`1`/`2` on an
 *  Icom), so only one of the two may ever carry dB. */
interface StepRowText {
  aria: string
  title: string
  unit: boolean
}

/** ✓rig · ⌁cmd · ⊘ — HOW WELL THIS NUMBER IS KNOWN, which on a transmit readout is half the
 *  number's meaning.
 *
 *  `rig` is a genuine read-back. `cmd` is Nexus's own command with no way to confirm it: RIT,
 *  XIT and VFO A/B are write-only and optimistic (engine.rs:5558, :18262), so an offset the
 *  operator dialled on the radio's own clarifier knob is invisible to this app entirely. `off`
 *  is "there is no value here", with the reason in the tooltip.
 *
 *  ⚠️ A COMMANDED VALUE IS NEVER RENDERED AS THOUGH IT WERE READ. That is the whole rule, and
 *  it is why the emission's mark falls to `cmd` the moment XIT is non-zero — the figure is
 *  still the engine's, but part of it is now something nobody read back.
 *
 *  ⚠️ TEXT, NEVER COLOUR ALONE. The glyph is aria-hidden decoration; the WORD beside it is
 *  what a screen reader speaks and what a monochrome display shows. */
function TruthMark({ kind, title, word }: { kind: 'rig' | 'cmd' | 'off'; title: string; word?: string }) {
  const glyph = kind === 'rig' ? '✓' : kind === 'cmd' ? '⌁' : '⊘'
  // Each key written out as a literal `t()` call rather than one call over a computed key:
  // the catalog guard extracts keys by reading the source, so a computed one is invisible to
  // it and the entry it names reads as an orphan nobody references.
  const text =
    word ?? (kind === 'off' ? t('phone.unavail.mark') : kind === 'rig' ? t('phone.truth.rig.mark') : t('phone.truth.cmd.mark'))
  return (
    <span className={`ph-truth ph-truth--${kind}`} title={title}>
      <span aria-hidden="true">{glyph}</span>
      {kind === 'off' ? ' ' : ''}
      {text}
    </span>
  )
}

/** Bandscope span presets — the width of OCCUPIED SIDEBAND to show, because the scope's axis
 *  is the rig's: RF offset from the dial, dial at the 1/9 mark and the sideband filling the
 *  rest (PhoneScope's `carrierCentered`). The engine is asked for exactly 0..width — receiver
 *  audio is one-sided — so the preset is both what is captured and 3/4 of what is shown; the
 *  remaining quarter is scopeView's guard band, which has no data by construction.
 *
 *  These were briefly labelled ±4k/±2.7k/±1.5k/±800, from the fortnight the axis was
 *  symmetric. The Hz never changed meaning; the ± did, and the labels came off with it.
 *
 *  FROM THE OLD SLICES: 'Full' (0–4000) and 'Voice' (300–2700) are unchanged in meaning.
 *  'Low' (200–1500) is now the plain 1.5k zoom. 'High' (1500–2900) has no carrier-referenced
 *  equivalent — a window that excludes the dial cannot be drawn from it — so its slot became
 *  the tighter 800 Hz zoom.
 *
 *  The chips whose face is a WIDTH ('1.5k', '800') print it from here; 'Auto', 'Full' and
 *  'Voice' are words, and every title's figure is supplied to the sentence rather than
 *  written in it. Both resolve when the row RENDERS (a module constant would freeze the
 *  first locale loaded), so a chip is keyed and compared on `id` — the preset itself rather
 *  than the word printed on it. */
const SPANS = [
  // ⭐ 'Auto' tracks the rig's reported filter width (operator, 2026-08-16: a fixed span
  // wider than the filter shows a dead stopband — on the audio feed those pixels can never
  // light). widthHz 0 is the sentinel; the render resolves it from filterWidthHz, clamped
  // 800..4000, falling back to the full 4 kHz when the rig doesn't report one.
  {
    id: 'auto',
    widthHz: 0,
    label: () => t('phone.span.auto.label'),
    title: () => t('phone.span.auto.title'),
  },
  {
    id: 'full',
    widthHz: 4000,
    label: () => t('phone.span.full.label'),
    title: () => t('phone.span.full.title', { khz: 4 }),
  },
  {
    id: 'voice',
    widthHz: 2700,
    label: () => t('phone.span.voice.label'),
    title: () => t('phone.span.voice.title', { khz: 2.7 }),
  },
  {
    id: '1.5k',
    widthHz: 1500,
    label: () => '1.5k',
    title: () => t('phone.span.zoom.title', { khz: 1.5 }),
  },
  {
    id: '800',
    widthHz: 800,
    label: () => '800',
    title: () => t('phone.span.tight.title', { hz: 800 }),
  },
] as const

/** RF panadapter zoom presets (used only when a native RF scope is streaming). Symmetric ±Hz
 *  windows centered on the dial — scopeView maps these to absolute RF and clamps to the swept
 *  span, so "Full" (a huge window) shows the rig's WHOLE sweep rather than a passband-width sliver.
 *
 *  The ± faces are measurements and stay written here; 'Full' is a word. Keyed and compared
 *  on `id` for the same reason SPANS is. */
const RF_SPANS = [
  {
    id: 'full',
    lo: -1e9,
    hi: 1e9,
    label: () => t('phone.rfZoom.full.label'),
    title: () => t('phone.rfZoom.full.title'),
  },
  {
    id: '±25k',
    lo: -25_000,
    hi: 25_000,
    label: () => '±25k',
    title: () => t('phone.rfZoom.span.title', { khz: 25 }),
  },
  {
    id: '±10k',
    lo: -10_000,
    hi: 10_000,
    label: () => '±10k',
    title: () => t('phone.rfZoom.span.title', { khz: 10 }),
  },
  {
    id: '±5k',
    lo: -5_000,
    hi: 5_000,
    label: () => '±5k',
    title: () => t('phone.rfZoom.span.title', { khz: 5 }),
  },
] as const

/** The FT-710's OWN span ladder (CAT reference, `SS` P2=5) — these command the RADIO, not a crop.
 *
 *  The operator's expectation, and it is the right one: the app's panadapter should reflect the
 *  radio's, its settings and its width. Cropping the row client-side shows a narrower window of the
 *  SAME sweep — ±5 kHz out of 200 kHz is ~42 of 850 bins stretched across the panel, which is why
 *  it looked coarse. Asking the RADIO for a 10 kHz sweep puts all 850 bins across it: 12 Hz per bin
 *  instead of 235.
 *
 *  `label` is the radio's own name for the rung (the FT-710 menu says "200 kHz", never "±100k").
 *  `halfHz` is what goes on the wire, because `setScopeSpan` was written for Icom CI-V 27 15 and its
 *  argument is ± half the sweep; the backend doubles it back. Every rung the rig has is offered —
 *  a span it cannot sweep is refused rather than rounded, so the two ladders cannot drift apart. */
const YAESU_SPANS = [
  { label: '1k', halfHz: 500 },
  { label: '2k', halfHz: 1_000 },
  { label: '5k', halfHz: 2_500 },
  { label: '10k', halfHz: 5_000 },
  { label: '20k', halfHz: 10_000 },
  { label: '50k', halfHz: 25_000 },
  { label: '100k', halfHz: 50_000 },
  { label: '200k', halfHz: 100_000 },
  { label: '500k', halfHz: 250_000 },
  { label: '1M', halfHz: 500_000 },
] as const

/** RIG scope-span presets (native Icom CI-V only) — these change the RADIO's real panadapter
 *  sweep width via CI-V 27 15 (± half-width in Hz), from the rig's own span table. Unlike the
 *  client-side RF zoom above, this commands the hardware. */
const RIG_SPANS = [
  { label: '±2.5k', hz: 2_500 },
  { label: '±5k', hz: 5_000 },
  { label: '±10k', hz: 10_000 },
  { label: '±25k', hz: 25_000 },
  { label: '±50k', hz: 50_000 },
  { label: '±100k', hz: 100_000 },
  { label: '±250k', hz: 250_000 },
] as const

/** FlexRadio pan BANDWIDTH presets (full span, not ± half-width) — command the SmartSDR
 *  panadapter's real width via `display pan set … bw=`. */
const FLEX_SPANS = [
  { label: '50k', hz: 50_000 },
  { label: '100k', hz: 100_000 },
  { label: '200k', hz: 200_000 },
  { label: '500k', hz: 500_000 },
  { label: '1M', hz: 1_000_000 },
  { label: '2M', hz: 2_000_000 },
] as const

export function PhoneCockpit({ active = true, snap, theme, pendingWork, onConsumeWork, onSnap, fieldDay, phoneMode, wheelSensitivity, spots, needByCall, typeByCall, onWorkSpot, onRecallMemory, onOpenMemories, onOpenSettings, onOpenLogbook, panels }: Props) {
  const display = useRemotePresentation()
  const quick = display?.presentation === 'quick'
  const details = !quick || display.radioDetails
  const frequencyControl = useStationCapability('frequency')
  const rotatorControl = useStationCapability('rotator')
  const scopeClick = useRemoteScopeClick(snap)
  const levels = useRadioLevels(snap)
  const dspControl = useReceiverDsp(snap, 'phone')
  const phoneModeControl = usePhoneMode(snap, phoneMode)
  const control = useStationControl()
  // A browser moves the rig scope with the station's rigScope hint; the station judges the family.
  const rigScope = useStationCapability('rigScope')
  const scopeControl = control || rigScope
  const scopeFailed = (error: unknown): void => { pushToast(controlFailureMessage(error), 'error') }
  // One remote reference change at a time: a drag sends its newest value, never every step.
  const [sendScopeRef] = useState(() => latestOnly((tenths: number) => setScopeRef(tenths), scopeFailed))
  const [sendFlexRef] = useState(() => latestOnly((dbm: number) => setFlexPanRef(dbm), scopeFailed))
  const spotsRead = useRemoteCollection('spots')
  // Live S-meter (shared 100 ms poll, lock-free backend) — used to arrive via the 300 ms
  // snapshot on top of the backend's own sampling, which read as a laggy needle. smeterDb-only
  // subscription: the cockpit re-renders when the S-meter changes, never on RX-level churn.
  const smeterDb = useSmeterDb(active)
  const [power, setPower] = useState(100) // % — only pushed to the rig once touched
  // Mirror the RIG's real level (CAT read-back / last commanded) so the slider
  // never lies at a guessed 100% — but never fight an in-flight drag.
  const dragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.rfPower
    if (rb != null && !dragging.current) {
      const pct = Math.round(rb * 100)
      setPower((p) => (Math.abs(p - pct) >= 2 ? pct : p))
    }
  }, [snap.radio.rfPower])
  const [mic, setMic] = useState(50) // % mic gain — pushed to the rig once touched
  const micDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.micGain
    if (rb != null && !micDragging.current) {
      const pct = Math.round(rb * 100)
      setMic((m) => (Math.abs(m - pct) >= 2 ? pct : m))
    }
  }, [snap.radio.micGain])
  const shownMic = control ? mic : Math.round((levels.draft('micGain') ?? snap.radio.micGain ?? 0) * 100)
  const changeMic = (pct: number) => {
    if (!levels.can('micGain')) return
    if (control) setMic(pct)
    void levels.change('micGain', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  // ── THE TX MONITOR'S LEVEL (2026-09-22) ──────────────────────────────────────────────
  // Same optimistic shape as MIC above, and deliberately NOT through `useRadioLevels`:
  // `MONITOR_GAIN` is not one of the remote protocol's `RadioLevel`s, and inventing an entry
  // for it would widen the station-operation surface a browser observer may command — a
  // trust decision, not a cockpit one. So it is LOCAL ONLY, and a remote observer gets the
  // slider disabled rather than a control that would be refused at the far end.
  const [mon, setMon] = useState(0) // % TX-monitor level — pushed to the rig once touched
  const monDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.monitorGain
    if (rb != null && !monDragging.current) {
      const pct = Math.round(rb * 100)
      setMon((m) => (Math.abs(m - pct) >= 2 ? pct : m))
    }
  }, [snap.radio.monitorGain])
  const shownMon = control ? mon : Math.round((snap.radio.monitorGain ?? 0) * 100)
  const changeMon = (pct: number) => {
    if (!control) return
    setMon(pct)
    void setMonitorGain(pct / 100)
      .then((s) => s && onSnap?.(s))
      .catch((error) => pushToast(controlFailureMessage(error), 'error'))
  }
  /**
   * ⛔ A PAD IS PICKED, NOT SET, so this sends the operator's chip and nothing else.
   *
   * `set_att_db` / `set_preamp_db` REJECT any value that is not in the radio's own step
   * list — deliberately, because a rig NAKs an unheld pad or silently substitutes its
   * neighbour, and either way the front end moves by an amount nobody chose. The chips are
   * rendered FROM that list, so a refusal here means the list changed under a press (a
   * radio handoff mid-click); it is said rather than swallowed.
   */
  const pickStep = (which: 'ATT' | 'PRE', db: number) => {
    if (!control) return
    void (which === 'ATT' ? setAttDb(db) : setPreampDb(db))
      .then((s) => s && onSnap?.(s))
      .catch((error) => pushToast(controlFailureMessage(error), 'error'))
  }
  const [nr, setNr] = useState(30) // % noise-reduction level — pushed once touched
  const nrDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.nrLevel
    if (rb != null && !nrDragging.current) {
      const pct = Math.round(rb * 100)
      setNr((n) => (Math.abs(n - pct) >= 2 ? pct : n))
    }
  }, [snap.radio.nrLevel])
  // #95's two additions, both the same shape as NR above: mirror the rig's own value so the
  // slider shows where the knob really is, and never fight an in-flight drag.
  const [comp, setComp] = useState(50)
  const compDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.compLevel
    if (rb != null && !compDragging.current) {
      const pct = Math.round(rb * 100)
      setComp((c) => (Math.abs(c - pct) >= 2 ? pct : c))
    }
  }, [snap.radio.compLevel])
  const shownComp = control ? comp : Math.round((levels.draft('compression') ?? snap.radio.compLevel ?? 0) * 100)
  const changeComp = (pct: number) => {
    if (!levels.can('compression')) return
    if (control) setComp(pct)
    void levels.change('compression', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  // HZ, not a percentage — the notch sits at an audio frequency and the operator is placing it
  // on a heterodyne by ear. A 10 Hz step is fine enough to null a tone and coarse enough to
  // sweep the passband without a hundred CAT writes.
  const [notchHz, setNotchHz] = useState(1000)
  const notchDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.notchFreqHz
    if (rb != null && !notchDragging.current) {
      const hz = Math.round(rb)
      setNotchHz((n) => (Math.abs(n - hz) >= 10 ? hz : n))
    }
  }, [snap.radio.notchFreqHz])
  const shownNotch = control ? notchHz : Math.round(levels.draft('notch') ?? snap.radio.notchFreqHz ?? 0)
  const changeNotch = (hz: number) => {
    if (!levels.can('notch')) return
    if (control) setNotchHz(hz)
    void levels.change('notch', hz).catch(error => pushToast(String(error), 'error'))
  }
  // The three analog levels, each the same shape as NR/COMP above: mirror the rig's own
  // value so the slider shows where the knob really is, and never fight an in-flight drag.
  const [af, setAf] = useState(50)
  const afDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.afGain
    if (rb != null && !afDragging.current) {
      const pct = Math.round(rb * 100)
      setAf((a) => (Math.abs(a - pct) >= 2 ? pct : a))
    }
  }, [snap.radio.afGain])
  const shownAf = control ? af : Math.round((levels.draft('afGain') ?? snap.radio.afGain ?? 0) * 100)
  const changeAf = (pct: number) => {
    if (!levels.can('afGain')) return
    if (control) setAf(pct)
    void levels.change('afGain', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  const [rfg, setRfg] = useState(100)
  const rfgDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.rfGain
    if (rb != null && !rfgDragging.current) {
      const pct = Math.round(rb * 100)
      setRfg((r) => (Math.abs(r - pct) >= 2 ? pct : r))
    }
  }, [snap.radio.rfGain])
  const shownRfg = control ? rfg : Math.round((levels.draft('rfGain') ?? snap.radio.rfGain ?? 0) * 100)
  const changeRfg = (pct: number) => {
    if (!levels.can('rfGain')) return
    if (control) setRfg(pct)
    void levels.change('rfGain', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  const [sql, setSql] = useState(0)
  const sqlDragging = useRef(false)
  useEffect(() => {
    const rb = snap.radio.squelch
    if (rb != null && !sqlDragging.current) {
      const pct = Math.round(rb * 100)
      setSql((q) => (Math.abs(q - pct) >= 2 ? pct : q))
    }
  }, [snap.radio.squelch])
  const shownSql = control ? sql : Math.round((levels.draft('squelch') ?? snap.radio.squelch ?? 0) * 100)
  const changeSql = (pct: number) => {
    if (!levels.can('squelch')) return
    if (control) setSql(pct)
    void levels.change('squelch', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  const shownNr = control ? nr : Math.round((levels.draft('nr') ?? snap.radio.nrLevel ?? 0) * 100)
  const changeNr = (pct: number) => {
    if (!levels.can('nr')) return
    if (control) setNr(pct)
    void levels.change('nr', pct / 100).catch(error => pushToast(String(error), 'error'))
  }
  // AGC speed — the chip lights on the click, then the rig gets the last word. Reading
  // snap.radio.agc directly lagged ~0.75–1.5 s: setAgc's snapshot returns the OLD rig read-back
  // (rig_agc.or(agc)) until the next RX poll, so the clicked chip wouldn't light up. DERIVED
  // rather than a remembered mirror, because a mirror synced only "when the snapshot changes"
  // is blind to a REFUSAL — a rig whose Hamlib backend lacks that AGC step answers RPRT -1, the
  // read-back never moves, and the chip claimed a speed the radio never took.
  const [agcPick, setAgcPick] = useState<string | null>(null)
  const agc =
    control && agcPick != null && agcPick !== snap.radio.refusedAgc ? agcPick : (snap.radio.agc ?? null)
  const changeAgc = (sp: 'auto' | 'fast' | 'mid' | 'slow' | 'off') => {
    if (!dspControl.canAgc) return
    if (control) setAgcPick(sp)
    void dspControl.changeAgc(sp)
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
  }
  // Native Icom scope reference level, in tenths of a dB (−200..+200 = −20.0..+20.0 dB).
  const [scopeRefTenths, setScopeRefTenths] = useState(0)
  const changeScopeRef = (tenths: number) => {
    if (!scopeControl) return
    setScopeRefTenths(tenths)
    if (control) void setScopeRef(tenths)
    else sendScopeRef(tenths)
  }
  const [keyed, setKeyed] = useState(false)
  /** Mirrors `keyed` for the window key handlers, which capture their closure once per
   *  effect run — the release must act on what is TRUE when it fires, not on what was true
   *  when it was bound. See the space-release comment below. */
  const keyedRef = useRef(false)
  useEffect(() => {
    keyedRef.current = keyed
  }, [keyed])
  // Bandscope span (audio-window zoom within the captured passband — this is
  // soundcard audio, not RF IQ, so "span" means which slice of the passband
  // fills the scope).
  const [span, setSpan] = useState<(typeof SPANS)[number]>(SPANS[0])
  const [rfSpan, setRfSpan] = useState<(typeof RF_SPANS)[number]>(RF_SPANS[0])
  // Live scope feed (reported by PhoneScope) — keeps the "RX audio" label honest when a
  // native RF panadapter is driving the scope (show the real RF span instead).
  const [scopeFeed, setScopeFeed] = useState<{ source: string; loHz: number; hiHz: number } | null>(
    null,
  )
  // True while a native RF panadapter (Flex/Icom CI-V) is actually streaming the scope. Drives
  // the whole panel's identity: when the rig's real RF spectrum is live we drop the audio-passband
  // framing (the "RX audio" label and the audio-Hz span chips) so the operator sees ONE unambiguous
  // display — the panadapter — instead of RF spectrum wrapped in audio-passband chrome.
  const nativeRf = scopeFeed != null && isRfScopeSource(scopeFeed.source)
  // The FT-710 is the one native scope whose SPAN this app can command over plain CAT, so its
  // control row is the rig's own ladder rather than a client-side crop. Icom/Flex keep the crop:
  // their hardware span already has its own row (RIG_SPANS / FLEX_SPANS) further down.
  // TWO DIFFERENT QUESTIONS, and conflating them cost the operator the only way out of FIX.
  //
  // `yaesuScope` — does this radio have a scope Nexus is talking to? True as soon as the mode code
  // has been read over CAT, which happens whether or not the sweep can be PLACED. The controls hang
  // off this.
  // `yaesuRf` — are RF rows arriving right now? The view bounds hang off this, because when the feed
  // falls back to sound-card audio the axis really is audio.
  //
  // Gating the controls on the feed made them vanish exactly when they were needed: in FIX with no
  // start stated no rows flow, so the panadapter block unmounted — and the position select went
  // with it, leaving no way to get back to Center and no way to see why the panel had emptied.
  //
  // The original wording here justified that by a "FIX starts here" button being taken away with
  // the block. There is no such button: the chip-row input that would have driven one was removed
  // once the band-edge derivation proved right on the air (see `RadioProfile::yaesu_fix_starts`),
  // and `Engine::set_yaesu_fix_start` still has no caller. The reason to keep the controls mounted
  // survives that — it is the position select, not a start button, that must not disappear.
  const yaesuScope = snap.radio.scopeModeCode != null
  const yaesuRf = scopeFeed?.source === 'yaesu'
  // What the radio reports, so the two selects show the rig's state rather than a local guess.
  // `scopeModeCode` is the `SS` P3 byte widened for JSON; an unknown code shows as Center, which is
  // the only position this app can place anyway.
  const yaesuPosition: 'center' | 'cursor' | 'fix' = (() => {
    switch (snap.radio.scopeModeCode ?? 0x34) {
      case 0x31: case 0x36: case 0x37: return 'cursor'
      case 0x32: case 0x39: case 0x41: return 'fix'
      default: return 'center'
    }
  })()
  // The span the radio is sweeping, matched back onto the ladder for the <select>'s value.
  const yaesuSpanLabel =
    YAESU_SPANS.find((sp) => scopeFeed != null && Math.abs((scopeFeed.hiHz - scopeFeed.loHz) - sp.halfHz * 2) < sp.halfHz * 0.1)?.label ??
    YAESU_SPANS[7].label

  // True only when the rig's own Icom scope is streaming (span/ref are Icom CI-V commands; the
  // Flex panadapter has a different control path, so gate on 'civ' specifically, not any RF feed).
  const civScope = scopeFeed?.source === 'civ'
  // FlexRadio SmartSDR panadapter — its own span/ref command path (display pan set …).
  const flexScope = scopeFeed?.source === 'flex'
  const [flexRefDbm, setFlexRefDbm] = useState(-80)
  const changeFlexRef = (dbm: number) => {
    if (!scopeControl) return
    setFlexRefDbm(dbm)
    if (!control) return sendFlexRef(dbm)
    void setFlexPanRef(dbm)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  const [lock, setLock] = useState(false) // hands-free PTT (toggle instead of hold)
  const [recBusy, setRecBusy] = useState(false) // in-flight guard for the record toggle
  const [spotOpen, setSpotOpen] = useState(false) // spot-to-cluster popup
  const [spotCall, setSpotCall] = useState('') // seed: '' from the toolbar, the typed call from LogEntry
  // The call the operator is working RIGHT NOW, reported up by the log strip on every change.
  // ⭐ Not `spotCall`, which is only captured when Spot is pressed and is stale or empty the rest
  // of the time — a Beam button reading it would point at the last station spotted.
  //
  // ⚠️ REPORT DIRECTION ONLY: `onCallChange` without `cwLive`. RttyCockpit passes BOTH halves
  // because a decoder fills its box and the strip has to take that fill; nothing fills Phone's
  // call but the operator's own typing, so the fill half has no source here. Passing `cwLive`
  // anyway would ACTIVATE LogEntry's clear-on-empty branch (`logCall` is set to '' when the
  // host's box goes empty) against a host that has no box — see the test below.
  const [workedCall, setWorkedCall] = useState('')
  // Wheel-to-tune over the bandscope, sharing the tuning strip's step selector.
  // Tuning step, persisted per cockpit ('nexus.phone.tuneStep'): the cockpit unmounts on every
  // mode switch, so plain state reset the step to 100 Hz on each round-trip (FTDX10
  // report 2026-07-21 — the third remount-state-loss bug today). localStorage so a
  // CW operator's 10 Hz survives restarts too, like nexus.cw.sensitivity.
  const [tuneStep, setTuneStep] = useState(() => {
    try {
      const v = Number(localStorage.getItem('nexus.phone.tuneStep'))
      return Number.isFinite(v) && v > 0 ? v : 100
    } catch {
      return 100
    }
  })
  useEffect(() => {
    try {
      localStorage.setItem('nexus.phone.tuneStep', String(tuneStep))
    } catch {
      /* ignore */
    }
  }, [tuneStep])
  // The last output power the rig actually measured — see the transmit contract below.
  const lastPoW = useRef<number | null>(null)
  const scopeRef = useRef<HTMLDivElement>(null)
  // Cockpit root: the scope-height splitter measures + writes its CSS var here.
  const cockpitRef = useRef<HTMLElement>(null)
  useWheelTune(scopeRef, {
    remoteFrequency: true,
    radioId: snap.activeRadioId,
    dialMhz: snap.radio.dialMhz,
    sideband: snap.radio.sideband || 'USB',
    enabled: snap.radio.catOk === true && !snap.radio.txBusyReason && !snap.radio.transmitting,
    stepHz: tuneStep,
    sensitivity: wheelSensitivity,
    onSnap,
  })

  // AUTO sideband from the rig-mode policy — FM when the FM sub-mode is selected, else sideband
  // by band (LSB <10 MHz, USB above). The operator can override this transiently (below).
  const sidebandAuto =
    phoneMode?.toLowerCase() === 'fm' ? 'FM' : snap.radio.dialMhz < 10 ? 'LSB' : 'USB'
  // Transient operator override ("USB"/"LSB"/"FM") or null = AUTO. The COMMANDED mode (canonical
  // for TX/logging + what the rig is set to) is the override when set, else the band-auto sideband.
  const modeOverride = snap.radio.sidebandOverride ?? null
  const commandedMode = modeOverride ?? sidebandAuto
  const pickMode = (m: 'USB' | 'LSB' | 'FM' | 'AM' | null) => {
    if (!phoneModeControl.canPick(m)) return
    void phoneModeControl.pick(m)
      .then((s) => s && onSnap?.(s))
      .catch(() => pushToast(t('phone.mode.failed'), 'error'))
  }
  // Whether the app can actually control the rig. Without CAT (VOX/serial PTT) the dial +
  // mode can't be set or read back — surface that so it's clear, not silently broken.
  const catOk = snap.radio.catOk === true

  // Click/drag tuning from the bandscope (Flex-style): clicks command immediately, a
  // drag coalesces to one CAT write per ~120 ms. PhoneScope does the signal-snap math
  // and reports the final dial; this hook just commands it.
  const onScopeTune = useScopeTune({
    sideband: commandedMode,
    enabled: control && catOk && !snap.radio.txBusyReason && !snap.radio.transmitting,
    onSnap,
  })

  // RX filter / passband width — the rig's read-back (null = unknown/default). The ± stepper
  // nudges it 100 Hz within a sane SSB/CW span, seeded from the current value or a 2.4 kHz default.
  const filterControl = useReceiverFilter(snap, 'phone')
  const filterHz = snap.radio.filterWidthHz ?? null
  const bumpFilter = (deltaHz: number) => {
    if (!filterControl.allowed) return
    const base = filterHz ?? 2400
    const next = Math.min(4000, Math.max(300, base + deltaHz))
    // Never let the clamp invert the direction ("wider" must not narrow at the rails).
    if ((deltaHz > 0 && next <= base) || (deltaHz < 0 && next >= base)) return
    void filterControl.setWidth(next)
      .then((s) => s && onSnap?.(s))
      .catch(() => pushToast(t('phone.filter.failed'), 'error'))
  }

  // Rig's actual mode read back over CAT. `commandedMode` stays canonical for TX; this is what
  // flags when the rig's mode disagrees, so the badge never silently lies — and, since the
  // 2026-09-10 AM report below, it is also what the LOG believes.
  const rigMode = (snap.radio.rigMode ?? '').toUpperCase()
  // Collapse ONLY the FM variants (FMN/WFM → FM). Deliberately do NOT strip PKT/data suffixes:
  // in Phone a rig stuck in PKTUSB / DATA-U (rear-jack audio → dead mic) vs a commanded USB is a
  // REAL operational mismatch worth flagging, not a cosmetic naming variant.
  const rigFamily = /^W?FM/.test(rigMode) ? 'FM' : rigMode
  const modeMismatch = catOk && rigMode !== '' && rigFamily !== commandedMode ? rigMode : null

  // ⭐ WHAT THE RIG IS ACTUALLY ON — and the mode the QSO is LOGGED as.
  //
  // The operator held an AM contact on 14.286 (the 20 m AM calling frequency) and Nexus wrote it
  // to the logbook as SSB. The log strip was handed `commandedMode === 'FM' ? 'FM' : 'SSB'`, a
  // binary choice in which everything that is not FM is SSB — so it could not say AM even when
  // the operator had PICKED AM, and it never asked the rig at all. The screenshot showed both
  // halves at once: `rig: AM` in the header, "Logs to the shared logbook as SSB" below it. Nexus
  // knew the answer and logged something else.
  //
  // THE RIG WINS OVER THE COMMAND, and that direction is the point: the emission that went out is
  // the rig's, not the one Nexus asked for. It fixes the reported case (the rig put into AM at the
  // radio, which is the only way to work AM on 20 m — the picker does not offer it there), and it
  // is also the safe direction for the opposite error, because a pick the rig did not take logs as
  // what the rig DID transmit. Mislabelling an SSB contact AM is exactly as bad as the report.
  //
  // A read-back is believed only on the same terms the mismatch chip above is shown on, so the
  // badge the operator SEES and the mode Nexus WRITES can never disagree: `catOk`, and a non-empty
  // `rigMode`. That is not a loose gate — the engine sets `rig_mode` only from a successful
  // `read_mode_passband()`, and CLEARS it both when the app commands a new dial/mode (so a cached
  // Hamlib `m` cannot answer for the command in flight) and when the link drops. A stale read
  // cannot outlive either event; the only residual window is a mode knob turned at the radio
  // within the last few polls, which is a race with the QSO itself and not a systematic lie.
  //
  // A rig outside the phone family (CW, RTTY, PKTUSB…) names no mode this strip can log FROM, so
  // it keeps the commanded mode — unchanged behaviour for a state the chip is already shouting
  // about, and the log strip's own manual override is there for a genuine cross-mode contact.
  // The read-back, once it has passed both gates and names a phone mode — `null` when it does
  // not, which is the single "the rig did not tell us" answer both lines below read.
  const rigReadPhoneMode = catOk && phoneAdifMode(rigFamily) !== null ? rigFamily : null
  const observedMode = rigReadPhoneMode ?? commandedMode

  // ⭐ AND THE SIDEBAND IS PART OF THE CONTACT (operator report, 2026-09-15).
  //
  // The paragraph above fixed WHICH phone mode gets logged. It still could not say which
  // SIDEBAND, because it logged the ADIF *Mode* and USB/LSB are ADIF SUBMODEs of SSB — so
  // every phone QSO was written as plain SSB and NOTHING Nexus wrote recorded upper or lower:
  // not the Logbook, not the export, not the QRZ/LoTW/ClubLog/eQSL push. What a given site
  // then DISPLAYS for a submode-less SSB record is its own business and is not asserted here
  // (the operator sees "USB" on QRZ for all of them); the defect on this side is that the
  // sideband was never recorded at all, while HRD and the loggers he compares against track
  // U/L.
  //
  // The record's mode label is now the sideband itself, and `logbook::adif_submode` turns it
  // into the correct ADIF `<MODE:3>SSB<SUBMODE:3>LSB` on the way out — the MODE field does NOT
  // change, so the closed Mode enumeration that keeps USB/LSB out of `LOG_MODES` is untouched.
  //
  // ⛔ THE SIDEBAND COMES FROM THE RIG OR NOT AT ALL, and that is the whole gate. The fallback
  // this line does NOT use is `commandedMode`, whose AUTO face is `dialMhz < 10 ? LSB : USB` —
  // a BAND DEFAULT. Writing that into a permanent record would be inventing the operator's
  // sideband from the band, which is exactly the claim nobody can check later: the rig may have
  // been on the other one all along. Without CAT, or with the rig sitting in CW/PKTUSB where
  // the read-back names no phone mode, there IS no evidence of a sideband and the contact logs
  // as plain SSB — unchanged from before, and no worse than the report. AM and FM are modes in
  // their own right, not sidebands, and ride through this line as themselves.
  const logMode = rigReadPhoneMode ?? phoneAdifMode(commandedMode) ?? 'SSB'

  // ⚠️ THE MIC IS DEAD AND ONLY THIS SCREEN CAN SAY SO (2026-08-17 Flex audit, critic gap #6).
  // Native Flex DAX audio sends `transmit set dax=1`, which is a RADIO-WIDE setting: while it
  // stands, the Flex's modulator takes its audio from DAX and ignores the physical microphone —
  // on every slice, in every program, SmartSDR's own MOX included. So the operator who switched
  // native audio on for FT8 and then picks up the mic to work someone on SSB transmits silence,
  // and until now nothing anywhere told them. The digital screens don't care (they feed the
  // modulator over DAX, which is the point); Phone is where the harm lands, so Phone is where it
  // is said. Same chip vocabulary as the mode-mismatch pill beside it — no new pane, no
  // structural size of its own (cockpit-panes.css owns those).
  const micOffForDax = snap.radio.flexDaxTx === true

  // Live snapshot ref so the spacebar PTT handler (bound on `lock` changes, not every render)
  // reads the CURRENT TX-allowed privilege state through key() — not whatever existed when bound.
  const snapRef = useRef(snap)
  snapRef.current = snap
  const key = (on: boolean) => {
    if (!control) return
    // Don't key (or show ON-AIR) outside license privileges — the engine blocks it anyway.
    if (on && !snapRef.current.radio.txAllowed) {
      pushToast(t('phone.tx.locked'), 'info', 3500)
      return
    }
    // TX SWITCHED OFF IS A SECOND, SEPARATE REFUSAL, and until #81 it was a SILENT one.
    // `Engine::set_ptt` is `on && tx_enabled && tx_allowed()` — two conditions — and this
    // cockpit only ever showed the second. With TX off the click went to the wire, the wire
    // discarded it, and `setKeyed(true)` below had ALREADY run: the button lit up red and read
    // "ON AIR — release to stop" over a transmitter that was not keyed. From the operator's
    // chair that is indistinguishable from a dead PTT line, which is how #81 was reported
    // (FTdx10 over USB, nothing wrong with the rig). Stop TX, the TX watchdog and a UDP HaltTx
    // all leave exactly this state.
    //
    // So: say which switch is down, and PUT IT BACK UP. Phone has no other Enable-Tx
    // affordance on screen — App hides the TopBar's TX cluster in this view, and the header's
    // TX pill is display-only here (CockpitHeader arms only when it is passed onSetTxEnabled,
    // which Phone deliberately does not: doing so would replace the ▲ TX on-air pill with an
    // arm button mid-over, since `transmitting` is the FT slot flag alone) — so a message that
    // named a switch would name one that is not there, and he would stay stuck until he
    // navigated out of Phone and back. Arming keys NOTHING by itself, and it is no more than
    // entering Phone already does (`set_operating_mode` arms TX for the manual modes); the
    // rig keys on the NEXT press, through the ordinary path below.
    //
    // ⚠️ THE ENGINE'S GATE IS UNTOUCHED. This is what the operator is TOLD, not what he is
    // allowed: set_ptt still refuses while tx_enabled is false, and TX is still only ever
    // armed by an explicit operator action.
    if (on && !snapRef.current.radio.txEnabled) {
      pushToast(t('phone.tx.turnedBackOn'), 'info', 4000)
      void setTxEnabled(true)
        .then((s) => onSnap?.(s))
        .catch(() => {})
      return
    }
    setKeyed(on)
    void setPtt(on)
  }
  const onPttDown = () => {
    if (!control) return
    if (lock) {
      key(!keyed) // hands-free: toggle
    } else {
      key(true)
    }
  }
  const onPttUp = () => {
    if (!control) return
    if (!lock) key(false)
  }
  const changePower = (pct: number) => {
    if (!control) return
    setPower(pct)
    void setRfPower(pct / 100)
  }
  // Commit a typed dial from the shared header readout — same CAT path as the
  // TuningStrip nudge/wheel (keeps the current sideband so an in-band entry
  // never flips the mode); rejects out-of-plan frequencies with a toast.
  const commitDial = (mhz: number) => {
    if (!frequencyControl) return
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13), so a typed WWV/shortwave/inter-band frequency tunes there. This used to toast
    // "outside the band plan" and discard the entry.
    // In-band keeps the current sideband; crossing 10 MHz follows the band convention (#45).
    void setFrequency(mhz, bandLabelForMhz(mhz), sidebandForQsy(mhz, snap.radio.dialMhz, snap.radio.sideband))
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
  }

  // QSO recording (audio bridge): a session-level toggle driven by the snapshot (so the REC
  // badge survives nav + multi-window). Apply the returned snapshot immediately (no ~300 ms
  // poll lag) and guard re-entry so a rapid double-click can't double-fire.
  const recording = snap.radio.qsoRecording
  const toggleRecord = () => {
    if (!control) return
    if (recBusy) return
    setRecBusy(true)
    const fn = recording ? stopQsoRecording : startQsoRecording
    fn()
      .then((s) => onSnap?.(s))
      .catch(() =>
        pushToast(recording ? t('phone.record.stopFailed') : t('phone.record.startFailed'), 'error'),
      )
      .finally(() => setRecBusy(false))
  }

  // Spacebar = push-to-talk (hold), unless typing in a field.
  useEffect(() => {
    if (!control) return // observation owns no PTT; mounting/leaving it cannot unkey the station
    const isField = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !e.repeat && !isField(e.target) && !lock) {
        e.preventDefault()
        key(true)
      }
    }
    // ⚠️ THE RELEASE ASKS ONLY WHETHER WE ARE KEYED. It used to carry the same
    // `!isField` guard as the press, on the reasoning that nothing here moves focus by
    // itself — but the OPERATOR does: hold the space bar to talk, click into the log strip
    // while still holding, release, and the keyup now targets an INPUT. The old guard
    // returned, `key(false)` never fired, and the rig stayed keyed with the button still
    // reading "release to stop", which is exactly what had just been done. An unkey a guard
    // can swallow is a stuck transmitter. The PRESS keeps its guard — a space typed into a
    // field must never start a transmission — and the asymmetry is the whole point.
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space' && keyedRef.current && !lock) {
        e.preventDefault()
        key(false)
      }
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      void setPtt(false) // safety: never leave the rig keyed on unmount
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lock])

  // Field Day exchange the operator reads aloud (and the string to record into a voice-keyer
  // slot) — what the SESSION is composing, which is what the next contact will hear. Empty
  // until FD setup fills it in.
  const fdExchange = fieldDay?.composingText || composingText(fieldDay?.composing)

  // ── THE PANE REGION (2026-07-30 layout assessment, design3 §3) ─────────────────────
  // Every operator-content block under the scope renders through a CockpitPaneFrame in
  // ONE .cockpit-panes grid; the ⊞ Panels 'removed' gating is unchanged (shown()).
  //
  // "TX CHROME NEVER ENTERS THE REGION" USED TO BE WRITTEN HERE AND IS FALSE: the voiceKeyer
  // pane transmits (F1–F6 key the rig) and hosts a ■ Stop of its own, and it renders inside
  // .cockpit-panes with a ⊞ id. What is true is THE STOP LINE (features/panelState.ts): the
  // controls this cockpit's census rests on stay out of the region — the PTT row lives in the
  // pinned .cockpit-txdock, Stop TX and Tune in the header — and none of them has an id in the
  // pane vocabulary, so moving or hiding one is unrepresentable. Whether a pane transmits has
  // no bearing on whether it may be hidden.
  //
  // DSP funcs: only those the rig actually reports (non-null) render — capability-gated,
  // no dead buttons. STICKY ONCE SEEN: `null` means BOTH "this rig lacks the func" and
  // "we don't know right now", and the radio loop clears every reading to null on a CAT
  // re-confirmation — which a QSY triggers. Gating purely on the live value therefore
  // made the whole DSP group vanish the moment you clicked a spot, the region reflow,
  // and Band Activity jump (operator report 2026-07-25). Once a rig has reported a func
  // we keep showing it; a rig that never reports one still shows nothing.
  // Learned only while the pane is SHOWN — the ⊞-hidden state must not keep learning
  // (that is the pre-rebuild behaviour: re-showing a hidden pane renders what it knew
  // when it was hidden, and nothing else silently widens the column budget meanwhile).
  // ⚠️ THE DECODE-AUDIO HAZARD — the one way these two controls can break something the
  // operator is not looking at.
  //
  // WHAT IS TRUE. On a station whose soundcard is fed from the rig's SPEAKER or HEADPHONE
  // jack — a SignaLink, a Rigblaster, a bare 3.5 mm lead, which is how most radios older
  // than a built-in USB codec are wired — the audio the decoder hears passes through the AF
  // gain control. Drop AF to silence the room and FT8/RTTY/PSK stop decoding, with nothing
  // on screen saying why. A rig's own USB codec and a fixed-level ACC/DATA jack are tapped
  // ahead of AF and are NOT affected. Squelch is the worse of the two: on most rigs a closed
  // squelch mutes the AF path including the USB/data tap, which is why every digital-mode
  // guide says to open it fully on SSB.
  //
  // WHY THE WARNING IS NOT CONDITIONAL ON THE AUDIO PATH. Nexus cannot tell those wirings
  // apart. `audio_in` is a capture-device NAME, and a SignaLink enumerates as "USB Audio
  // CODEC" exactly like a rig's internal one — sniffing the name would be a proxy for the
  // thing, and a wrong one. So the control carries the warning and the operator, who can see
  // their own cabling, decides.
  //
  // IT NOTIFIES AND NEVER ACTS: nothing here clamps, refuses or restores a level. A zero AF
  // is a legitimate thing to want (headphones out, listening through the computer), and an
  // operator who means it must be able to have it.
  const DECODE_MUTE_PCT = 5
  const afMutesDecode = snap.radio.afGain != null && Math.round(snap.radio.afGain * 100) <= DECODE_MUTE_PCT
  // ON FM A RAISED SQUELCH IS CORRECT OPERATING and warning about it would be noise, so the
  // FM case is excluded rather than merely ranked lower.
  //
  // ⛔ **THE RIG WINS OVER THE COMMAND HERE TOO** (operator review of 1.15.0, finding T1).
  // This read `commandedMode`, so on the ONE station where the warning matters — a rig
  // sitting on a mode Nexus did not command — it stayed silent over a squelch that had
  // muted the decoder. The header is already showing the `rig: USB` mismatch chip at that
  // moment. It cuts both ways: a rig moved to FM at the knob raised a spurious DECODE? over
  // a squelch that was correct for the mode it was actually on. `observedMode` is the
  // rig-first answer built for exactly this rule under this file's "THE RIG WINS OVER THE
  // COMMAND" note, and it falls back to the command when there is no read-back.
  const squelchMutesDecode = snap.radio.squelch != null &&
    Math.round(snap.radio.squelch * 100) >= DECODE_MUTE_PCT && observedMode !== 'FM'
  // ⭐ DOES THIS RIG DRIVE THIS CONTROL — and it is STICKY, which is the half that is easy
  // to drop and expensive to lose.
  //
  // `null` means BOTH "this rig lacks it" and "we don't know right now": the radio loop
  // clears every reading to null on a CAT re-confirmation, which a QSY triggers. Reading the
  // live value alone used to make the whole DSP group VANISH the moment you clicked a spot
  // (operator report, 2026-07-25). Since the 2026-09-20 ruling nothing vanishes any more —
  // but without the sticky the controls would go dead-and-⊘ under the operator's hand for a
  // poll or two on every QSY, which is the same lie in a new costume. So: once a rig has
  // reported a field this session, it keeps the control.
  //
  // ⚠️ IT LEARNS UNCONDITIONALLY NOW. The old pair of refs learned only while their pane was
  // SHOWN, and the reason was the column budget — a hidden pane must not silently widen it.
  // Nothing here feeds a column budget any longer (both chain panes always render), and a
  // ⊞-hidden pane that forgot what the rig could do would come back dead.
  //
  // ⚠️ AND IT COVERS EVERY FIELD, closing the gap #95 left: `compLevel` and `notchFreqHz` had
  // no sticky at all, so the two controls that report went away on a QSY while NB/NR stayed.
  const seenFields = useRef(new Set<string>())
  // ⚠️ …BUT ONE RIG CANNOT VOUCH FOR ANOTHER. A handoff to a second radio is not a momentary
  // null, and this cockpit does NOT remount on one — so without this the new radio would
  // inherit the old one's capabilities and wear live-looking controls it has nothing behind,
  // which is the exact failure the ⊘ exists to prevent, arriving through the back door. The
  // two refs this replaces had the same hole; widening the memory to every field is what
  // makes it worth closing.
  const seenRadio = useRef(snap.activeRadioId)
  if (seenRadio.current !== snap.activeRadioId) {
    seenFields.current = new Set()
    seenRadio.current = snap.activeRadioId
  }
  /** Reported now, or reported at some point on THIS radio. Mutating during render matches
   *  what the two refs this replaces already did, and for the same reason: the answer has to
   *  be available to the very render that asks. */
  const reports = (field: string): boolean => {
    if ((snap.radio as unknown as Record<string, unknown>)[field] != null) seenFields.current.add(field)
    return seenFields.current.has(field)
  }
  // ⊞ Panels. `main`/`side` are unused here (Phone has no two-column pane grid), so the
  // host is only supplying `shown` + the menu items.
  //
  // Three notes, of two different kinds — see panelHost's `notes` doc, which is why they
  // share one field.
  //
  // TWO ARE "nothing on screen behind this box" — the dead-checkbox the operator hit on
  // 2026-08-03. The rig-scope pane needs the RADIO's own panadapter streaming; TX Meters
  // mounts fine and reads only while keyed.
  //
  // ⭐ THE TWO DSP NOTES ARE GONE WITH THE PANES THEY DESCRIBED. `receiver` and
  // `transmitter` always have something on screen — that is the whole point of the 2026-09-20
  // ruling — so an availability note on either would be false. What used to be one note for a
  // whole empty pane is now one ⊘ mark per control that cannot be driven, which says the same
  // thing about strictly more of the radio.
  //
  // THE FIFTH IS A CONSEQUENCE. Unticking Voice Keyer ends a message and discards a
  // recording. Not the price of its being hideable — nothing had to buy that (THE STOP LINE:
  // neither being a sender nor hosting a ■ Stop of its own has any bearing on it) — but a
  // stop the operator did not ask for reads as a dropout, so it is said before the tick.
  // That note is required, not decorative:
  // PhoneCockpit.keyerHide.test.tsx hides every id here, asks the wire which hides stopped
  // something, and requires exactly those entries to carry one. The menu's Undo button
  // reaches the same hide and carries the same consequence (VOICE_KEYER_UNDO_ENDS); Reset
  // cannot reach it at all.
  //
  // The notes explain; every box stays the operator's.
  const host = panels
    ? panelHost(panels, {
        menu: PHONE_PANEL_IDS,
        side: [],
        main: 'bandActivity',
        labels: phonePanelLabels(),
        notes: {
          rigscope: civScope || flexScope ? undefined : NO_NATIVE_SCOPE_REASON,
          txmeters: TX_METERS_WHEN,
        },
        // The ONE hide in the app that ends something in flight. In `endsOnHide` rather
        // than `notes` so the pane's own ✕ carries the same sentence the ⊞ entry prints —
        // one wording, two doors (panelHost).
        endsOnHide: { voiceKeyer: VOICE_KEYER_STOPS_ON_HIDE },
      })
    : null
  const shown = (id: PhonePanelId) => (host ? host.shown(id) : true)
  // The pane's own ✕ — the SAME setPanelState the ⊞ tick makes. `{}` with no panel record
  // (the remote observer), so the button simply is not there rather than being dead.
  const closeProps = (id: PhonePanelId) => (host ? host.closeProps(id) : {})

  // Which panes CAN render right now — the exact conditions that gate them in the JSX,
  // hoisted because the region's column budget (maxCols) depends on them.
  //
  // ⚠️ THE TWO CHAIN PANES HAVE NO CAPABILITY GATE, and that is the ruling rather than an
  // omission: `shown(id)` is the operator's tick and nothing else may take a pane away.
  const hasBandPane = onWorkSpot != null && shown('bandActivity')
  const hasKeyerPane = shown('voiceKeyer')
  const hasRigScopePane = shown('rigscope') && (civScope || flexScope)
  const hasReceiverPane = shown('receiver')
  const hasTransmitterPane = shown('transmitter')
  const auxPresent = hasRigScopePane || hasReceiverPane || hasTransmitterPane
  // Everything the LEADING column can hold below the 3-col tier. The keyer used to make
  // this unconditionally true; now that it has a ⊞ entry, a rig with no DSP and no native
  // scope can have the operator untick its way to an empty leading track — a `minmax(0,1fr)`
  // column holding nothing beside the log, which is the "band of empty black" this region
  // was rebuilt to kill. So the column is not rendered when it is empty, and maxCols
  // collapses with it — a bounded tier (2/3, `overflow:hidden`) never gets a track with
  // nothing in it.
  const leadPresent = hasBandPane || hasKeyerPane || auxPresent
  // Three columns are band | keyer+rig/dsp | log, so the tier is only offered when the
  // leading column (Band Activity) AND at least one aux pane exist — otherwise a track
  // would sit empty, the operator's "band of empty black" rebuilt. This is the same
  // collapse panelHost ships as dataCols 'one'|'two' for Operate; it feeds maxCols and
  // is NEVER stamped on the region (useRegionCols owns data-cols='1|2|3').
  const { ref: panesRef, cols } = useRegionCols<HTMLDivElement>(
    auxPresent && hasBandPane ? 3 : leadPresent ? 2 : 1,
  )

  // ── WHAT THIS RIG DRIVES, one boolean per control ────────────────────────────────────
  // Read through `reports`, so each one is "reporting now, or reported at some point this
  // session" and a QSY's momentary null cannot kill a live control. These are CAPABILITY
  // only; the PERMISSION half (a remote observer, a lease that is not ours) stays with
  // `levels.can(…)` / `dspControl.can…` at each control, and both disable it.
  // ── THE REGISTRY ANSWERS FOR EVERY CONTROL ────────────────────────────────────────
  // One table decides whether a control may be driven, which of several causes is the one
  // to NAME, and whether it gets a row at all (features/rigControls.ts). Thirteen inline
  // ternaries used to do this, each free to drift.
  const caps: RigCapsDto = {
    attDb: snap.radio.attStepsDb ?? undefined,
    preampDb: snap.radio.preampStepsDb ?? undefined,
  }
  const chainState: ControlState = {
    catOk,
    // The STICKY answer, never the raw `!= null` — a QSY blanks every reading, and the
    // registry must never read that flicker as a radio losing a feature.
    reported: (c: RigControl) => (c.field ? reports(c.field) : false),
    mode: commandedMode,
    // ⭐ THE FIRST CAPS THE REGISTRY HAS EVER BEEN GIVEN (2026-09-22), and only the half the
    // backend actually ships: `attStepsDb` / `preampStepsDb`, the pads and preamps this
    // radio DECLARED in its own `\dump_state` (`Engine::observe_rig_db_steps`, read once per
    // CAT confirmation). The func/level masks are still a backend job that has not landed,
    // so they stay absent and every OTHER control falls through to the sticky above exactly
    // as before — this widens nothing but ATT and PRE.
    //
    // ⚠️ `?? undefined` IS LOAD-BEARING. The DTO's `null` and its absence both mean "the
    // radio never told us", and `capStateFor` reads `undefined` as UNKNOWN — passing `null`
    // through would hit the `!steps` branch as well but by accident of falsiness, and the
    // three states are too close together to leave one resting on that.
    caps,
  }
  const control_ = (id: string) => RIG_CONTROLS.find((c) => c.id === id)!
  /** Can this control be driven right now? CAPABILITY only — the permission half (a remote
   *  observer, a lease that is not ours) stays with `levels.can(…)` at each control. */
  const dead = (id: string) => causeFor(control_(id), chainState) !== null
  const show = (id: string) => rendersRow(control_(id), chainState)
  /** One id per pane, so a row can point `aria-describedby` at the banner that explains it
   *  rather than repeating one sentence thirteen times. */
  const NOCAT = { rx: 'ph-nocat-rx', tx: 'ph-nocat-tx' } as const
  const describedBy = (chain: 'rx' | 'tx') => (catOk ? undefined : NOCAT[chain])
  /** The pane-level banner: ONE sentence for a fact true of every control in the pane at
   *  once. It is also why no row carries a ⊘ for a dead link. */
  const noCatBanner = (chain: 'rx' | 'tx') =>
    catOk ? null : (
      <p className="ph-chain-banner" id={NOCAT[chain]} role="note">
        {t('phone.chain.noCat')}
      </p>
    )
  /** ⛔ THE COLLAPSE. Controls this radio does not have do not each get a grey row — their
   *  plates go in ONE line at the pane's foot, so nothing vanishes silently, the operator
   *  sees that Nexus knows the control exists, and an IC-7300 does not open to four dead
   *  rows. A control Nexus has built no path for appears in neither place. */
  const absentLine = (chain: 'rx' | 'tx') => {
    const plates = absentPlates(chain, chainState)
    return plates.length === 0 ? null : (
      <p className="ph-chain-absent" role="note">
        {t('phone.chain.absent', { plates: plates.join(' · ') })}
      </p>
    )
  }
  // ⛔ THE FM REPEATER SHIFT — the one thing `tx_emission_mhz` does NOT fold in, verified
  // rather than assumed: `Engine::tx_emission_mhz` (engine.rs:17677) is `verdict base +
  // xit_offset` and there is no `rptr` in it at all. The shift lives in
  // `settings.rptr_shift` / `rptr_offset_hz()` and is applied on the rig-CONFIGURATION path
  // (:7720, :15339), so on FM through a repeater the engine's figure is the dial the
  // operator is LISTENING on while the transmitter is a whole shift away.
  //
  // Settings is the only surface that carries it, and reading it here is the pattern CW and
  // APRS already use. `null` = not read yet, or a station whose settings carry no shift —
  // and that resolves to UNKNOWN, never to simplex: resolving an absent value to "no shift"
  // is the one default that puts the confident wrong number back.
  const [rptrShift, setRptrShift] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    void getSettings()
      .then((s) => {
        if (!alive) return
        setRptrShift(typeof s?.rptrShift === 'string' ? s.rptrShift : null)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
    // Re-read on the two events that change it: the operator switching to FM, and a memory
    // recall (which lands as a band change and rewrites the shift in the same patch).
  }, [commandedMode, snap.radio.band])

  // ── THE TRANSMIT CONTRACT — read, never computed ───────────────────────────────────
  // `txEmissionMhz` is `Engine::tx_emission_mhz()`, which resolves `tx_freq_verdict()` — THE
  // one decision about which frequency the next over is emitted on — and folds XIT in. It is
  // printed verbatim. Adding a dial to an offset here would be a SECOND answer to that
  // question, free to disagree with the one the licence gate actually judges.
  const txEmission = snap.radio.txEmissionMhz ?? null
  const xitHz = snap.radio.xitHz ?? 0
  // ⚠️ XIT DEMOTES THE WHOLE FIGURE. It rides INTO the emission and its path is write-only
  // and optimistic, so with a clarifier offset in play the number contains a component
  // nothing read back — and calling that a read-back is exactly the lie the marks exist to
  // stop. No CAT link demotes it for the plainer reason.
  const emissionRead = catOk && xitHz === 0
  // ⛔ AND ON FM THE FIGURE MAY NOT BE THE EMISSION AT ALL. Anything but a shift we have
  // READ and found to be simplex disqualifies it — including not having read one yet.
  //
  // ⚠️ IT IS NOT FIXED BY ADDING THE SHIFT ON HERE. `rptr_offset_hz()` is an override or a
  // BAND-CONVENTION TABLE in Rust (settings.rs:360); re-deriving that in the UI would be a
  // second copy of engine arithmetic, free to drift — the same defect as the wrong number,
  // arriving later. The fix belongs in `tx_emission_mhz`; until it lands, the strip says
  // nothing here rather than something wrong.
  const repeaterUnknown = observedMode === 'FM' && rptrShift !== 'simplex'
  const splitTxMhz = snap.radio.splitTxMhz ?? null
  // Formatting two reported numbers, not deciding anything: the engine already put the split
  // into the emission above. This cell only says WHY that figure differs from the dial.
  const splitOffsetKhz = splitTxMhz == null ? null : (splitTxMhz - snap.radio.dialMhz) * 1000
  // The watts the rig last actually measured. `txPoW` is present only while transmitting, so
  // between overs this is a memory and the word "last" is what says so. A ref, not state:
  // the poll re-renders anyway and retention must never itself cause a render (TxMeters).
  if (snap.radio.txPoW != null) lastPoW.current = snap.radio.txPoW
  const powerPct = snap.radio.rfPower == null ? null : Math.round(snap.radio.rfPower * 100)
  /** BW on FM is the ONE per-row mark left in the panes: mode-inapplicability is not a
   *  fault in the radio and not a feature it lacks, so it keeps its slot rather than
   *  collapsing into the foot line. The visible mark is the MODE — an invariant token — and
   *  the sentence is the tooltip. */
  const bwNotOnMode = causeFor(control_('BW'), chainState) === 'notOnMode'
  /**
   * ONE STEPPED PAD — the attenuator or the preamp, drawn FROM THE LIST THE RADIO PUBLISHED.
   *
   * ⛔ THE LIST IS THE CONTROL. An attenuator is not a slider: it is the handful of pads this
   * particular rig has, an IC-7300's one 20 dB against an IC-7610's 6/12/18, and `set_att_db`
   * rejects anything that is not on it. So the chips ARE `attStepsDb` and there is no path
   * here that can invent a step — which is why the registry answers `noSteps` rather than
   * `absent` when the radio published nothing, and why that row is dead instead of missing.
   *
   * ⚠️ `0` IS NOT IN THE PUBLISHED LIST AND IS ALWAYS OFFERED. Every rig can switch the stage
   * out, and the DTO omits the implicit off deliberately (types.ts) — so the Off chip is
   * prepended here rather than expected from the radio.
   *
   * ⚠️ AND NO UNIT ON THE PREAMP. ATT is decibels; PRE is whatever THIS radio calls each
   * position, and on an Icom those are `1` and `2` (P.AMP1/P.AMP2) — names, not gains.
   * Printing "1 dB" beside a 1 that means "first preamp" is a wrong number, not a cosmetic.
   */
  const stepRow = (id: 'ATT' | 'PRE', spec: StepRowText) => {
    if (!show(id)) return null
    const c = control_(id)
    const isDead = dead(id)
    const now = snap.radio[c.field!] as number | null | undefined
    return (
      <span className="ph-chain-item" data-chain={id} key={id}>
        <div className="ph-steps" role="group" aria-label={spec.aria} title={spec.title}>
          <span className="ph-dsplev-lbl">{c.plate}</span>
          {[0, ...(stepsFor(c, caps) ?? [])].map((db) => (
            <button
              {...(isDead
                ? deadControlProps('button', describedBy('rx'))
                : { disabled: !control })}
              key={db}
              type="button"
              className={`theme-chip${now === db ? ' active' : ''}${isDead ? ' dead' : ''}`}
              // UNKNOWN IS NOT "OFF". A rig that has never reported which pad is in has no
              // selection to announce, and `aria-pressed="false"` on every chip announces
              // one — the same lie the AGC chips are careful about two rows down.
              aria-pressed={isDead || now == null ? undefined : now === db}
              onClick={guard(isDead, () => pickStep(id, db))}
            >
              {db === 0 ? t('phone.chain.off') : spec.unit ? `${db} ${DB}` : `${db}`}
            </button>
          ))}
        </div>
        {/* The radio published no list. It keeps its row and says WHICH unknown this is —
            "not on this radio" would blame a rig that may well have the pad. */}
        {causeFor(c, chainState) === 'noSteps' && (
          <Unavailable
            mark={t('phone.unavail.mark')}
            title={t('phone.unavail.noSteps', { plate: c.plate })}
          />
        )}
      </span>
    )
  }
  /** One rig-FUNCTION toggle. Both chain panes draw theirs through this, so the receive four
   *  and the transmit two cannot drift apart in behaviour the way they just did in place. */
  const funcToggle = (f: PhoneFunc) => {
    if (!show(f.chain)) return null
    const isDead = dead(f.chain)
    const on = snap.radio[f.key] === true
    const chain = RIG_CONTROLS.find((c) => c.id === f.chain)!.chain
    const press = () =>
      void dspControl.changeFunction(f.key, !on)
        .then((s) => s && onSnap?.(s))
        .catch(() => pushToast(t('phone.dsp.toggleFailed', { func: f.label }), 'error'))
    return (
      <span className="ph-chain-item" data-chain={f.chain} key={f.key}>
        {/* A BUTTON stays focusable when it is dead — `disabled` takes it out of the tab
            order, so a screen-reader operator would never reach the reason. `guard` is the
            other half: aria-disabled does not prevent activation by itself. */}
        <button
          {...(isDead
            ? deadControlProps('button', describedBy(chain))
            : { disabled: !dspControl.canFunction(f.key) })}
          type="button"
          className={`ph-dsp-btn${on ? ' on' : ''}${isDead ? ' dead' : ''}`}
          // UNKNOWN IS NOT "OFF". A rig that has never reported this function has no state
          // to announce, and `aria-pressed="false"` announces one — to a screen reader that
          // is the same lie the missing button used to tell an eye.
          aria-pressed={isDead ? undefined : on}
          title={t(f.titleKey)}
          onClick={guard(isDead, press)}
        >
          {f.label}
        </button>
      </span>
    )
  }

  const bandPane =
    hasBandPane && onWorkSpot ? (
      <CockpitPaneFrame title={t('phone.pane.bandActivity.title')} paneId="bandActivity" fit="content" {...closeProps('bandActivity')}>
        {control || spotsRead?.phase === 'ready' ? <BandStrip
          band={snap.radio.band}
          dialMhz={snap.radio.dialMhz}
          txAllowed={snap.radio.txAllowed}
          phoneSegLo={snap.radio.phoneSegLo}
          phoneSegHi={snap.radio.phoneSegHi}
          spots={spots ?? []}
          needByCall={needByCall}
          typeByCall={typeByCall}
          onWorkSpot={onWorkSpot}
          onPopOut={() => void openPanelWindow('bandmapPhone')}
          // Wheel-tune from the strip (#96): same step + sensitivity + gate as the scope above.
          sideband={snap.radio.sideband || 'USB'}
          tuneEnabled={snap.radio.catOk === true && !snap.radio.txBusyReason && !snap.radio.transmitting}
          stepHz={tuneStep}
          wheelSensitivity={wheelSensitivity}
          onSnap={onSnap}
        /> : <p className="dim" role="status">{t('remote.spotsUnavailable')}</p>}
          {!control && <CollectionStatus name="spots" />}
      </CockpitPaneFrame>
    ) : null

  // The voice keyer TRANSMITS, but it is a pane, not dock chrome (design3 §2/§6: F1–F6
  // visible, record controls may sit behind the pane scroll): its F-key sends are
  // guarded inside the component (txEnabled/keyed/licence via the engine).
  //
  // It IS in the panel vocabulary, and THE STOP LINE (features/panelState.ts) is why nothing
  // stood in the way: the operator must never be unable to stop a transmission, so PTT, Tune
  // and Stop TX render OUTSIDE every ⊞-removable pane with no id at all — and nothing else
  // about a pane bears on whether it may be hidden. That it can START one does not (six panes
  // can). That it hosts a ■ Stop of its own does not either: that button goes away with the
  // pane, a convenience built on the guarantee rather than what holds it up. Two wordings
  // turned on that button, from opposite sides — the first forbade this pane for having one,
  // the fourth forbade the entry for hiding one — and both were wrong the same way.
  // Separately, and as courtesy rather than safety: unmounting the keyer calls stopVoice, so
  // the tick really does end an over. That is why the ⊞ entry carries
  // VOICE_KEYER_STOPS_ON_HIDE — the stopped message AND the discarded recording — before the
  // tick rather than after, and ⊞ Undo, which reaches the same unmount, carries
  // VOICE_KEYER_UNDO_ENDS.
  //
  // Because it transmits, this pane must NEVER remount for any reason but its OWN entry:
  // the same cleanup that makes hiding it safe is data loss when the region merely
  // reflows (an in-flight message aborted, an in-progress recording discarded). So the
  // keyer lives in the LEADING column at every tier (see the region JSX): a pane whose
  // column assignment changes with the tier changes DOM parents, and React cannot carry a
  // fiber across that. Guarded by PhoneCockpit.structure.test.tsx (tier flips, ⊞ toggles
  // of other panels, and the restore back to stock).
  const keyerPane = hasKeyerPane ? (
    <CockpitPaneFrame title={t('phone.pane.voiceKeyer.title')} paneId="voiceKeyer" fit="content" {...closeProps('voiceKeyer')}>
      {control ? <VoiceKeyer
        txEnabled={snap.radio.txEnabled}
        keyed={keyed}
        transmitting={snap.radio.transmitting}
        fdExchange={fdExchange}
      /> : <p className="dim" role="status">{t('remote.voiceKeyerUnavailable')}</p>}
    </CockpitPaneFrame>
  ) : null

  /* Aux panes — rig-scope / DSP / RX-DSP-levels control strips. In the 3-column tier
     they share the middle column with the voice keyer; below that they append to the
     main column. civScope and flexScope are mutually exclusive (one scope feed), so at
     most one "rigscope" frame renders. */
  const auxPanes = (
    <>
      {/* Rig scope controls (native Icom CI-V only) — drive the RADIO's real panadapter: span
          changes the hardware sweep width, ref sets weak-signal visibility. Distinct from the
          view-zoom chips on the scope itself, which only zoom what's already streamed. */}
      {hasRigScopePane && civScope && (
        <CockpitPaneFrame title={t('phone.pane.rigscope.title')} paneId="rigscope" fit="content" {...closeProps('rigscope')}>
          <div className="ph-rigscope" role="group" aria-label={t('phone.rigScope.aria')}>
            <span className="ph-rigscope-lbl" title={t('phone.rigScope.title')}>
              {t('phone.rigScope.label')}
            </span>
            <div className="ph-span">
              {RIG_SPANS.map((sp) => (
                <button disabled={!scopeControl}
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('phone.rigScope.span.title', { span: sp.label })}
                  /* #275: a failure here was swallowed, matching the backend's own swallow —
                     between them a span button on an IC-7300 in Fixed mode did nothing with no
                     explanation anywhere. The rig's OWN refusal arrives a tick later in
                     `radio.scopeSpanRefused` (the status lane); this catch is the nearer half,
                     the command itself failing. */
                  onClick={() => void setScopeSpan(sp.hz).then((s) => onSnap?.(s)).catch(control ? () => {} : scopeFailed)}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('phone.rigScope.ref.title')}>
              <span>{t('phone.scope.ref.label')}</span>
              <input disabled={!scopeControl}
                type="range"
                min={-200}
                max={200}
                step={5}
                value={scopeRefTenths}
                style={{ visibility: scopeControl ? undefined : 'hidden' }}
                onChange={(e) => changeScopeRef(Number(e.target.value))}
                aria-label={t('phone.rigScope.ref.aria')}
              />
              <span className="ph-power-val">{scopeControl ? (scopeRefTenths / 10).toFixed(1) : '—'} {DB}</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {/* FlexRadio SmartSDR panadapter controls — command the Flex pan's real bandwidth + ref. */}
      {hasRigScopePane && flexScope && (
        <CockpitPaneFrame title={t('phone.pane.rigscope.title')} paneId="rigscope" fit="content" {...closeProps('rigscope')}>
          <div className="ph-rigscope" role="group" aria-label={t('phone.flexPan.aria')}>
            <span className="ph-rigscope-lbl" title={t('phone.flexPan.title')}>
              {t('phone.flexPan.label')}
            </span>
            <div className="ph-span">
              {FLEX_SPANS.map((sp) => (
                <button disabled={!scopeControl}
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('phone.flexPan.span.title', { span: sp.label })}
                  onClick={() => void setFlexPanSpan(sp.hz).then((s) => onSnap?.(s)).catch(control ? () => {} : scopeFailed)}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('phone.flexPan.ref.title')}>
              <span>{t('phone.scope.ref.label')}</span>
              <input disabled={!scopeControl}
                type="range"
                min={-140}
                max={-20}
                step={5}
                value={flexRefDbm}
                style={{ visibility: scopeControl ? undefined : 'hidden' }}
                onChange={(e) => changeFlexRef(Number(e.target.value))}
                aria-label={t('phone.flexPan.ref.aria')}
              />
              <span className="ph-power-val">{scopeControl ? flexRefDbm : '—'} {DBM}</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {hasReceiverPane && (
        <CockpitPaneFrame title={t('phone.pane.receiver.title')} paneId="receiver" fit="content" {...closeProps('receiver')}>
          {/* ⭐ THE S-METER, AT THE HEAD OF THE RECEIVE CHAIN. It was an arc floating alone in
              the scope region first, and it read as an ornament — a meter belongs WITH what it
              measures, and everything below this line is the same receiver. Placing it here
              also gives the waterfall back the ~150 px the face was spending. */}
          <SMeter radio={snap.radio} />
          <div className="ph-chain" role="group" aria-label={t('phone.chain.receiver.aria')}>
            {/* MAIN — drawn exactly while the SUB row below is, so a radio with one receiver
                (or a Sub Nexus cannot command) draws nothing here and is unchanged. */}
            <MainReceiverPlate radio={snap.radio} catOk={catOk} />
            {noCatBanner('rx')}
            {/* ── IF: the passband ──────────────────────────────────────────────────
                MOVED OUT OF THE HEADER (operator ruling, 2026-09-20). BW is an IF control
                and it belongs with the rest of the receive chain; the header it came from
                already wraps at 1024, and a filter width sitting between a band picker and a
                power slider answers neither of this cockpit's two questions.

                ⚠️ ITS GATE IS NOT ITS READ-BACK. `filterWidthHz` null means "unknown or the
                rig's default" — the stepper still commands the rig perfectly well from its
                2.4 kHz base and only the READOUT is blank, so marking it unavailable would
                take away a control that works. What stops it is a dead CAT link; what makes
                it meaningless is FM, whose passband is fixed. */}
            {show('BW') && (
            <div className="ph-chain-item" data-chain="BW">
              <div className="ph-filter" title={t('phone.filter.title')}>
                <span className="ph-filter-lbl">{BW}</span>
                <button disabled={dead('BW') || !filterControl.allowed}
                  aria-describedby={describedBy('rx')}
                  type="button"
                  className="ph-filter-step"
                  onClick={() => bumpFilter(-FILTER_STEP_HZ)}
                  title={t('phone.filter.narrower.title', { step: FILTER_STEP_HZ })}
                >
                  −
                </button>
                <span className="ph-filter-val mono">
                  {filterHz ? `${(filterHz / 1000).toFixed(1)}k` : '—'}
                </span>
                <button disabled={dead('BW') || !filterControl.allowed}
                  aria-describedby={describedBy('rx')}
                  type="button"
                  className="ph-filter-step"
                  onClick={() => bumpFilter(FILTER_STEP_HZ)}
                  title={t('phone.filter.wider.title', { step: FILTER_STEP_HZ })}
                >
                  +
                </button>
              </div>
              {/* The visible mark is the MODE — a token, the same in every locale — and the
                  sentence is its tooltip. */}
              {bwNotOnMode && (
                <Unavailable
                  mark={commandedMode}
                  title={t('phone.unavail.notOnMode', { plate: BW, mode: commandedMode })}
                />
              )}
            </div>
            )}

            {/* ── FRONT END ────────────────────────────────────────────────────────
                The two STEPPED stages come first because they are first in the signal: the
                pad and the preamp act on the antenna before anything else in this pane
                does, and an operator fighting a strong neighbour reaches for ATT before he
                touches RF gain. Registry order (`RIG_CONTROLS`) puts them here for that
                reason and this is what renders it. */}
            {stepRow('ATT', {
              aria: t('phone.chain.att.aria'),
              title: t('phone.chain.att.title'),
              unit: true,
            })}
            {stepRow('PRE', {
              // ⛔ NO UNIT. On an Icom the preamp labels are `1`/`2` (P.AMP1/P.AMP2) — the
              // names of two positions, not two gains — and Nexus cannot tell those from a
              // rig whose positions really are decibels. So neither gets a dB.
              aria: t('phone.chain.pre.aria'),
              title: t('phone.chain.pre.title'),
              unit: false,
            })}

            {/* RF GAIN — receive front-end gain. Not the header's Pwr slider, which is
                transmit power; they are `RF` and `RFPOWER` to Hamlib for that reason. */}
            {show('RF') && (
            <div className="ph-chain-item" data-chain="RF">
              <label className="ph-dsplev" title={t('phone.analog.rf.title')}>
                <span>{RF}</span>
                <input {...levels.input('rfGain')} disabled={dead('RF') || !levels.can('rfGain')}
                  aria-describedby={describedBy('rx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownRfg}
                  onChange={(e) => changeRfg(Number(e.target.value))}
                  onPointerDown={() => {
                    rfgDragging.current = true
                    levels.input('rfGain').onPointerDown()
                  }}
                  onPointerUp={() => {
                    rfgDragging.current = false
                    levels.input('rfGain').onPointerUp()
                  }}
                  aria-label={t('phone.analog.rf.aria')}
                />
                <span className="ph-power-val">{dead('RF') ? '—' : `${shownRfg}%`}</span>
              </label>
            </div>
            )}

            {/* ── DSP: the four that shape what you HEAR ───────────────────────────
                The two that shape what goes OUT (COMP, VOX) are in the transmitter pane;
                they were in this row only because Hamlib calls all six a "function". */}
            {funcToggle(RX_FUNCS[0])}
            {funcToggle(RX_FUNCS[1])}
            {show('NRLVL') && (
            <div className="ph-chain-item" data-chain="NRLVL">
              <label className="ph-dsplev" title={t('phone.rxDsp.nr.title')}>
                <span>{NR}</span>
                <input {...levels.input('nr')} disabled={dead('NRLVL') || !levels.can('nr')}
                  aria-describedby={describedBy('rx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownNr}
                  onChange={(e) => changeNr(Number(e.target.value))}
                  onPointerDown={() => {
                    nrDragging.current = true
                    levels.input('nr').onPointerDown()
                  }}
                  onPointerUp={() => {
                    nrDragging.current = false
                    levels.input('nr').onPointerUp()
                  }}
                  aria-label={t('phone.rxDsp.nr.aria')}
                />
                <span className="ph-power-val">{dead('NRLVL') ? '—' : `${shownNr}%`}</span>
              </label>
            </div>
            )}
            {funcToggle(RX_FUNCS[2])}
            {funcToggle(RX_FUNCS[3])}

            {/* #95: WHERE the manual notch sits. Hz, not a percentage — you are placing it on
                a tone you can hear.

                ⭐ ITS REASON NAMES THE BACKEND, NOT THE RADIO, and the difference is the
                whole point of saying anything: Hamlib's Icom backend exposes no NOTCHF for
                ANY natively-driven model, so on those rigs the notch is real and reachable
                from the radio's own knob while the FREQUENCY is simply not on the wire. A
                reason that said "your radio does not have this" would be false about the
                radio, and the operator would go looking for a fault that is not there. */}
            {show('NOTCHF') && (
            <div className="ph-chain-item" data-chain="NOTCHF">
              <label className="ph-dsplev" title={t('phone.rxDsp.notchFreq.title')}>
                <span>{NOTCH}</span>
                <input {...levels.input('notch')} disabled={dead('NOTCHF') || !levels.can('notch')}
                  aria-describedby={describedBy('rx')}
                  type="range"
                  min={300}
                  max={3400}
                  step={10}
                  value={shownNotch}
                  onChange={(e) => changeNotch(Number(e.target.value))}
                  onPointerDown={() => {
                    notchDragging.current = true
                    levels.input('notch').onPointerDown()
                  }}
                  onPointerUp={() => {
                    notchDragging.current = false
                    levels.input('notch').onPointerUp()
                  }}
                  aria-label={t('phone.rxDsp.notchFreq.aria')}
                />
                <span className="ph-power-val">{dead('NOTCHF') ? '—' : `${shownNotch} ${HZ}`}</span>
              </label>
            </div>
            )}

            {show('AGC') && (
            <div className="ph-chain-item" data-chain="AGC">
              <div className="ph-agc" role="group" aria-label={t('phone.rxDsp.agc.aria')} title={t('phone.rxDsp.agc.title')}>
                <span className="ph-dsplev-lbl">{AGC}</span>
                {AGC_CHIPS.map(({ id, labelKey }) => (
                  <button disabled={dead('AGC') || !dspControl.canAgc}
                    aria-describedby={describedBy('rx')}
                    key={id}
                    type="button"
                    className={`theme-chip${agc === id ? ' active' : ''}`}
                    // UNKNOWN is not "not this speed": a rig that reports no AGC has no
                    // selection to announce, and `aria-pressed="false"` on five chips
                    // announces one.
                    aria-pressed={dead('AGC') ? undefined : agc === id}
                    onClick={() => changeAgc(id)}
                  >
                    {t(labelKey)}
                  </button>
                ))}
              </div>
            </div>
            )}

            {/* ── AUDIO: the end of the chain, and the two that go quietly wrong ───
                Both carry the decode-audio warning. See the note where `afMutesDecode` and
                `squelchMutesDecode` are computed: it NOTIFIES AND NEVER ACTS. */}
            {show('AF') && (
            <div className="ph-chain-item" data-chain="AF">
              <label className="ph-dsplev" title={t('phone.analog.af.title')}>
                <span>{AF}</span>
                <input {...levels.input('afGain')} disabled={dead('AF') || !levels.can('afGain')}
                  aria-describedby={describedBy('rx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownAf}
                  onChange={(e) => changeAf(Number(e.target.value))}
                  onPointerDown={() => {
                    afDragging.current = true
                    levels.input('afGain').onPointerDown()
                  }}
                  onPointerUp={() => {
                    afDragging.current = false
                    levels.input('afGain').onPointerUp()
                  }}
                  aria-label={t('phone.analog.af.aria')}
                />
                <span className="ph-power-val">{dead('AF') ? '—' : `${shownAf}%`}</span>
                {afMutesDecode && (
                  <span className="ph-lvl-warn" role="status" title={t('phone.analog.af.mutesDecode.title')}>
                    {t('phone.analog.af.mutesDecode.label')}
                  </span>
                )}
              </label>
            </div>
            )}
            {show('SQL') && (
            <div className="ph-chain-item" data-chain="SQL">
              <label className="ph-dsplev" title={t('phone.analog.sql.title')}>
                <span>{SQL}</span>
                <input {...levels.input('squelch')} disabled={dead('SQL') || !levels.can('squelch')}
                  aria-describedby={describedBy('rx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownSql}
                  onChange={(e) => changeSql(Number(e.target.value))}
                  onPointerDown={() => {
                    sqlDragging.current = true
                    levels.input('squelch').onPointerDown()
                  }}
                  onPointerUp={() => {
                    sqlDragging.current = false
                    levels.input('squelch').onPointerUp()
                  }}
                  aria-label={t('phone.analog.sql.aria')}
                />
                <span className="ph-power-val">{dead('SQL') ? '—' : `${shownSql}%`}</span>
                {squelchMutesDecode && (
                  <span className="ph-lvl-warn" role="status" title={t('phone.analog.sql.mutesDecode.title')}>
                    {t('phone.analog.sql.mutesDecode.label')}
                  </span>
                )}
              </label>
            </div>
            )}
            {absentLine('rx')}
          </div>
          {/* ⭐ THE SUB RECEIVER — a dual-receiver radio's second receiver, below Main's chain.
              Draws NOTHING unless the snapshot offers a Sub Nexus can command, so every other
              radio's pane is exactly what it was. On the Remote page too: its sliders go through
              the station's `radio.subLevel` intent. A component of its own, not a widened shared
              one — see its header. */}
          <SubReceiverStrip radio={snap.radio} radioId={snap.activeRadioId} catOk={catOk} describedBy={describedBy('rx')} onSnap={onSnap} />
        </CockpitPaneFrame>
      )}

      {hasTransmitterPane && (
        <CockpitPaneFrame title={t('phone.pane.transmitter.title')} paneId="transmitter" fit="content" {...closeProps('transmitter')}>
          <div className="ph-chain" role="group" aria-label={t('phone.chain.transmitter.aria')}>
            {noCatBanner('tx')}
            {/* MIC gain came out of the header for a sharper reason than BW did: it sat
                DIRECTLY BESIDE the AF slider there — a transmit level and a receive level,
                adjacent, with nothing on screen saying which was which. */}
            {show('MIC') && (
            <div className="ph-chain-item" data-chain="MIC">
              <label className="ph-dsplev" title={t('phone.mic.title')}>
                <span>{t('phone.mic.label')}</span>
                <input {...levels.input('micGain')} disabled={dead('MIC') || !levels.can('micGain')}
                  aria-describedby={describedBy('tx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownMic}
                  onChange={(e) => changeMic(Number(e.target.value))}
                  onPointerDown={() => {
                    micDragging.current = true
                    levels.input('micGain').onPointerDown()
                  }}
                  onPointerUp={() => {
                    micDragging.current = false
                    levels.input('micGain').onPointerUp()
                  }}
                  aria-label={t('phone.mic.aria')}
                />
                <span className="ph-power-val">{dead('MIC') ? '—' : `${shownMic}%`}</span>
              </label>
            </div>
            )}

            {/* #95: the speech processor's toggle and its DEPTH, which are separately
                reported and so are separately marked — the report was precisely that the
                toggle arrived without the level. */}
            {funcToggle(TX_FUNCS[0])}
            {show('COMPLVL') && (
            <div className="ph-chain-item" data-chain="COMPLVL">
              <label className="ph-dsplev" title={t('phone.rxDsp.comp.title')}>
                <span>{COMP}</span>
                <input {...levels.input('compression')} disabled={dead('COMPLVL') || !levels.can('compression')}
                  aria-describedby={describedBy('tx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownComp}
                  onChange={(e) => changeComp(Number(e.target.value))}
                  onPointerDown={() => {
                    compDragging.current = true
                    levels.input('compression').onPointerDown()
                  }}
                  onPointerUp={() => {
                    compDragging.current = false
                    levels.input('compression').onPointerUp()
                  }}
                  aria-label={t('phone.rxDsp.comp.aria')}
                />
                <span className="ph-power-val">{dead('COMPLVL') ? '—' : `${shownComp}%`}</span>
              </label>
            </div>
            )}
            {funcToggle(TX_FUNCS[1])}

            {/* ── THE MONITOR — the last thing in the transmit chain, and the only one of
                these you hear rather than send. It is the rig playing YOUR OWN audio back
                while you talk, which is how an operator catches his own splatter, a stuck
                VOX or a processor wound too far.

                ⚠️ IT IS NOT AF GAIN AND THE TOOLTIP SAYS SO. This one is heard only while
                TRANSMITTING, so turning it to zero can never silence the audio a decoder is
                listening to — which is exactly the trap the AF slider in the receive pane
                carries a warning about. Two controls, opposite hazards, so neither wording
                may be reused for the other. */}
            {show('MON') && (
            <div className="ph-chain-item" data-chain="MON">
              <label className="ph-dsplev" title={t('phone.chain.mon.title')}>
                <span>{MON}</span>
                <input disabled={dead('MON') || !control}
                  aria-describedby={describedBy('tx')}
                  type="range"
                  min={0}
                  max={100}
                  value={shownMon}
                  onChange={(e) => changeMon(Number(e.target.value))}
                  onPointerDown={() => { monDragging.current = true }}
                  onPointerUp={() => { monDragging.current = false }}
                  aria-label={t('phone.chain.mon.aria')}
                />
                <span className="ph-power-val">{dead('MON') ? '—' : `${shownMon}%`}</span>
              </label>
            </div>
            )}
            {absentLine('tx')}
          </div>
        </CockpitPaneFrame>
      )}
    </>
  )

  const logPane = (
    <CockpitPaneFrame title={quick && !fieldDay ? t('remote.quick.logbook') : t('phone.pane.log.title')} paneId="log">
      {/* compactRecall died here (2026-07-31). It existed because the pre-overhaul cockpit had
          no interposed scroller: the full recall card's height crushed the operating panes
          directly. This pane is now a FILL pane whose .pane-body scrolls internally, so a tall
          card scrolls inside the log column and can never squeeze the cockpit — the operator
          gets the QRZ photo / bearing / history back while operating. */}
      {control ? <LogEntry
        onOpenLogbook={onOpenLogbook}
        snap={snap}
        mode={logMode}
        defaultRst="59"
        exchange="terrestrial"
        // The frame head above already reads LOG and is this pane's accessible name.
        titled={false}
        onSpot={(call) => {
          setSpotCall(call)
          setSpotOpen(true)
        }}
        onCallChange={setWorkedCall}
        pendingWork={pendingWork}
        onConsumeWork={onConsumeWork}
        fieldDay={fieldDay}
        fdMode="PH"
      /> : <RemoteRecallEntry snap={snap} mode={commandedMode === 'FM' ? 'FM' : 'SSB'} onOpenLog={onOpenLogbook} pendingWork={pendingWork} onConsumeWork={onConsumeWork} />}
    </CockpitPaneFrame>
  )

  return (
    <main className={`layout single phone-cockpit${quick ? ' remote-quick-contact' : ''}`} ref={cockpitRef}>
      <CockpitHeader
        snap={snap}
        onSnap={onSnap}
        modeIndicator={
          <div className="ph-mode-pick" role="group" aria-label={t('phone.mode.aria')}>
            {(['AUTO', 'USB', 'LSB', 'FM', 'AM'] as const)
              // AM IS OFFERED ON EVERY BAND, like USB/LSB/FM — none of which is band-filtered
              // either. This used to hide AM below 10 MHz and at 28 MHz and up, on the reasoning
              // that "20/17/15/12 m are not AM territory". That was wrong on the one frequency it
              // most needed to be right about: 14.286 is the 20 m AM calling frequency, and an
              // operator working it there found the button missing, set AM at the radio instead,
              // and Nexus logged his contact as SSB (2026-09-10 field report).
              //
              // The deeper problem is that a band plan hardcoded in a UI file goes stale silently
              // and differs by region, while the RIG already knows what it supports and refuses
              // what it does not. Nexus does not get to have an opinion about where AM belongs;
              // that is the operator's business and his licence.
              .map((m) => {
              const active = m === 'AUTO' ? modeOverride === null : modeOverride === m
              return (
                <button
                  key={m}
                  type="button"
                  className={`ph-mode-btn${active ? ' active' : ''}`}
                  aria-pressed={active}
                  disabled={!phoneModeControl.canPick(m === 'AUTO' ? null : m) || !catOk}
                  title={
                    m === 'AUTO'
                      ? t('phone.mode.auto.title', { sideband: sidebandAuto })
                      : t('phone.mode.force.title', { mode: m })
                  }
                  onClick={() => pickMode(m === 'AUTO' ? null : m)}
                >
                  {m === 'AUTO' ? autoPlate(sidebandAuto) : m}
                </button>
              )
            })}
          </div>
        }
        bandControl={<BandPicker snap={snap} mode="phone" onSnap={onSnap} />}
        remoteFrequency
        remoteMode="phone"
        onCommitDial={commitDial}
        actions={
          host && panels ? (
            <PanelsMenu
              items={host.menuItems}
              onToggle={(id, show) =>
                panels.setPanelState(id as PhonePanelId, show ? 'docked' : 'removed')
              }
              onUndo={panels.undo}
              canUndo={panels.canUndo}
              // Undo is the ⊞ menu's second hide path — see VOICE_KEYER_UNDO_ENDS.
              undoNote={
                panels.undoRemoves.includes('voiceKeyer') ? VOICE_KEYER_UNDO_ENDS : undefined
              }
              onReset={panels.reset}
            />
          ) : undefined
        }
        wheelTune
        digitTune
        wheelStepHz={tuneStep}
        wheelSensitivity={wheelSensitivity}
        frequencyExtras={
          <TuningStrip
            snap={snap}
            onSnap={onSnap}
            step={tuneStep}
            onStep={setTuneStep}
            sensitivity={wheelSensitivity}
            showReadout={false}
          />
        }
        power={{
          value: control ? power : snap.radio.rfPower == null ? null : Math.round(snap.radio.rfPower * 100),
          unit: '%',
          onChange: changePower,
          label: t('phone.header.power.label'),
          title: t('phone.header.power.title'),
          onPointerDown: () => {
            dragging.current = true
          },
          onPointerUp: () => {
            dragging.current = false
          },
        }}
        txActiveLabel="▲ TX"
        onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
        onAtuTune={() =>
          void atuTune()
            .then((s) => onSnap?.(s))
            .catch((e) => pushToast(String(e), 'error'))
        }
        onStopTx={() => void haltTx()}
      >
        {micOffForDax && (
          <span className="ph-mode-mismatch" title={t('phone.micDax.title')}>
            {t('phone.micDax.label')}
          </span>
        )}
        {modeMismatch && (
          <span
            className="ph-mode-mismatch"
            title={t('phone.rigMismatch.title', {
              rigMode: modeMismatch,
              mode: commandedMode,
            })}
          >
            {t('phone.rigMismatch.chip', { mode: modeMismatch })}
          </span>
        )}
        {/* ⭐ SPLIT IS NO LONGER GATED ON `catOk` (2026-09-20 ruling). It used to vanish with
            the link, which told the operator nothing about whether this cockpit HAS a split
            control — and split is the one thing a DX chaser looks for first. It stays and
            says why it is dead; the reason rides INTO the control rather than sitting beside
            it, so its three buttons are disabled by the same fact that prints the mark. */}
        <SplitControl
          snap={snap}
          onSnap={onSnap}
          onError={(m) => pushToast(m, 'error')}
          unavailable={
            catOk
              ? undefined
              : { mark: t('phone.unavail.mark'), title: t('phone.unavail.noCat', { plate: SPLIT }) }
          }
        />
        {!catOk && (
          <span
            className="ph-nocat"
            title={snap.radio.catDetail || t('phone.noCat.title')}
          >
            {t('phone.noCat.label')}
          </span>
        )}
        {onRecallMemory && (
          // The ★-favorites quick-recall strip (bounded + wrapping — the old
          // MemoryBank list grew the header unbounded). Recall is App-owned
          // (recallMemory): settings patch + retune + cockpit auto-switch.
          control ? <MemoryStrip
            dialMhz={snap.radio.dialMhz}
            mode={commandedMode}
            onRecall={onRecallMemory}
            onManage={onOpenMemories}
          /> : <MemoryStripUnavailable />
        )}
        {/* ⭐ THE BEAM BUTTON WAS NEVER MISSING FROM THE STRIP — it was missing from THIS HOST.
            `RotorStrip` has rendered a one-click "→ CALL" slew since it was written, and CwCockpit
            and OperateCockpit both pass `targetCall`/`onPointAt`. Phone did not, so the control
            rendered as nothing (`{targetCall && onPointAt && …}`) and two testers reported the
            feature as absent — one of them naming this tab specifically. Phone was the only
            cockpit with no live worked-call to pass; `workedCall` above is that source. */}
        {control || rotatorControl ? <RotorStrip
          onOpenSettings={onOpenSettings}
          targetCall={workedCall || null}
          onPointAt={(call) =>
            pointRotatorAtCall(call)
              .then((bearing: number | null | undefined) =>
                // A browser gets no bearing back: the station resolves it.
                pushToast(bearing == null ? t('remote.b1.rotatorPointing', { call }) : t('cw.rotator.pointed', { call, bearing: Math.round(bearing) }), 'info'),
              )
              .catch((e) =>
                pushToast(
                  control ? t('cw.rotator.failed', { error: e instanceof Error ? e.message : String(e) }) : controlFailureMessage(e),
                  'error',
                ),
              )
          }
        /> : <span className="dim" role="status" aria-label={t('remote.rotatorUnavailable')} title={t('remote.rotatorUnavailable')}>{t('rotor.strip.aria')} —</span>}
        {/* Glyph only (density pass 2026-08-04, the same move the FT cockpit's header made):
            '● Record QSO' spent ~95px of a header region that WRAPS, and the word said what
            the glyph and the tooltip already say. The accessible name is explicit here rather
            than left to the glyph — a screen reader must not be handed a bare bullet. */}
        <button
          type="button"
          className={`ph-rec${recording ? ' on' : ''}`}
          onClick={toggleRecord}
          disabled={!control || (recBusy)}
          aria-label={recording ? t('phone.record.stop.aria') : t('phone.record.start.aria')}
          title={recording ? t('phone.record.on.title') : t('phone.record.off.title')}
        >
          <span className="ph-rec-dot" aria-hidden="true">
            {recording ? '■' : '●'}
          </span>
          {REC}
        </button>
        {/* No visible 'RX': the meter is a role="meter" already named "RX audio level", and
            the label element keeps the same string as its tooltip. */}
        <label className="ph-rxmeter" title={t('phone.rxMeter.label')}>
          <LiveLevelMeter active={active} label={t('phone.rxMeter.label')} variant="compact" />
        </label>
      </CockpitHeader>

      {/* THE SCOPE STRIP, ⊞-hideable since 2026-08-16. The shell's four-child census is
          unchanged — header, scope, ONE pane region, TX dock — one of the four is simply
          conditional now, exactly as Operate's waterfall has been since 0.15.0. The gate is
          HERE, at the shell child, and not inside the section: a scope that rendered an empty
          `.ph-scope-panel` would keep its `flex: 0 1 22%` basis and its 8em floor and hand the
          operator back a bordered empty box instead of the height he asked to reclaim.
          `.cockpit-panes` is `flex: 1 1 0` (cockpit-panes.css) — the region IS the sink, so
          the freed height goes to the panes with no rule change.

          The Splitter goes with it. It drags `--ph-scope-h`, the strip's flex basis, so on its
          own it is a grab handle for something that is not there — and it is a shell child of
          its own, which would leave a stranded 8px seam between the header and the region.
          The var it wrote is untouched and still stored, so re-ticking restores the height he
          dragged to rather than the 22% default.

          Nothing on this strip stops a transmission (THE STOP LINE, features/panelState.ts):
          it is a display plus click-to-tune, and Stop TX / Tune are in the header above, PTT in
          the dock below, none of them reachable from the ⊞ menu. */}
      {shown('scope') && (
        <>
      <section hidden={!details} className={`ph-scope-panel${!details ? ' ph-scope-panel--quiet' : ''}`}>
        <div className="ph-scope-head">
          {(() => {
            // Honest per feed: soundcard FFT = the demodulated RX audio; a native
            // panadapter = the real RF spectrum, so name it a panadapter and show its span.
            const rf = nativeRf ? scopeFeed : null
            // The fed span is a MEASUREMENT and is assembled here; only the audio view's
            // word is a catalog entry.
            const scopeSub = rf
              ? `· ${(rf.loHz / 1e6).toFixed(4)}–${(rf.hiHz / 1e6).toFixed(4)} MHz`
              : `· ${t('phone.scope.audio.sub')}`
            return (
              <span
                className="ph-scope-title"
                title={rf ? t('phone.scope.nativeRf.title') : t('phone.scope.audio.title')}
              >
                {rf ? t('phone.scope.nativeRf.label') : t('phone.scope.audio.label')}{' '}
                <span className="ph-scope-sub">{scopeSub}</span>
              </span>
            )
          })()}
          {/* No 'Colors' label: PalettePicker is a <select> that already carries
              aria-label="Waterfall color palette (applies to all modes)" and the matching
              tooltip, so the word was the third statement of the same thing on one row. */}
          <PalettePicker />
          {/* THE STRIP'S OWN ✕ (⊞ `scope`), last in the head — the title carries
              `margin-right: auto`, so this row's tail is its right edge, where every other
              pane in the app keeps its close button. Nothing on this strip stops a
              transmission (THE STOP LINE), so its hide ends nothing and it warns of nothing. */}
          <PaneCloseButton title={phonePanelLabels().scope} {...closeProps('scope')} />
        </div>
        <div className="ph-scope-wrap" ref={scopeRef} title={t('phone.scope.tuneHint')}>
          {yaesuScope ? (
            // The FT-710 sweeps its own span and owns where the sweep sits, so these command the RADIO
            // and the app draws what comes back. Two compact <select>s rather than thirteen chips: the
            // rig has ten span rungs and three positions, and a chip row that long crowds the scope it
            // is supposed to serve.
            <div className="ph-span" role="group" aria-label={t('phone.scope.yaesu.aria')}>
              <select disabled={!scopeControl}
                className="theme-chip"
                aria-label={t('phone.scope.yaesu.span.aria')}
                title={t('phone.scope.yaesu.span.title')}
                value={yaesuSpanLabel}
                onChange={(e) => {
                  const sp = YAESU_SPANS.find((x) => x.label === e.target.value)
                  if (sp) void setScopeSpan(sp.halfHz).then((s) => onSnap?.(s)).catch((e) => (control ? pushToast(String(e), 'error') : scopeFailed(e)))
                }}
              >
                {YAESU_SPANS.map((sp) => (
                  <option key={sp.label} value={sp.label}>
                    {sp.label}Hz
                  </option>
                ))}
              </select>
              <select disabled={!scopeControl}
                className="theme-chip"
                aria-label={t('phone.scope.yaesu.pos.aria')}
                title={t('phone.scope.yaesu.pos.title')}
                value={yaesuPosition}
                onChange={(e) => {
                  const pos = e.target.value as 'center' | 'cursor' | 'fix'
                  void setYaesuScopeMode(pos).then((s) => onSnap?.(s)).catch((e) => (control ? pushToast(String(e), 'error') : scopeFailed(e)))
                }}
              >
                <option value="center">{t('phone.scope.yaesu.pos.center')}</option>
                <option value="cursor">{t('phone.scope.yaesu.pos.cursor')}</option>
                <option value="fix">{t('phone.scope.yaesu.pos.fix')}</option>
              </select>
            </div>
          ) : nativeRf ? (

            // Native RF panadapter: RF-width zoom around the dial (not audio-passband slices).
            <div className="ph-span" role="group" aria-label={t('phone.rfZoom.aria')}>
              {RF_SPANS.map((sp) => (
                <button
                  key={sp.id}
                  type="button"
                  className={`theme-chip${rfSpan.id === sp.id ? ' active' : ''}`}
                  aria-pressed={rfSpan.id === sp.id}
                  title={sp.title()}
                  onClick={() => setRfSpan(sp)}
                >
                  {sp.label()}
                </button>
              ))}
            </div>
          ) : (
            <div className="ph-span" role="group" aria-label={t('phone.scope.span.aria')}>
              {SPANS.map((sp) => (
                <button
                  key={sp.id}
                  type="button"
                  className={`theme-chip${span.id === sp.id ? ' active' : ''}`}
                  aria-pressed={span.id === sp.id}
                  title={sp.title()}
                  onClick={() => setSpan(sp)}
                >
                  {sp.label()}
                </button>
              ))}
            </div>
          )}
          <PhoneScope
            hideSmeter
            active={active && details}
            transmitting={snap.radio.transmitting}
            theme={theme}
            smeterDb={smeterDb}
            viewLoHz={yaesuRf ? -1e9 : nativeRf ? rfSpan.lo : 0}
            viewHiHz={yaesuRf ? 1e9 : nativeRf ? rfSpan.hi : (span.widthHz || Math.min(4000, Math.max(800, filterHz ?? 4000)))}
            carrierCentered={!nativeRf}
            sideband={commandedMode}
            dialHz={snap.radio.dialMhz > 0 ? Math.round(snap.radio.dialMhz * 1e6) : null}
            onFeed={(source, loHz, hiHz) => setScopeFeed({ source, loHz, hiHz })}
            onTune={onScopeTune}
          onBeginClick={control ? undefined : scopeClick.begin}
            filterWidthHz={filterHz ?? 2400}
            interactive={details && (control || scopeClick.allowed) && catOk && !snap.radio.txBusyReason && !snap.radio.transmitting && snap.radio.dialMhz > 0}
          />
        </div>
      </section>
      {details && <Splitter
        axis="y"
        varName="--ph-scope-h"
        target={cockpitRef}
        storageKey="nexus.split.phone.scope"
        min={SCOPE_SPLIT_MIN}
        max={SCOPE_SPLIT_MAX}
        defaultPct={22}
        label={t('phone.scope.splitter.label')}
      />}
        </>
      )}

      {/* THE PANE REGION — one CockpitPaneFrame grid for every operator-content block.
          useRegionCols OWNS data-cols (measured from the region itself, stamped
          imperatively — never rendered here); the JSX renders exactly as many
          .cockpit-col groups as the tier: at 1 the two-column grouping simply stacks and
          the region is the scroller; at 2 it is band+keyer+aux | log; at 3 the aux
          strips take their own middle column. Columns are implicit-row grids, so a pane
          hidden from ⊞ Panels leaves no empty cell behind — and when the ⊞ menu empties
          the LEADING column outright (possible only since the keyer gained an entry), the
          column itself is not rendered and maxCols collapses to 1 with it, so a bounded
          tier never holds a track with nothing in it.

          The columns are KEYED, and the log + keyer keep the same key at every tier: a
          tier flip that moved a pane to a different column div would UNMOUNT it (React
          cannot carry a fiber across a parent change — keys on the pane don't help,
          only a stable parent does). For LogEntry that wiped every in-progress QSO
          field mid-flip; for VoiceKeyer it aborted an in-flight voice TX. So the keyer
          stays in the leading column at 3-col too — a deliberate deviation from
          design3 §2's "keyer in col 2" grouping, because fiber stability for a
          TX-capable pane outranks the grouping aesthetic (fix-round D1, 2026-07-31;
          guarded by PhoneCockpit.structure.test.tsx). Aux strips still change columns
          on a 2↔3 flip and do remount — they hold no local state (their sliders bind
          to cockpit state), so that residual is harmless and accepted. */}
      <div className={`cockpit-panes${quick ? ' cockpit-panes--contact' : ''}`} ref={panesRef}>
        {cols === 3 ? (
          <>
            <div className={`cockpit-col${!details ? ' cockpit-col--quiet' : ''}`} key="main">
              {bandPane}
              {keyerPane}
            </div>
            <div className={`cockpit-col${!details ? ' cockpit-col--quiet' : ''}`} key="aux">{auxPanes}</div>
            <div className={`cockpit-col${quick ? ' cockpit-col--contact' : ''}`} key="log">{logPane}</div>
          </>
        ) : (
          <>
            {leadPresent && (
              <div className={`cockpit-col${!details ? ' cockpit-col--quiet' : ''}`} key="main">
                {bandPane}
                {keyerPane}
                {auxPanes}
              </div>
            )}
            <div className={`cockpit-col${quick ? ' cockpit-col--contact' : ''}`} key="log">{logPane}</div>
          </>
        )}
      </div>

      {/* TX DOCK — the transmit chrome, pinned OUTSIDE the pane region so no pane
          layout, stored or hand-edited, can move, hide or scroll it away. PTT and Lock
          have no id in the panel vocabulary (unrepresentable beats guarded); the TX
          meters DO keep their ⊞ id — a readout, not a control — but render here, beside
          the button that keys the rig. */}
      <div className={`cockpit-txdock${control ? '' : ' remote-observer-dock'}`}>
      {/* Transmit meters (SWR/ALC/Po/COMP), `pinned`: the readings STAY after the key is
          released, dimmed, and before the first over the panel holds a one-line hint saying
          when it reads. They used to render only while keyed, and that is the wrong half of
          the QSO — this is the meter a voice operator learns their drive from, and a reading
          that exists only while the mic key is held is one they can never study. Same prop,
          same reasons, as the Operate strip (the anti-bounce ruling in TxMeters.tsx).

          At the TOP of the dock, exactly as in CW: the dock is bottom-anchored (sticky
          bottom), so a child GROWING below the PTT row pushes the dock upward and shifts the
          button out from under the operator's held pointer mid-over — onPointerLeave would
          then unkey the rig. Pinning removes the mount/unmount but not the growth: the hint
          is one line and the readings are up to four rows, so the panel still changes height
          on key-down. Above the row, that growth leaves the row's screen position fixed. */}
      {/* ⭐ THE TRANSMIT CONTRACT — "what goes out when I key?", in one line that is always
          there and never moves (operator, 2026-09-20).
 
          The answer used to be scattered across four places and none of them was the
          emission: the dial is in the header, the split offset is a chip beside it, XIT is in
          the tuning strip, and under split the RX dial is not a conservative stand-in for the
          TX frequency — it is an unrelated number. XIT moves the transmitter without moving
          anything on screen at all.
 
          ⚠️ ABOVE THE METERS, so it is above the PTT row. The dock is bottom-anchored and a
          child that GROWS below the row pushes the button out from under a held pointer,
          which fires onPointerLeave and drops the transmission mid-over. This strip is one
          fixed line either way; its position is the belt to that brace.
 
          ⚠️ NO ⊞ ID AND NO CONTROLS. It is a readout, so it is not in the vocabulary (nothing
          can hide it) and it holds nothing that could be mistaken for a way to stop or start
          a transmission — THE STOP LINE is untouched by it in both directions. */}
      <div className="ph-txcontract" role="group" aria-label={t('phone.txContract.aria')}>
        <span className="ph-txc-cell" data-txc="freq" title={t('phone.txContract.freq.title')}>
          <span className="ph-txc-lbl">{TX}<span aria-hidden="true">▸</span></span>
          {txEmission == null || repeaterUnknown ? (
            <>
              <span className="ph-txc-val mono">—</span>
              <TruthMark
                kind="off"
                word={repeaterUnknown ? t('phone.tx.repeaterMark') : undefined}
                title={repeaterUnknown ? t('phone.txContract.repeaterShift.title') : t('phone.txContract.noEmission.title')}
              />
            </>
          ) : (
            <>
              {/* The header's OWN formatter, so the dock and the readout can never print one
                  frequency two different ways. */}
              <span className="ph-txc-val mono">{formatDialMhz(txEmission)}</span>
              <TruthMark
                kind={emissionRead ? 'rig' : 'cmd'}
                title={emissionRead ? t('phone.prov.rig') : t('phone.txContract.freq.commanded.title')}
              />
            </>
          )}
        </span>
        <span className="ph-txc-cell" data-txc="mode" title={t('phone.txContract.mode.title')}>
          <span className="ph-txc-val">{observedMode}</span>
          {/* The SAME read-back gate the log writes through (`rigReadPhoneMode`): catOk, a
              non-empty rig mode, and a mode this cockpit can name. What the operator is told
              here and what Nexus writes into the record cannot disagree. */}
          <TruthMark
            kind={rigReadPhoneMode != null ? 'rig' : 'cmd'}
            title={rigReadPhoneMode != null ? t('phone.prov.rig') : t('phone.prov.cmd', { plate: observedMode })}
          />
        </span>
        <span className="ph-txc-cell" data-txc="split" title={t('phone.txContract.split.title')}>
          <span className="ph-txc-lbl">{SPLIT}</span>
          {splitOffsetKhz == null ? (
            <span className="ph-txc-val">{SIMPLEX}</span>
          ) : (
            <>
              <span className="ph-txc-val mono">
                {splitOffsetKhz >= 0 ? `+${splitOffsetKhz.toFixed(1)}` : splitOffsetKhz.toFixed(1)}
              </span>
              {/* ⚠️ ALWAYS `cmd`, AND THAT IS A GAP IN THE DTO rather than a judgement about
                  the radio. `Engine::tx_freq_verdict` DOES distinguish a split we commanded
                  and the rig acknowledged from one the rig merely reported, and refuses
                  outright when it cannot say where — but none of that reaches the snapshot:
                  `txEmissionMhz` collapses SplitUnverified onto the dial and the verdict
                  itself is not a field. Until it is, "Nexus commanded this" is the most this
                  strip may claim, and claiming more is the one thing it must never do. */}
              <TruthMark kind="cmd" title={t('phone.txContract.split.commanded.title')} />
            </>
          )}
        </span>
        {/* No cell on a radio with no XIT (the IC-9700): its "+0 ⌁cmd" would stand for an
            offset dialled on an XIT knob the radio does not have. */}
        {!snap.radio.xitUnsupported && (
          <span className="ph-txc-cell" data-txc="xit" title={t('phone.txContract.xit.title')}>
            <span className="ph-txc-lbl">{XIT}</span>
            <span className="ph-txc-val mono">{xitHz >= 0 ? `+${xitHz}` : String(xitHz)}</span>
            {/* `cmd` EVEN AT ZERO. Write-only means Nexus cannot see an offset dialled at the
                radio, so "+0" states what Nexus commanded and not where the transmitter is. */}
            <TruthMark kind="cmd" title={t('phone.txContract.xit.commanded.title')} />
          </span>
        )}
        <span className="ph-txc-cell" data-txc="power" title={t('phone.txContract.power.title')}>
          <span className="ph-txc-val mono">{powerPct == null ? '—' : `${powerPct}%`}</span>
          {/* NO MARK ON POWER, deliberately: `rfPower` is documented as the rig read-back
              WHEN CAT REPORTS ONE and the last commanded value otherwise, and the snapshot
              does not say which it is — so neither mark would be supportable. The measured
              watts beside it are evidence of their own and need none. */}
          {lastPoW.current != null && (
            <span className="ph-txc-sub">{t('phone.tx.lastOver', { watts: Math.round(lastPoW.current) })}</span>
          )}
        </span>
        {/* ⚠️ VOX IS A SAFETY LINE, not a status one, and it belongs on the contract because
            it changes the answer to "what happens when I key": with VOX on the operator does
            not key at all — the radio does, from the microphone. Stop TX halts what NEXUS is
            doing and cannot unkey a transmitter the operator's own voice is holding up.
            A KEYING-SOURCE fact, never a stop control: it is a readout here and the VOX
            toggle itself lives in the transmit chain (THE STOP LINE, features/panelState.ts). */}
        {snap.radio.vox === true && (
          <span className="ph-txc-cell ph-txc-warn" data-txc="vox" title={t('phone.tx.voxWarn')}>
            {VOX_ON}
          </span>
        )}
        {/* WHO HOLDS THE TRANSMITTER, in the arbiter's own words — `tx_owner()` covers all
            seven owners, and a second wording here could disagree with the one that really
            holds the rig. Absent when nobody does: an idle transmitter is not news. */}
        {snap.radio.txBusyReason && (
          <span className="ph-txc-cell ph-txc-busy" data-txc="busy" title={snap.radio.txBusyReason}>
            {snap.radio.txBusyReason}
          </span>
        )}
      </div>

      {shown('txmeters') && <TxMeters radio={snap.radio} pinned />}

      {/* ⚠️ NOTHING IN THIS ROW IS MIGRATED, and that is the whole of why this file is on the
          i18n PARTIAL list. PTT is Phone's stop-line census (features/panelState.ts) and
          components/stop-line.test.tsx finds it by ACCESSIBLE NAME, matching all four of the
          labels below; its tooltip IS that control's description, naming the switch that is
          down and the mic the operator talks on. The Lock toggle beside it decides whether
          the window's Space keyup is a PTT release at all — the census's fourth holder — and
          the Field Day chip shares the row. All of it moves in the transmit-path batch, with
          the stop-line sweeps re-run. The TOASTS this row's handler raises did move (see
          `key` above): a toast is not a control, and no sweep can see one. */}
      <div className="ph-ptt-row">
        {fdExchange && (
          <span
            className="ph-fd-give"
            title="Field Day exchange — read this to the station you're working (your class + section)."
          >
            <span className="ph-fd-give-lbl">Give</span>
            <span className="ph-fd-give-exch mono">{fdExchange}</span>
          </span>
        )}
        {/* THE BUTTON ANSWERS THREE STATES, NOT TWO (#81). The engine drops a key on either of
            `tx_enabled` / `tx_allowed` and they are different facts with different remedies:
            the lock is a licence question (change band or licence class), TX-off is a switch
            the operator himself, the watchdog or a UDP HaltTx put down. One label covering both
            is what made #81 unreportable — "PUSH TO TALK" over a rig that could not be keyed,
            with the operator left to guess between his cable and his software. */}
        <button
          type="button"
          className={`ph-ptt${keyed ? ' keyed' : ''}${
            snap.radio.txAllowed && !snap.radio.txEnabled ? ' txoff' : ''
          }`}
          aria-pressed={lock ? keyed : undefined}
          onPointerDown={onPttDown}
          onPointerUp={onPttUp}
          onPointerLeave={onPttUp}
          // Keyboard: in Lock (hands-free) mode a focused Enter/Space toggles TX.
          // preventDefault suppresses the synthetic click so it can't double-fire
          // against the pointer handlers on a real mouse click. In hold mode the
          // window-level Space handler owns push-to-talk (a keypress can't hold).
          onKeyDown={(e) => {
            if (lock && (e.key === 'Enter' || e.key === ' ')) {
              e.preventDefault()
              key(!keyed)
            }
          }}
          disabled={!control || (!snap.radio.txAllowed)}
          // The last clause was a `.ph-ptt-hint` span below this row until 2026-08-04.
          // `flex-basis: 100%` made it a whole line of the PINNED dock, so it cost its ~19px
          // at every window size and no scroller could take it back — and its first half
          // ("Hold the button or the Space bar") repeated this very tooltip. The row went;
          // the sentence did not, because "you talk on the rig's mic" is what stops an
          // operator keying up believing Nexus carries his audio. Pinned in
          // PhoneCockpit.structure.test.tsx.
          title={
            !snap.radio.txAllowed
              ? 'TX locked — outside your license privileges (pick a band, or change your license in Settings)'
              : !snap.radio.txEnabled
                ? "Transmit is switched OFF, so keying is discarded — Stop TX, the TX watchdog or a logger's Halt Tx turns it off. Click to enable transmit, then hold to talk. You talk on the rig's mic."
                : "Hold to talk (or Space). Toggle 'Lock' for hands-free (then Enter keys/unkeys). You talk on the rig's mic."
          }
        >
          {!snap.radio.txAllowed
            ? '🔒 TX LOCKED'
            : !snap.radio.txEnabled
              ? '■ TX OFF — CLICK TO ENABLE'
              : keyed
                ? 'ON AIR — release to stop'
                : 'PUSH TO TALK'}
        </button>
        <label className="ph-lock" title="Hands-free: click PTT once to key, again to unkey">
          <input disabled={!control} type="checkbox" checked={lock} onChange={(e) => setLock(e.target.checked)} />
          <span>Lock</span>
        </label>
      </div>
      </div>

      <SpotDialog
        open={spotOpen}
        onClose={() => setSpotOpen(false)}
        initialCall={spotCall}
        freqMhz={snap.radio.dialMhz}
        // The rig's own mode, not the command — a spot posted from an AM QSO said "USB" for
        // the same reason the log did. The picker's vocabulary, not the ADIF one: a DX-cluster
        // comment wants the sideband word an operator would type ("USB"), not "SSB".
        defaultComment={observedMode}
      />
    </main>
  )
}

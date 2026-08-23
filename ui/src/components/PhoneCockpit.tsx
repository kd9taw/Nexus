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
import { panelHost, NO_DSP_FUNCS_REASON, NO_DSP_LEVELS_REASON } from '../features/panelHost'
import type { AppSnapshot, FieldDayStatus, NeedTag, SpotRow } from '../types'
import { PhoneScope } from './PhoneScope'
import { TxMeters, TX_METERS_WHEN } from './TxMeters'
import { BandStrip } from './BandStrip'
import { PanelsMenu } from './PanelsMenu'
import { SpotDialog } from './SpotDialog'
import { TuningStrip } from './TuningStrip'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { Splitter, SCOPE_SPLIT_MAX, SCOPE_SPLIT_MIN } from './Splitter'
import { PalettePicker } from './PalettePicker'
import { BandPicker } from './BandPicker'
import { VoiceKeyer } from './VoiceKeyer'
import { LiveLevelMeter, useSmeterDb } from './LiveMeters'
import { LogEntry } from './LogEntry'
import {
  setPtt,
  setRfPower,
  setMicGain,
  setNrLevel,
  setCompLevel,
  setNotchFreq,
  setAgc,
  setScopeSpan,
  setScopeRef,
  setFlexPanSpan,
  setFlexPanRef,
  startQsoRecording,
  stopQsoRecording,
  setTune,
  atuTune,
  haltTx,
  setTxEnabled,
} from '../api'
import { pushToast } from '../toast'
import { RotorStrip } from './RotorStrip'
import { MemoryStrip } from './MemoryStrip'
import type { Memory } from '../features/memories'
import { setFrequency, setSplit, setRigFunc, setSidebandOverride, setFilterWidth, openPanelWindow } from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { isRfScopeSource, NO_NATIVE_SCOPE_REASON } from '../waterfall'
import { useWheelTune } from '../useWheelTune'
import { useScopeTune } from '../useScopeTune'
import { useRegionCols } from '../useRegionCols'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'

/** This cockpit's INVARIANT vocabulary — the words that are technical tokens rather than
 *  prose, gathered here so the i18n guard reads them as the deliberate constants they are:
 *  the rig's own control-group names, the units the readouts print, and the two plates. */
const DSP = 'DSP'
const NR = 'NR'
const AGC = 'AGC'
// The rig's own names for the two controls #95 completed. COMP is the speech processor
// (PROC on a Yaesu front panel); NOTCH is the MANUAL notch, not the automatic one.
const COMP = 'COMP'
const NOTCH = 'NOTCH'
const HZ = 'Hz'
const BW = 'BW'
const DB = 'dB'
const DBM = 'dBm'
const REC = 'REC'
const SPLIT_PLATE = 'SPLIT'
/** The mode picker's AUTO face — the word and the sideband it resolved to, both tokens. */
const autoPlate = (sideband: string) => `AUTO·${sideband}`
/** The nudges the two steppers take, as their tooltips print them — figures, so they are
 *  supplied to the message rather than written in it. */
const FILTER_STEP_HZ = 100
const SPLIT_STEP_KHZ = 1

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
  dsp: t('phone.panel.dsp'),
  dspLevels: t('phone.panel.dspLevels'),
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
 * cockpit only renders those the rig reports as supported (field non-null), so no dead buttons. */
const DSP_FUNCS = [
  { key: 'nb', label: 'NB', titleKey: 'phone.dsp.nb.title' },
  { key: 'nr', label: 'NR', titleKey: 'phone.dsp.nr.title' },
  { key: 'notch', label: 'Notch', titleKey: 'phone.dsp.notch.title' },
  { key: 'comp', label: 'COMP', titleKey: 'phone.dsp.comp.title' },
  { key: 'vox', label: 'VOX', titleKey: 'phone.dsp.vox.title' },
  // #95. TWO notches, and they are different controls: `notch` above is the AUTOMATIC
  // notch (Hamlib ANF), which hunts a carrier on its own; this is the MANUAL one you
  // park on a heterodyne, and it is what most operators mean by "notch". Each renders
  // only when the rig reports it, so a radio with one shows one button.
  { key: 'manualNotch', label: 'MN', titleKey: 'phone.dsp.manualNotch.title' },
] as const

/** Bandscope span presets — the width of OCCUPIED SIDEBAND to show, because the scope's axis
 *  is the rig's: RF offset from the dial, dial at the 1/4 mark and the sideband filling the
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

export function PhoneCockpit({ snap, theme, pendingWork, onConsumeWork, onSnap, fieldDay, phoneMode, wheelSensitivity, spots, needByCall, typeByCall, onWorkSpot, onRecallMemory, onOpenMemories, onOpenSettings, panels }: Props) {
  // Live S-meter (shared 100 ms poll, lock-free backend) — used to arrive via the 300 ms
  // snapshot on top of the backend's own sampling, which read as a laggy needle. smeterDb-only
  // subscription: the cockpit re-renders when the S-meter changes, never on RX-level churn.
  const smeterDb = useSmeterDb()
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
  const changeMic = (pct: number) => {
    setMic(pct)
    void setMicGain(pct / 100)
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
  const changeComp = (pct: number) => {
    setComp(pct)
    void setCompLevel(pct / 100)
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
  const changeNotch = (hz: number) => {
    setNotchHz(hz)
    void setNotchFreq(hz)
  }
  const changeNr = (pct: number) => {
    setNr(pct)
    void setNrLevel(pct / 100)
  }
  // AGC speed — the chip lights on the click, then the rig gets the last word. Reading
  // snap.radio.agc directly lagged ~0.75–1.5 s: setAgc's snapshot returns the OLD rig read-back
  // (rig_agc.or(agc)) until the next RX poll, so the clicked chip wouldn't light up. DERIVED
  // rather than a remembered mirror, because a mirror synced only "when the snapshot changes"
  // is blind to a REFUSAL — a rig whose Hamlib backend lacks that AGC step answers RPRT -1, the
  // read-back never moves, and the chip claimed a speed the radio never took.
  const [agcPick, setAgcPick] = useState<string | null>(null)
  const agc =
    agcPick != null && agcPick !== snap.radio.refusedAgc ? agcPick : (snap.radio.agc ?? null)
  const changeAgc = (sp: 'auto' | 'fast' | 'mid' | 'slow' | 'off') => {
    setAgcPick(sp)
    void setAgc(sp)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  // Native Icom scope reference level, in tenths of a dB (−200..+200 = −20.0..+20.0 dB).
  const [scopeRefTenths, setScopeRefTenths] = useState(0)
  const changeScopeRef = (tenths: number) => {
    setScopeRefTenths(tenths)
    void setScopeRef(tenths)
  }
  const [keyed, setKeyed] = useState(false)
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
  // True only when the rig's own Icom scope is streaming (span/ref are Icom CI-V commands; the
  // Flex panadapter has a different control path, so gate on 'civ' specifically, not any RF feed).
  const civScope = scopeFeed?.source === 'civ'
  // FlexRadio SmartSDR panadapter — its own span/ref command path (display pan set …).
  const flexScope = scopeFeed?.source === 'flex'
  const [flexRefDbm, setFlexRefDbm] = useState(-80)
  const changeFlexRef = (dbm: number) => {
    setFlexRefDbm(dbm)
    void setFlexPanRef(dbm)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  const [lock, setLock] = useState(false) // hands-free PTT (toggle instead of hold)
  const [recBusy, setRecBusy] = useState(false) // in-flight guard for the record toggle
  const [spotOpen, setSpotOpen] = useState(false) // spot-to-cluster popup
  const [spotCall, setSpotCall] = useState('') // seed: '' from the toolbar, the typed call from LogEntry
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
  const scopeRef = useRef<HTMLDivElement>(null)
  // Cockpit root: the scope-height splitter measures + writes its CSS var here.
  const cockpitRef = useRef<HTMLElement>(null)
  // Which DSP funcs this rig has EVER reported this session — see the sticky note at the DSP pane.
  const seenDspFuncs = useRef<string[]>([])
  const seenDspLevels = useRef(false)
  useWheelTune(scopeRef, {
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
  // Bands where AM is worked: the HF windows below 10 MHz, and 10 m / 6 m and up. Mirrors the
  // comment on the picker's filter below.
  const amBandOk = snap.radio.dialMhz > 0 && (snap.radio.dialMhz < 10 || snap.radio.dialMhz >= 28)
  const commandedMode = modeOverride ?? sidebandAuto
  const pickMode = (m: 'USB' | 'LSB' | 'FM' | 'AM' | null) =>
    void setSidebandOverride(m)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('phone.mode.failed'), 'error'))
  // Whether the app can actually control the rig. Without CAT (VOX/serial PTT) the dial +
  // mode can't be set or read back — surface that so it's clear, not silently broken.
  const catOk = snap.radio.catOk === true

  // Click/drag tuning from the bandscope (Flex-style): clicks command immediately, a
  // drag coalesces to one CAT write per ~120 ms. PhoneScope does the signal-snap math
  // and reports the final dial; this hook just commands it.
  const onScopeTune = useScopeTune({
    sideband: commandedMode,
    enabled: catOk && !snap.radio.txBusyReason && !snap.radio.transmitting,
    onSnap,
  })

  // RX filter / passband width — the rig's read-back (null = unknown/default). The ± stepper
  // nudges it 100 Hz within a sane SSB/CW span, seeded from the current value or a 2.4 kHz default.
  const filterHz = snap.radio.filterWidthHz ?? null
  const bumpFilter = (deltaHz: number) => {
    const base = filterHz ?? 2400
    const next = Math.min(4000, Math.max(300, base + deltaHz))
    // Never let the clamp invert the direction ("wider" must not narrow at the rails).
    if ((deltaHz > 0 && next <= base) || (deltaHz < 0 && next >= base)) return
    void setFilterWidth(next)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('phone.filter.failed'), 'error'))
  }

  // Rig's actual mode read back over CAT (display-only). The app's `commandedMode` stays
  // canonical for TX/logging; this just flags when the rig's mode disagrees, so the badge
  // never silently lies.
  const rigMode = (snap.radio.rigMode ?? '').toUpperCase()
  // Collapse ONLY the FM variants (FMN/WFM → FM). Deliberately do NOT strip PKT/data suffixes:
  // in Phone a rig stuck in PKTUSB / DATA-U (rear-jack audio → dead mic) vs a commanded USB is a
  // REAL operational mismatch worth flagging, not a cosmetic naming variant.
  const rigFamily = /^W?FM/.test(rigMode) ? 'FM' : rigMode
  const modeMismatch = catOk && rigMode !== '' && rigFamily !== commandedMode ? rigMode : null

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

  // Manual split (casual DX "work up N"): the desired TX dial lives in the snapshot; a plain
  // retune clears it (backend). Offset is kHz off the RX dial; default +5, the common pileup.
  const [splitOffsetKhz, setSplitOffsetKhz] = useState(5)
  const splitTxMhz = snap.radio.splitTxMhz ?? null
  const splitOn = splitTxMhz != null
  // When split turns on externally (e.g. a pile-up spot programs it), sync the local offset
  // to the rig so the display + bumping start from the real value, not a stale default.
  const wasSplitOn = useRef(false)
  useEffect(() => {
    if (splitOn && !wasSplitOn.current && splitTxMhz != null) {
      setSplitOffsetKhz(Math.round((splitTxMhz - snap.radio.dialMhz) * 1000))
    }
    wasSplitOn.current = splitOn
  }, [splitOn, splitTxMhz, snap.radio.dialMhz])
  const applySplitTx = (offsetKhz: number) =>
    setSplit(snap.radio.dialMhz + offsetKhz / 1000)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('phone.split.setFailed'), 'error'))
  const toggleSplit = () =>
    splitOn
      ? setSplit(null)
          .then((s) => onSnap?.(s))
          .catch(() => pushToast(t('phone.split.clearFailed'), 'error'))
      : applySplitTx(splitOffsetKhz)
  // Accumulate on local state (functional updater) so rapid bumps that fire before the
  // IPC/onSnap round-trip don't all read the same stale value and collapse into one step.
  const bumpSplit = (delta: number) =>
    setSplitOffsetKhz((prev) => {
      const next = Math.max(-90, Math.min(90, prev + delta))
      if (splitOn) void applySplitTx(next)
      return next
    })

  // Live snapshot ref so the spacebar PTT handler (bound on `lock` changes, not every render)
  // reads the CURRENT TX-allowed privilege state through key() — not whatever existed when bound.
  const snapRef = useRef(snap)
  snapRef.current = snap
  const key = (on: boolean) => {
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
    if (lock) {
      key(!keyed) // hands-free: toggle
    } else {
      key(true)
    }
  }
  const onPttUp = () => {
    if (!lock) key(false)
  }
  const changePower = (pct: number) => {
    setPower(pct)
    void setRfPower(pct / 100)
  }
  // Commit a typed dial from the shared header readout — same CAT path as the
  // TuningStrip nudge/wheel (keeps the current sideband so an in-band entry
  // never flips the mode); rejects out-of-plan frequencies with a toast.
  const commitDial = (mhz: number) => {
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
    const isField = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !e.repeat && !isField(e.target) && !lock) {
        e.preventDefault()
        key(true)
      }
    }
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !isField(e.target) && !lock) {
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
  // slot). Class + Section from the active ruleset; empty until FD setup fills them in.
  const fdExchange = fieldDay ? `${fieldDay.myClass} ${fieldDay.mySection}`.trim() : ''

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
  const liveDspFuncs = DSP_FUNCS.filter((f) => snap.radio[f.key] != null)
  // Whether each rig-gated pane CAN render at all on this station, independent of the ⊞
  // tick. Computed here, above the menu, so the entry's note and the pane read ONE boolean
  // each and cannot drift apart — which is the whole failure the notes exist to answer.
  // CAPABILITY, not the learned flag: the sticky learns just below only run while the pane
  // is SHOWN, so keying availability off the ref alone would leave an operator who unticked
  // before the rig first reported stuck with a dead entry once it does.
  const canDsp = liveDspFuncs.length > 0 || seenDspFuncs.current.length > 0
  const canDspLevels =
    seenDspLevels.current ||
    snap.radio.nrLevel != null ||
    snap.radio.agc != null ||
    // #95's two, and leaving them out is not a detail: this boolean decides whether the pane
    // EXISTS. A rig reporting a speech-processor depth or a notch frequency but no NR and no
    // AGC would have had both new controls built and then never rendered — the dead-entry
    // failure this whole block is written against, arriving from the other side. Caught by
    // PhoneCockpit.notch.test.tsx before it shipped.
    snap.radio.compLevel != null ||
    snap.radio.notchFreqHz != null
  // ⊞ Panels. `main`/`side` are unused here (Phone has no two-column pane grid), so the
  // host is only supplying `shown` + the menu items.
  //
  // Five notes, of two different kinds — see panelHost's `notes` doc, which is why they
  // share one field.
  //
  // FOUR ARE "nothing on screen behind this box" — the dead-checkbox the operator hit on
  // 2026-08-03. Three of those are conditional on the STATION and clear by themselves the
  // moment the rig can feed them: the rig-scope pane needs the RADIO's own panadapter
  // streaming, and the two DSP panes need the rig to report those fields over CAT (the same
  // capability gate the DSP row itself has always applied to its buttons). The fourth, TX
  // Meters, mounts fine and reads only while keyed.
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
          dsp: canDsp ? undefined : NO_DSP_FUNCS_REASON,
          dspLevels: canDspLevels ? undefined : NO_DSP_LEVELS_REASON,
          txmeters: TX_METERS_WHEN,
          voiceKeyer: VOICE_KEYER_STOPS_ON_HIDE,
        },
      })
    : null
  const shown = (id: PhonePanelId) => (host ? host.shown(id) : true)
  if (shown('dsp') && liveDspFuncs.length > 0) seenDspFuncs.current = liveDspFuncs.map((f) => f.key)
  const dspFuncs =
    liveDspFuncs.length > 0
      ? liveDspFuncs
      : DSP_FUNCS.filter((f) => seenDspFuncs.current.includes(f.key))
  // Same sticky rule for the NR/AGC levels pane: a CAT re-confirmation nulls these too,
  // and unmounting on that made the pane flicker away on every QSY.
  if (shown('dspLevels') && (snap.radio.nrLevel != null || snap.radio.agc != null))
    seenDspLevels.current = true

  // Which panes CAN render right now — the exact conditions that gate them in the JSX,
  // hoisted because the region's column budget (maxCols) depends on them.
  const hasBandPane = onWorkSpot != null && shown('bandActivity')
  const hasKeyerPane = shown('voiceKeyer')
  const hasRigScopePane = shown('rigscope') && (civScope || flexScope)
  const hasDspPane = shown('dsp') && canDsp
  const hasDspLevelsPane = shown('dspLevels') && canDspLevels
  const auxPresent = hasRigScopePane || hasDspPane || hasDspLevelsPane
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

  const bandPane =
    hasBandPane && onWorkSpot ? (
      <CockpitPaneFrame title={t('phone.pane.bandActivity.title')} paneId="bandActivity" fit="content">
        <BandStrip
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
        />
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
    <CockpitPaneFrame title={t('phone.pane.voiceKeyer.title')} paneId="voiceKeyer" fit="content">
      <VoiceKeyer
        txEnabled={snap.radio.txEnabled}
        keyed={keyed}
        transmitting={snap.radio.transmitting}
        fdExchange={fdExchange}
      />
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
        <CockpitPaneFrame title={t('phone.pane.rigscope.title')} paneId="rigscope" fit="content">
          <div className="ph-rigscope" role="group" aria-label={t('phone.rigScope.aria')}>
            <span className="ph-rigscope-lbl" title={t('phone.rigScope.title')}>
              {t('phone.rigScope.label')}
            </span>
            <div className="ph-span">
              {RIG_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('phone.rigScope.span.title', { span: sp.label })}
                  onClick={() => void setScopeSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('phone.rigScope.ref.title')}>
              <span>{t('phone.scope.ref.label')}</span>
              <input
                type="range"
                min={-200}
                max={200}
                step={5}
                value={scopeRefTenths}
                onChange={(e) => changeScopeRef(Number(e.target.value))}
                aria-label={t('phone.rigScope.ref.aria')}
              />
              <span className="ph-power-val">{(scopeRefTenths / 10).toFixed(1)} {DB}</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {/* FlexRadio SmartSDR panadapter controls — command the Flex pan's real bandwidth + ref. */}
      {hasRigScopePane && flexScope && (
        <CockpitPaneFrame title={t('phone.pane.rigscope.title')} paneId="rigscope" fit="content">
          <div className="ph-rigscope" role="group" aria-label={t('phone.flexPan.aria')}>
            <span className="ph-rigscope-lbl" title={t('phone.flexPan.title')}>
              {t('phone.flexPan.label')}
            </span>
            <div className="ph-span">
              {FLEX_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('phone.flexPan.span.title', { span: sp.label })}
                  onClick={() => void setFlexPanSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('phone.flexPan.ref.title')}>
              <span>{t('phone.scope.ref.label')}</span>
              <input
                type="range"
                min={-140}
                max={-20}
                step={5}
                value={flexRefDbm}
                onChange={(e) => changeFlexRef(Number(e.target.value))}
                aria-label={t('phone.flexPan.ref.aria')}
              />
              <span className="ph-power-val">{flexRefDbm} {DBM}</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {hasDspPane && (
        <CockpitPaneFrame title={t('phone.pane.dsp.title')} paneId="dsp" fit="content">
          <div className="ph-dsp" role="group" aria-label={t('phone.dsp.aria')}>
            <span className="ph-dsp-label">{DSP}</span>
            {dspFuncs.map((f) => {
              const on = snap.radio[f.key] === true
              return (
                <button
                  key={f.key}
                  type="button"
                  className={`ph-dsp-btn${on ? ' on' : ''}`}
                  aria-pressed={on}
                  title={t(f.titleKey)}
                  onClick={() =>
                    void setRigFunc(f.key, !on)
                      .then((s) => onSnap?.(s))
                      .catch(() => pushToast(t('phone.dsp.toggleFailed', { func: f.label }), 'error'))
                  }
                >
                  {f.label}
                </button>
              )
            })}
          </div>
        </CockpitPaneFrame>
      )}

      {/* RX DSP levels — NR level slider + AGC speed, each shown only when the rig reports it. */}
      {hasDspLevelsPane && (
        <CockpitPaneFrame title={t('phone.pane.dspLevels.title')} paneId="dspLevels" fit="content">
          <div className="ph-dsp-levels" role="group" aria-label={t('phone.rxDsp.aria')}>
            {snap.radio.nrLevel != null && (
              <label className="ph-dsplev" title={t('phone.rxDsp.nr.title')}>
                <span>{NR}</span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={nr}
                  onChange={(e) => changeNr(Number(e.target.value))}
                  onPointerDown={() => {
                    nrDragging.current = true
                  }}
                  onPointerUp={() => {
                    nrDragging.current = false
                  }}
                  aria-label={t('phone.rxDsp.nr.aria')}
                />
                <span className="ph-power-val">{nr}%</span>
              </label>
            )}
            {/* #95: the speech processor's DEPTH. The toggle switched PROC on and there was
                no way to say how hard it worked, which is the half that matters. */}
            {snap.radio.compLevel != null && (
              <label className="ph-dsplev" title={t('phone.rxDsp.comp.title')}>
                <span>{COMP}</span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={comp}
                  onChange={(e) => changeComp(Number(e.target.value))}
                  onPointerDown={() => {
                    compDragging.current = true
                  }}
                  onPointerUp={() => {
                    compDragging.current = false
                  }}
                  aria-label={t('phone.rxDsp.comp.aria')}
                />
                <span className="ph-power-val">{comp}%</span>
              </label>
            )}
            {/* #95: WHERE the manual notch sits. Shown only when the rig reports NOTCHF, so a
                radio with an auto-notch and nothing else never grows a slider that does
                nothing. Hz, not a percentage — you are placing it on a tone you can hear. */}
            {snap.radio.notchFreqHz != null && (
              <label className="ph-dsplev" title={t('phone.rxDsp.notchFreq.title')}>
                <span>{NOTCH}</span>
                <input
                  type="range"
                  min={300}
                  max={3400}
                  step={10}
                  value={notchHz}
                  onChange={(e) => changeNotch(Number(e.target.value))}
                  onPointerDown={() => {
                    notchDragging.current = true
                  }}
                  onPointerUp={() => {
                    notchDragging.current = false
                  }}
                  aria-label={t('phone.rxDsp.notchFreq.aria')}
                />
                <span className="ph-power-val">{notchHz} {HZ}</span>
              </label>
            )}
            {snap.radio.agc != null && (
              <div className="ph-agc" role="group" aria-label={t('phone.rxDsp.agc.aria')} title={t('phone.rxDsp.agc.title')}>
                <span className="ph-dsplev-lbl">{AGC}</span>
                {AGC_CHIPS.map(({ id, labelKey }) => (
                  <button
                    key={id}
                    type="button"
                    className={`theme-chip${agc === id ? ' active' : ''}`}
                    aria-pressed={agc === id}
                    onClick={() => changeAgc(id)}
                  >
                    {t(labelKey)}
                  </button>
                ))}
              </div>
            )}
          </div>
        </CockpitPaneFrame>
      )}
    </>
  )

  const logPane = (
    <CockpitPaneFrame title={t('phone.pane.log.title')} paneId="log">
      {/* compactRecall died here (2026-07-31). It existed because the pre-overhaul cockpit had
          no interposed scroller: the full recall card's height crushed the operating panes
          directly. This pane is now a FILL pane whose .pane-body scrolls internally, so a tall
          card scrolls inside the log column and can never squeeze the cockpit — the operator
          gets the QRZ photo / bearing / history back while operating. */}
      <LogEntry
        snap={snap}
        mode={commandedMode === 'FM' ? 'FM' : 'SSB'}
        defaultRst="59"
        exchange="terrestrial"
        // The frame head above already reads LOG and is this pane's accessible name.
        titled={false}
        onSpot={(call) => {
          setSpotCall(call)
          setSpotOpen(true)
        }}
        pendingWork={pendingWork}
        onConsumeWork={onConsumeWork}
        fieldDay={fieldDay}
        fdMode="PH"
      />
    </CockpitPaneFrame>
  )

  return (
    <main className="layout single phone-cockpit" ref={cockpitRef}>
      <CockpitHeader
        snap={snap}
        onSnap={onSnap}
        modeIndicator={
          <div className="ph-mode-pick" role="group" aria-label={t('phone.mode.aria')}>
            {(['AUTO', 'USB', 'LSB', 'FM', 'AM'] as const)
              // AM IS OFFERED ONLY WHERE AM IS ACTUALLY WORKED — below 10 MHz (the 1.885,
              // 3.870-3.890 and 7.290 windows) and at 28 MHz and up (29.0-29.2, 50.4 and above).
              // 20/17/15/12 m are not AM territory, and a 6 kHz emission there has no room in the
              // band plan.
              //
              // NOT OFFERING it rather than refusing it, deliberately. The cockpit pick is the
              // operator's own CURRENT action and this app honours those — `sideband_override` is
              // already cleared by a band change, a cross-band knob QSY, working a spot and an
              // SSTV tune, so unlike the station-wide `phone_mode` it cannot follow anybody onto a
              // band they did not choose it for. There is no stale state to protect against here;
              // the only thing worth doing is not putting a button in front of somebody on a band
              // where it makes no sense.
              .filter((m) => m !== 'AM' || amBandOk)
              .map((m) => {
              const active = m === 'AUTO' ? modeOverride === null : modeOverride === m
              return (
                <button
                  key={m}
                  type="button"
                  className={`ph-mode-btn${active ? ' active' : ''}`}
                  aria-pressed={active}
                  disabled={!catOk}
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
          value: power,
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
        {catOk && (
          <div className={`ph-split ${splitOn ? 'on' : ''}`}>
            <button
              className="ph-split-toggle"
              onClick={toggleSplit}
              title={
                splitOn
                  ? t('phone.split.on.title', { freq: splitTxMhz?.toFixed(3) ?? '' })
                  : t('phone.split.off.title')
              }
            >
              {SPLIT_PLATE}
            </button>
            <button
              className="ph-split-step"
              onClick={() => bumpSplit(-SPLIT_STEP_KHZ)}
              title={t('phone.split.lower.title', { step: SPLIT_STEP_KHZ })}
            >
              −
            </button>
            <span className="ph-split-amt mono" title={t('phone.split.offset.title')}>
              {splitOffsetKhz >= 0 ? `+${splitOffsetKhz}` : `${splitOffsetKhz}`}
            </span>
            <button
              className="ph-split-step"
              onClick={() => bumpSplit(SPLIT_STEP_KHZ)}
              title={t('phone.split.higher.title', { step: SPLIT_STEP_KHZ })}
            >
              +
            </button>
          </div>
        )}
        {!catOk && (
          <span
            className="ph-nocat"
            title={snap.radio.catDetail || t('phone.noCat.title')}
          >
            {t('phone.noCat.label')}
          </span>
        )}
        {snap.radio.micGain != null && (
          <label className="ph-power" title={t('phone.mic.title')}>
            <span>{t('phone.mic.label')}</span>
            <input
              type="range"
              min={0}
              max={100}
              value={mic}
              onChange={(e) => changeMic(Number(e.target.value))}
              onPointerDown={() => {
                micDragging.current = true
              }}
              onPointerUp={() => {
                micDragging.current = false
              }}
              aria-label={t('phone.mic.aria')}
            />
            <span className="ph-power-val">{mic}%</span>
          </label>
        )}
        {catOk && commandedMode !== 'FM' && (
          <div className="ph-filter" title={t('phone.filter.title')}>
            <span className="ph-filter-lbl">{BW}</span>
            <button
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
            <button
              type="button"
              className="ph-filter-step"
              onClick={() => bumpFilter(FILTER_STEP_HZ)}
              title={t('phone.filter.wider.title', { step: FILTER_STEP_HZ })}
            >
              +
            </button>
          </div>
        )}
        {onRecallMemory && (
          // The ★-favorites quick-recall strip (bounded + wrapping — the old
          // MemoryBank list grew the header unbounded). Recall is App-owned
          // (recallMemory): settings patch + retune + cockpit auto-switch.
          <MemoryStrip
            dialMhz={snap.radio.dialMhz}
            mode={commandedMode}
            onRecall={onRecallMemory}
            onManage={onOpenMemories}
          />
        )}
        <RotorStrip onOpenSettings={onOpenSettings} />
        {/* Glyph only (density pass 2026-08-04, the same move the FT cockpit's header made):
            '● Record QSO' spent ~95px of a header region that WRAPS, and the word said what
            the glyph and the tooltip already say. The accessible name is explicit here rather
            than left to the glyph — a screen reader must not be handed a bare bullet. */}
        <button
          type="button"
          className={`ph-rec${recording ? ' on' : ''}`}
          onClick={toggleRecord}
          disabled={recBusy}
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
          <LiveLevelMeter label={t('phone.rxMeter.label')} variant="compact" />
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
      <section className="ph-scope-panel">
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
        </div>
        <div className="ph-scope-wrap" ref={scopeRef} title={t('phone.scope.tuneHint')}>
          {nativeRf ? (
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
            transmitting={snap.radio.transmitting}
            theme={theme}
            smeterDb={smeterDb}
            viewLoHz={nativeRf ? rfSpan.lo : 0}
            viewHiHz={nativeRf ? rfSpan.hi : (span.widthHz || Math.min(4000, Math.max(800, filterHz ?? 4000)))}
            carrierCentered={!nativeRf}
            sideband={commandedMode}
            dialHz={snap.radio.dialMhz > 0 ? Math.round(snap.radio.dialMhz * 1e6) : null}
            onFeed={(source, loHz, hiHz) => setScopeFeed({ source, loHz, hiHz })}
            onTune={onScopeTune}
            filterWidthHz={filterHz ?? 2400}
            interactive={catOk && !snap.radio.txBusyReason && !snap.radio.transmitting && snap.radio.dialMhz > 0}
          />
        </div>
      </section>
      <Splitter
        axis="y"
        varName="--ph-scope-h"
        target={cockpitRef}
        storageKey="nexus.split.phone.scope"
        min={SCOPE_SPLIT_MIN}
        max={SCOPE_SPLIT_MAX}
        defaultPct={22}
        label={t('phone.scope.splitter.label')}
      />
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
      <div className="cockpit-panes" ref={panesRef}>
        {cols === 3 ? (
          <>
            <div className="cockpit-col" key="main">
              {bandPane}
              {keyerPane}
            </div>
            <div className="cockpit-col" key="aux">{auxPanes}</div>
            <div className="cockpit-col" key="log">{logPane}</div>
          </>
        ) : (
          <>
            {leadPresent && (
              <div className="cockpit-col" key="main">
                {bandPane}
                {keyerPane}
                {auxPanes}
              </div>
            )}
            <div className="cockpit-col" key="log">{logPane}</div>
          </>
        )}
      </div>

      {/* TX DOCK — the transmit chrome, pinned OUTSIDE the pane region so no pane
          layout, stored or hand-edited, can move, hide or scroll it away. PTT and Lock
          have no id in the panel vocabulary (unrepresentable beats guarded); the TX
          meters DO keep their ⊞ id — a readout, not a control — but render here, beside
          the button that keys the rig (they appear only while keyed). */}
      <div className="cockpit-txdock">
      {/* Transmit meters (SWR/ALC/Po/COMP) — appear only while keyed. At the TOP of the
          dock, exactly as in CW: the dock is bottom-anchored (sticky bottom), so a child
          mounting BELOW the PTT row would grow the dock upward and shift the button out
          from under the operator's held pointer mid-over — onPointerLeave would then
          unkey the rig. Growth above the row leaves the row's screen position fixed. */}
      {shown('txmeters') && <TxMeters radio={snap.radio} />}

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
          disabled={!snap.radio.txAllowed}
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
          <input type="checkbox" checked={lock} onChange={(e) => setLock(e.target.checked)} />
          <span>Lock</span>
        </label>
      </div>
      </div>

      <SpotDialog
        open={spotOpen}
        onClose={() => setSpotOpen(false)}
        initialCall={spotCall}
        freqMhz={snap.radio.dialMhz}
        defaultComment={commandedMode}
      />
    </main>
  )
}

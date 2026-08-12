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
  setAgc,
  setScopeSpan,
  setScopeRef,
  setFlexPanSpan,
  setFlexPanRef,
  startQsoRecording,
  stopQsoRecording,
  setTune,
  haltTx,
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
const PHONE_PANEL_LABELS: Record<PhonePanelId, string> = {
  rigscope: 'Rig Scope Controls',
  txmeters: 'TX Meters',
  dsp: 'DSP Functions',
  dspLevels: 'RX DSP Levels',
  bandActivity: 'Band Activity',
  voiceKeyer: 'Voice Keyer',
}

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
  { key: 'nb', label: 'NB', title: 'Noise Blanker — kills impulse/ignition noise (RX)' },
  { key: 'nr', label: 'NR', title: 'Noise Reduction — pulls voice out of broadband hiss (RX, DSP)' },
  { key: 'notch', label: 'Notch', title: 'Auto-Notch (ANF) — nulls carriers/heterodynes (RX, DSP)' },
  { key: 'comp', label: 'COMP', title: 'Speech Compressor — more average talk power (TX)' },
  { key: 'vox', label: 'VOX', title: 'Voice-Operated Transmit — hands-free keying (TX)' },
] as const

/** Bandscope span presets — slices of the captured audio passband. */
const SPANS = [
  { label: 'Full', lo: 0, hi: 4000, title: 'Whole captured band (0–4000 Hz) — incl. the filter slopes' },
  { label: 'Voice', lo: 300, hi: 2700, title: 'Voice energy (300–2700 Hz)' },
  { label: 'Low', lo: 200, hi: 1500, title: 'Lower half — zoomed (200–1500 Hz)' },
  { label: 'High', lo: 1500, hi: 2900, title: 'Upper half — zoomed (1500–2900 Hz)' },
] as const

/** RF panadapter zoom presets (used only when a native RF scope is streaming). Symmetric ±Hz
 *  windows centered on the dial — scopeView maps these to absolute RF and clamps to the swept
 *  span, so "Full" (a huge window) shows the rig's WHOLE sweep rather than a passband-width sliver. */
const RF_SPANS = [
  { label: 'Full', lo: -1e9, hi: 1e9, title: "The rig's whole scope sweep (set the width on the radio)" },
  { label: '±25k', lo: -25_000, hi: 25_000, title: '±25 kHz around your dial' },
  { label: '±10k', lo: -10_000, hi: 10_000, title: '±10 kHz around your dial' },
  { label: '±5k', lo: -5_000, hi: 5_000, title: '±5 kHz around your dial' },
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
  const changeNr = (pct: number) => {
    setNr(pct)
    void setNrLevel(pct / 100)
  }
  // AGC speed — local optimistic mirror so the segmented highlight flips on click. Reading
  // snap.radio.agc directly lagged ~0.75–1.5 s: setAgc's snapshot returns the OLD rig read-back
  // (rig_agc.or(agc)) until the next RX poll, so the clicked chip wouldn't light up. Sync from
  // the snapshot when it changes (confirms / corrects), exactly like the NR slider above.
  const [agc, setAgcLocal] = useState<string | null>(() => snap.radio.agc ?? null)
  useEffect(() => {
    if (snap.radio.agc != null) setAgcLocal(snap.radio.agc)
  }, [snap.radio.agc])
  const changeAgc = (sp: 'fast' | 'mid' | 'slow') => {
    setAgcLocal(sp)
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
  const commandedMode = modeOverride ?? sidebandAuto
  const pickMode = (m: 'USB' | 'LSB' | 'FM' | null) =>
    void setSidebandOverride(m)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast('Could not set mode', 'error'))
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
      .catch(() => pushToast('Could not set filter width', 'error'))
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
      .catch(() => pushToast('Could not set split', 'error'))
  const toggleSplit = () =>
    splitOn
      ? setSplit(null)
          .then((s) => onSnap?.(s))
          .catch(() => pushToast('Could not clear split', 'error'))
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
      pushToast('TX locked — this frequency/mode is outside your license privileges', 'info', 3500)
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
    const band = bandLabelForMhz(mhz)
    if (!band) {
      pushToast(`${mhz.toFixed(4)} MHz is outside the band plan`, 'error', 3000)
      return
    }
    // In-band keeps the current sideband; crossing 10 MHz follows the band convention (#45).
    void setFrequency(mhz, band, sidebandForQsy(mhz, snap.radio.dialMhz, snap.radio.sideband))
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
      .catch(() => pushToast(`Could not ${recording ? 'stop' : 'start'} recording`, 'error'))
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
    seenDspLevels.current || snap.radio.nrLevel != null || snap.radio.agc != null
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
        labels: PHONE_PANEL_LABELS,
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
      <CockpitPaneFrame title="Band activity" paneId="bandActivity" fit="content">
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
    <CockpitPaneFrame title="Voice keyer" paneId="voiceKeyer" fit="content">
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
        <CockpitPaneFrame title="Rig scope controls" paneId="rigscope" fit="content">
          <div className="ph-rigscope" role="group" aria-label="Rig scope control">
            <span className="ph-rigscope-lbl" title="These command the radio's own scope, not just the on-screen zoom">
              Rig&nbsp;scope
            </span>
            <div className="ph-span">
              {RIG_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={`Set the radio's scope span to ${sp.label}`}
                  onClick={() => void setScopeSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title="Scope reference level — lower to lift weak signals out of the noise">
              <span>Ref</span>
              <input
                type="range"
                min={-200}
                max={200}
                step={5}
                value={scopeRefTenths}
                onChange={(e) => changeScopeRef(Number(e.target.value))}
                aria-label="Scope reference level (dB)"
              />
              <span className="ph-power-val">{(scopeRefTenths / 10).toFixed(1)} dB</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {/* FlexRadio SmartSDR panadapter controls — command the Flex pan's real bandwidth + ref. */}
      {hasRigScopePane && flexScope && (
        <CockpitPaneFrame title="Rig scope controls" paneId="rigscope" fit="content">
          <div className="ph-rigscope" role="group" aria-label="Flex panadapter control">
            <span className="ph-rigscope-lbl" title="These command the FlexRadio's real SmartSDR panadapter, not just the on-screen zoom">
              Flex&nbsp;pan
            </span>
            <div className="ph-span">
              {FLEX_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={`Set the Flex panadapter bandwidth to ${sp.label}`}
                  onClick={() => void setFlexPanSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title="Panadapter reference level (dBm) — lower to lift weak signals out of the noise">
              <span>Ref</span>
              <input
                type="range"
                min={-140}
                max={-20}
                step={5}
                value={flexRefDbm}
                onChange={(e) => changeFlexRef(Number(e.target.value))}
                aria-label="Flex panadapter reference level (dBm)"
              />
              <span className="ph-power-val">{flexRefDbm} dBm</span>
            </label>
          </div>
        </CockpitPaneFrame>
      )}

      {hasDspPane && (
        <CockpitPaneFrame title="DSP functions" paneId="dsp" fit="content">
          <div className="ph-dsp" role="group" aria-label="Rig DSP functions">
            <span className="ph-dsp-label">DSP</span>
            {dspFuncs.map((f) => {
              const on = snap.radio[f.key] === true
              return (
                <button
                  key={f.key}
                  type="button"
                  className={`ph-dsp-btn${on ? ' on' : ''}`}
                  aria-pressed={on}
                  title={f.title}
                  onClick={() =>
                    void setRigFunc(f.key, !on)
                      .then((s) => onSnap?.(s))
                      .catch(() => pushToast(`Could not toggle ${f.label}`, 'error'))
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
        <CockpitPaneFrame title="RX DSP levels" paneId="dspLevels" fit="content">
          <div className="ph-dsp-levels" role="group" aria-label="RX DSP levels">
            {snap.radio.nrLevel != null && (
              <label className="ph-dsplev" title="Noise-reduction depth — raise until the noise floor drops, back off if audio gets watery">
                <span>NR</span>
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
                  aria-label="Noise-reduction level"
                />
                <span className="ph-power-val">{nr}%</span>
              </label>
            )}
            {snap.radio.agc != null && (
              <div className="ph-agc" role="group" aria-label="AGC speed" title="AGC time constant">
                <span className="ph-dsplev-lbl">AGC</span>
                {(['fast', 'mid', 'slow'] as const).map((sp) => (
                  <button
                    key={sp}
                    type="button"
                    className={`theme-chip${agc === sp ? ' active' : ''}`}
                    aria-pressed={agc === sp}
                    onClick={() => changeAgc(sp)}
                  >
                    {sp === 'fast' ? 'Fast' : sp === 'mid' ? 'Mid' : 'Slow'}
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
    <CockpitPaneFrame title="Log" paneId="log">
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
          <div className="ph-mode-pick" role="group" aria-label="Phone mode">
            {(['AUTO', 'USB', 'LSB', 'FM'] as const).map((m) => {
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
                      ? `AUTO — sideband by band (now ${sidebandAuto}); a band change re-asserts this`
                      : `Force ${m} until you change bands`
                  }
                  onClick={() => pickMode(m === 'AUTO' ? null : m)}
                >
                  {m === 'AUTO' ? `AUTO·${sidebandAuto}` : m}
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
          label: 'Power',
          title: 'RF output power',
          onPointerDown: () => {
            dragging.current = true
          },
          onPointerUp: () => {
            dragging.current = false
          },
        }}
        txActiveLabel="▲ TX"
        onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
        onStopTx={() => void haltTx()}
      >
        {modeMismatch && (
          <span
            className="ph-mode-mismatch"
            title={`Your rig is on ${modeMismatch}, but Phone is set to ${commandedMode}. Logging and TX use ${commandedMode} — turn the rig's mode knob (or re-pick the band) to match.`}
          >
            rig: {modeMismatch}
          </span>
        )}
        {catOk && (
          <div className={`ph-split ${splitOn ? 'on' : ''}`}>
            <button
              className="ph-split-toggle"
              onClick={toggleSplit}
              title={
                splitOn
                  ? `Split ON — TX ${splitTxMhz?.toFixed(3)} MHz. Click for simplex.`
                  : 'Work split — TX off your RX frequency (e.g. up 5)'
              }
            >
              SPLIT
            </button>
            <button className="ph-split-step" onClick={() => bumpSplit(-1)} title="TX 1 kHz lower">
              −
            </button>
            <span className="ph-split-amt mono" title="TX offset from your RX dial (kHz)">
              {splitOffsetKhz >= 0 ? `+${splitOffsetKhz}` : `${splitOffsetKhz}`}
            </span>
            <button className="ph-split-step" onClick={() => bumpSplit(1)} title="TX 1 kHz higher">
              +
            </button>
          </div>
        )}
        {!catOk && (
          <span
            className="ph-nocat"
            title={
              snap.radio.catDetail ||
              'No CAT link — set a rigctld/CAT rig in Settings so the app can switch the mode and follow the dial. On VOX/RTS-DTR PTT the rig has no command channel.'
            }
          >
            ⚠ no rig control
          </span>
        )}
        {snap.radio.micGain != null && (
          <label className="ph-power" title="Microphone gain — raise it until SSB peaks tickle the ALC zone">
            <span>Mic</span>
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
              aria-label="Mic gain"
            />
            <span className="ph-power-val">{mic}%</span>
          </label>
        )}
        {catOk && commandedMode !== 'FM' && (
          <div className="ph-filter" title="RX filter / passband width (CAT)">
            <span className="ph-filter-lbl">BW</span>
            <button
              type="button"
              className="ph-filter-step"
              onClick={() => bumpFilter(-100)}
              title="Narrower (−100 Hz)"
            >
              −
            </button>
            <span className="ph-filter-val mono">
              {filterHz ? `${(filterHz / 1000).toFixed(1)}k` : '—'}
            </span>
            <button
              type="button"
              className="ph-filter-step"
              onClick={() => bumpFilter(100)}
              title="Wider (+100 Hz)"
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
          aria-label={recording ? 'Stop recording this QSO' : 'Record QSO audio'}
          title={
            recording
              ? 'Recording — click to stop recording this QSO'
              : 'Record the received audio to a WAV in the recordings folder'
          }
        >
          <span className="ph-rec-dot" aria-hidden="true">
            {recording ? '■' : '●'}
          </span>
          REC
        </button>
        {/* No visible 'RX': the meter is a role="meter" already named "RX audio level", and
            the label element keeps the same string as its tooltip. */}
        <label className="ph-rxmeter" title="RX audio level">
          <LiveLevelMeter label="RX audio level" variant="compact" />
        </label>
      </CockpitHeader>

      <section className="ph-scope-panel">
        <div className="ph-scope-head">
          {(() => {
            // Honest per feed: soundcard FFT = the demodulated RX audio; a native
            // panadapter = the real RF spectrum, so name it a panadapter and show its span.
            const rf = nativeRf ? scopeFeed : null
            return (
              <span
                className="ph-scope-title"
                title={
                  rf
                    ? 'Native RF panadapter — the real RF spectrum around your dial, not the demodulated audio passband.'
                    : 'Receiver AUDIO spectrum (200–2900 Hz of the demodulated passband) — not a band-wide RF panadapter, so a voice fills the passband rather than sliding across it as you tune.'
                }
              >
                {rf ? 'RF Panadapter' : 'Passband'}{' '}
                <span className="ph-scope-sub">
                  {rf
                    ? `· ${(rf.loHz / 1e6).toFixed(4)}–${(rf.hiHz / 1e6).toFixed(4)} MHz`
                    : '· RX audio'}
                </span>
              </span>
            )
          })()}
          {/* No 'Colors' label: PalettePicker is a <select> that already carries
              aria-label="Waterfall color palette (applies to all modes)" and the matching
              tooltip, so the word was the third statement of the same thing on one row. */}
          <PalettePicker />
        </div>
        <div className="ph-scope-wrap" ref={scopeRef} title="Scroll here to tune the VFO">
          {nativeRf ? (
            // Native RF panadapter: RF-width zoom around the dial (not audio-passband slices).
            <div className="ph-span" role="group" aria-label="Panadapter zoom">
              {RF_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className={`theme-chip${rfSpan.label === sp.label ? ' active' : ''}`}
                  aria-pressed={rfSpan.label === sp.label}
                  title={sp.title}
                  onClick={() => setRfSpan(sp)}
                >
                  {sp.label}
                </button>
              ))}
            </div>
          ) : (
            <div className="ph-span" role="group" aria-label="Bandscope span">
              {SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className={`theme-chip${span.label === sp.label ? ' active' : ''}`}
                  aria-pressed={span.label === sp.label}
                  title={sp.title}
                  onClick={() => setSpan(sp)}
                >
                  {sp.label}
                </button>
              ))}
            </div>
          )}
          <PhoneScope
            transmitting={snap.radio.transmitting}
            theme={theme}
            smeterDb={smeterDb}
            viewLoHz={nativeRf ? rfSpan.lo : span.lo}
            viewHiHz={nativeRf ? rfSpan.hi : span.hi}
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
        label="scope height"
      />

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
        <button
          type="button"
          className={`ph-ptt${keyed ? ' keyed' : ''}`}
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
            snap.radio.txAllowed
              ? "Hold to talk (or Space). Toggle 'Lock' for hands-free (then Enter keys/unkeys). You talk on the rig's mic."
              : 'TX locked — outside your license privileges (pick a band, or change your license in Settings)'
          }
        >
          {!snap.radio.txAllowed ? '🔒 TX LOCKED' : keyed ? 'ON AIR — release to stop' : 'PUSH TO TALK'}
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

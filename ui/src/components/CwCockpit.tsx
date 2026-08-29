// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): every
// operator-visible string in it is in the catalog under `cw.*`. Nothing was deferred —
// CW's stop line is Stop TX (→ stopCw + haltTx) and Tune, both drawn by CockpitHeader, plus
// Esc, a window keydown with no string at all. The F-key macros and the send bar are
// SENDERS and move normally; the keyer back-end, speed and pitch are CONFIGURATION on the
// transmit path and move exactly as PTT Method did. WPM, pitch and filter width in Hz, the
// scope spans and reference levels, the macro TEXTS with their {MYCALL}/{RST}/{NAME}/{EXCH}
// tokens and every <select> value are invariant tokens and stay in the code.
import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, FieldDayStatus, NeedTag, Settings, SpotRow } from '../types'
import { PhoneScope } from './PhoneScope'
import { useSmeterDb } from './LiveMeters'
import { TxMeters, TX_METERS_WHEN } from './TxMeters'
import { PalettePicker } from './PalettePicker'
import { BandPicker } from './BandPicker'
import { BandStrip } from './BandStrip'
import { TuningStrip } from './TuningStrip'
import { CockpitHeader } from './CockpitHeader'
import { ZeroBeat } from './ZeroBeat'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { MemoryStrip } from './MemoryStrip'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import type { Memory } from '../features/memories'
import { Splitter, SCOPE_SPLIT_MAX, SCOPE_SPLIT_MIN } from './Splitter'
import { PanelsMenu } from './PanelsMenu'
import {
  panelHost,
  NO_DSP_FUNCS_REASON,
  NO_DSP_LEVELS_REASON,
  NOTHING_SENT_REASON,
} from '../features/panelHost'
import { CW_PANEL_IDS, type CwPanelId, type PanelLayoutApi } from '../features/panelState'
import { LogEntry } from './LogEntry'
import { SpotDialog } from './SpotDialog'
import {
  getSettings,
  setSettings,
  sendCw,
  setCwKeyer,
  setCwWpm,
  stopCw,
  cwDecode,
  cwClear,
  setAiCw,
  selectPeer,
  previewCw,
  pointRotatorAtCall,
  setRigFunc,
  setFilterWidth,
  setNrLevel,
  setAgc,
  setScopeSpan,
  setScopeRef,
  setFlexPanSpan,
  setFlexPanRef,
  openPanelWindow,
  setTune,
  atuTune,
  setFrequency,
  haltTx,
  startQsoRecording,
  stopQsoRecording,
  getCatCwUnprovenRigModels,
} from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { SplitControl } from './SplitControl'
import { RotorStrip } from './RotorStrip'
import { useWheelTune } from '../useWheelTune'
import { useScopeTune } from '../useScopeTune'
import { useRegionCols } from '../useRegionCols'
import { usePinnedScroll } from '../usePinnedScroll'
import { cwScopeWindow, isRfScopeSource, sidebandSign, TRACE_HOLD_MS, NO_NATIVE_SCOPE_REASON } from '../waterfall'
import { t } from '../i18n'
import { T } from '../i18n/T'
import type { MessageKey } from '../i18n'

/** This cockpit's INVARIANT vocabulary — the words that are the mode's own technical tokens
 *  rather than prose, gathered here so the i18n guard reads them as the deliberate constants
 *  they are: the mode name in the badge, the units the readouts print, the rig's own control
 *  group names, the two back-end names that are a protocol and a product, and the split and
 *  record plates. */
const CW = 'CW'
const WPM = 'WPM'
const DB = 'dB'
const DBM = 'dBm'
const DSP = 'DSP'
const NR = 'NR'
const AGC = 'AGC'
const BW = 'BW'
const CAT = 'CAT'
const WINKEYER = 'WinKeyer'
const REC = 'REC'
/** The BW nudge and the AI decoder's audio window, as the tooltips print them — figures, so
 *  they are supplied to the message rather than written in it. */
const FILTER_STEP_HZ = 50
const AI_WINDOW_HZ = '400–1200'

/** The AGC chips, in the order `Engine::AGC_SPEEDS` lists them: AUTO left of the three time
 *  constants, OFF right of them — most-automatic through to no AGC at all. The `id` is the
 *  token that goes on the wire and the label is a KEY, not a word: `t()` runs when the row
 *  RENDERS, so a locale switch relabels the chips and the chip is still compared on its id
 *  (the same split RF_SPANS makes, and for the same reason). */
const AGC_CHIPS = [
  { id: 'auto', labelKey: 'cw.rxDsp.agc.auto' },
  { id: 'fast', labelKey: 'cw.rxDsp.agc.fast' },
  { id: 'mid', labelKey: 'cw.rxDsp.agc.mid' },
  { id: 'slow', labelKey: 'cw.rxDsp.agc.slow' },
  { id: 'off', labelKey: 'cw.rxDsp.agc.off' },
] as const satisfies readonly { id: string; labelKey: MessageKey }[]

/** Client-side RF-zoom presets for a native panadapter (mirror of the Phone cockpit).
 *  The ± labels are measurements and stay written here; `Full` is a word. Both it and every
 *  title resolve when the row RENDERS (a module constant would freeze the first locale
 *  loaded), so the chips are keyed and compared on `id` — the preset itself rather than the
 *  word printed on it. */
const RF_SPANS = [
  {
    id: 'full',
    lo: -1e9,
    hi: 1e9,
    label: () => t('cw.rfZoom.full.label'),
    title: () => t('cw.rfZoom.full.title'),
  },
  {
    id: '±25k',
    lo: -25_000,
    hi: 25_000,
    label: () => '±25k',
    title: () => t('cw.rfZoom.span.title', { khz: 25 }),
  },
  {
    id: '±10k',
    lo: -10_000,
    hi: 10_000,
    label: () => '±10k',
    title: () => t('cw.rfZoom.span.title', { khz: 10 }),
  },
  {
    id: '±5k',
    lo: -5_000,
    hi: 5_000,
    label: () => '±5k',
    title: () => t('cw.rfZoom.span.title', { khz: 5 }),
  },
] as const

/** RIG scope-span presets (native Icom CI-V) — command the RADIO's real panadapter sweep width
 *  via CI-V 27 15, exactly as the Phone cockpit does. */
const RIG_SPANS = [
  { label: '±2.5k', hz: 2_500 },
  { label: '±5k', hz: 5_000 },
  { label: '±10k', hz: 10_000 },
  { label: '±25k', hz: 25_000 },
  { label: '±50k', hz: 50_000 },
  { label: '±100k', hz: 100_000 },
  { label: '±250k', hz: 250_000 },
] as const

/** FlexRadio pan BANDWIDTH presets (full span) — command the SmartSDR panadapter width. */
const FLEX_SPANS = [
  { label: '50k', hz: 50_000 },
  { label: '100k', hz: 100_000 },
  { label: '200k', hz: 200_000 },
  { label: '500k', hz: 500_000 },
  { label: '1M', hz: 1_000_000 },
  { label: '2M', hz: 2_000_000 },
] as const

/** What each CW keyer back-end IS, and what it needs to work — verbatim from the four option
 *  buttons the header's keyer <select> replaced (2026-08-04 density pass). This is operating
 *  information, not chrome: the soundcard entry is the one that stops an operator keying a tone
 *  through SSB with nothing routed and the drive over ALC, so it is carried on the select AND
 *  on each option rather than dropped with the buttons. Resolved when the header renders. */
const keyerHelp = (): Record<'cat' | 'soundcard' | 'winkeyer' | 'serial', string> => ({
  cat: t('cw.keyer.cat.title'),
  serial: t('cw.keyer.serial.title'),
  winkeyer: t('cw.keyer.winkeyer.title'),
  soundcard: t('cw.keyer.soundcard.title'),
})

interface Props {
  snap: AppSnapshot
  theme: string
  /** CW sidetone pitch (Hz) — the scope's zero-beat marker. */
  pitchHz?: number
  /** Wheel-tune sensitivity (from Settings) — applied to the scope + readout wheel-tune. */
  wheelSensitivity?: number
  /** Click-to-work handoff from the Needed board: the callsign to prefill the log with.
   * `ts` changes on each click so re-working the same call refires the prefill. */
  pendingWork?: { call: string; ts: number } | null
  /** Called once the prefill has been applied, so the parent can clear it. */
  onConsumeWork?: () => void
  /** Apply a snapshot returned by a command (e.g. a band QSY) without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** Field Day status — when non-null the log strip switches to FD mode. */
  fieldDay?: FieldDayStatus | null
  /** Live cluster spots (all bands/modes); the band-strip filters to CW on the current band. */
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
  /** Panel visibility/resize record — host-owned (App) so it survives this view's remounts.
   *  Optional: without it every pane shows and there's no ⊞ menu. */
  panels?: PanelLayoutApi<CwPanelId>
}

/** Display labels for the CW removable panels (the ⊞ Panels menu). Resolved when the menu is
 *  BUILT — a module constant would freeze the first locale loaded. */
const cwPanelLabels = (): Record<CwPanelId, string> => ({
  // The strip itself — the CW-narrow audio view (or the rig's RF panadapter when one
  // streams). `scopeCtl` right below commands the RADIO's scope and is a separate pane in
  // the region; the two entries sit adjacent in the menu, so the labels have to distinguish
  // the display from the controls for it.
  scope: t('cw.panel.scope'),
  scopeCtl: t('cw.panel.scopeCtl'),
  dsp: t('cw.panel.dsp'),
  txmeters: t('cw.panel.txmeters'),
  rxdsp: t('cw.panel.rxdsp'),
  bandActivity: t('cw.panel.bandActivity'),
  copilot: t('cw.panel.copilot'),
  decode: t('cw.panel.decode'),
  sent: t('cw.panel.sent'),
})

/** One F-key macro row as the dock renders it.
 *
 * A macro from an operator's own named profile carries `label` — his words, never
 * translated. The BUILT-IN sets below carry `label` where the caption is on-air shorthand
 * (CQ, 73, AGN, TU, CQ FD, ?) and `labelKey` where it is a word, resolved when the row
 * renders. ⚠️ Both built-in sets are PARSED OUT OF THIS SOURCE by `docs-match-code.test.ts`
 * and compared with the tables docs/manual/CW.md publishes, so a caption is written here as
 * a plain literal or named by a key — never assembled. */
type CwMacro = { key: string; label?: string; labelKey?: MessageKey; text: string }

/** Default CASUAL/ragchew macro set (no contest serial/exchange). Standard CW QSO flow:
 * F1 CQ → F2 Call (answer a CQ with just your call, so they copy it — no report yet) →
 * F3 Reply (send your report + name, once they've come back to you) → F4 73. Overs end
 * `KN` ("go ahead, you only"). The engine expands the tokens ({MYCALL}/{NAME}/{RST}/! =
 * worked call) with the live QSO context, so we just send the template — every character
 * of `text` goes on the air and is invariant. */
const DEFAULT_MACROS: CwMacro[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.call.label', text: '! DE {MYCALL} {MYCALL} K' },
  {
    key: 'F3',
    labelKey: 'cw.macro.reply.label',
    text: '! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN',
  },
  { key: 'F4', label: '73', text: '! DE {MYCALL} TU 73 SK' },
  { key: 'F5', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F6', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: '?', text: '? ' },
]

/** Default Field Day CW macro set — replaces the casual defaults while FD mode is on.
 * The engine fills {EXCH} = "{CLASS} {SECTION}" (e.g. "3A WI") from the FD settings, so
 * one template serves both events. Contest cadence: F1 CQ FD → F2 answer with your call →
 * F3 send the exchange (twice, for copy) → F4 confirm + TU. */
const DEFAULT_FD_MACROS: CwMacro[] = [
  { key: 'F1', label: 'CQ FD', text: 'CQ FD DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.call.label', text: '! DE {MYCALL} K' },
  { key: 'F3', labelKey: 'cw.macro.exch.label', text: '! DE {MYCALL} {EXCH} {EXCH} K' },
  { key: 'F4', label: 'TU', text: '! TU {EXCH} DE {MYCALL} K' },
  { key: 'F5', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F6', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: '?', text: '? ' },
]

const WPM_MIN = 5
const WPM_MAX = 50

/**
 * CW operating cockpit — casual/ragchew. Keyboard + F-key macros key the rig via the
 * CAT keyer (the engine's send_cw path); the waterfall is the CW spectrum; a compact
 * strip logs the QSO into the multi-mode logbook (RST 599). Entering the section forces
 * the rig to CW (the rig-mode policy, wired in App). No contest scoring — by design.
 */
/** DSP funcs relevant to CW — NB (impulse noise), NR (broadband hiss), Notch/ANF (carriers).
 * COMP/VOX are voice-only, so they're deliberately absent here. Capability-gated like Phone. */
const CW_DSP_FUNCS = [
  { key: 'nb', label: 'NB', titleKey: 'cw.dsp.nb.title' },
  { key: 'nr', label: 'NR', titleKey: 'cw.dsp.nr.title' },
  { key: 'notch', label: 'Notch', titleKey: 'cw.dsp.notch.title' },
] as const

export function CwCockpit({
  snap,
  theme,
  pitchHz = 600,
  wheelSensitivity,
  pendingWork,
  onConsumeWork,
  onSnap,
  fieldDay,
  spots,
  needByCall,
  typeByCall,
  onWorkSpot,
  onRecallMemory,
  onOpenMemories,
  onOpenSettings,
  panels,
}: Props) {
  // Live S-meter (shared 100 ms poll, lock-free backend) — used to arrive via the 300 ms
  // snapshot on top of the backend's own sampling, which read as a laggy needle. smeterDb-only
  // subscription: the cockpit re-renders when the S-meter changes, never on RX-level churn.
  const smeterDb = useSmeterDb()
  const catOk = snap.radio.catOk === true
  // Wheel-to-tune over the CW scope, sharing the tuning strip's step selector.
  // Tuning step, persisted per cockpit ('nexus.cw.tuneStep'): the cockpit unmounts on every
  // mode switch, so plain state reset the step to 100 Hz on each round-trip (FTDX10
  // report 2026-07-21 — the third remount-state-loss bug today). localStorage so a
  // CW operator's 10 Hz survives restarts too, like nexus.cw.sensitivity.
  const [tuneStep, setTuneStep] = useState(() => {
    try {
      const v = Number(localStorage.getItem('nexus.cw.tuneStep'))
      return Number.isFinite(v) && v > 0 ? v : 100
    } catch {
      return 100
    }
  })

  // QSO recording. Phone has had this since the audio bridge landed; CW never got one, so the
  // only way to record a CW contact was to switch cockpits. Same session-level toggle driven by
  // the snapshot, so the TopBar REC badge and its stop work here identically.
  const [recBusy, setRecBusy] = useState(false)
  const recording = snap.radio.qsoRecording
  const toggleRecord = () => {
    if (recBusy) return
    setRecBusy(true)
    const fn = recording ? stopQsoRecording : startQsoRecording
    fn()
      .then((snapshot) => onSnap?.(snapshot))
      // Two whole sentences, not a stem with the verb spliced in: word order is not
      // universal and a sentence assembled from fragments cannot be translated.
      .catch(() =>
        pushToast(recording ? t('cw.record.stopFailed') : t('cw.record.startFailed'), 'error'),
      )
      .finally(() => setRecBusy(false))
  }
  useEffect(() => {
    try {
      localStorage.setItem('nexus.cw.tuneStep', String(tuneStep))
    } catch {
      /* ignore */
    }
  }, [tuneStep])
  // Spot-to-cluster popup, seeded from the LogEntry call field (operator ask: CW too).
  const [spotOpen, setSpotOpen] = useState(false)
  const [spotCall, setSpotCall] = useState('')
  const scopeRef = useRef<HTMLElement>(null)
  useWheelTune(scopeRef, {
    dialMhz: snap.radio.dialMhz,
    sideband: snap.radio.sideband || 'USB',
    enabled: catOk && !snap.radio.txBusyReason && !snap.radio.transmitting,
    stepHz: tuneStep,
    sensitivity: wheelSensitivity,
    onSnap,
  })
  // Click/drag tuning from the scope (Flex-style): a click zero-beats the clicked CW
  // signal to the pitch; a press-drag slides the passband box, coalesced ~120 ms.
  // The CAT write keeps the raw sideband (what wheel-tune sends — the CW rig-mode
  // policy is applied separately by the engine).
  const onScopeTune = useScopeTune({
    sideband: snap.radio.sideband || 'USB',
    enabled: catOk && !snap.radio.txBusyReason && !snap.radio.transmitting,
    onSnap,
  })
  // The scope's click/box math needs a CW-CLASSIFIED mode string (settings.sideband is
  // USB/LSB here — the soundcard keyer keys through SSB): same sideband SIGN, but the
  // click zero-beats instead of carrier-snapping, and the box centers on the dial.
  const scopeMode = sidebandSign(snap.radio.sideband || 'USB') < 0 ? 'CW-L' : 'CW'
  // RX filter width (CW wants a NARROW filter — default 500 Hz, 50-Hz steps, 50–2000 Hz span).
  const filterHz = snap.radio.filterWidthHz ?? null
  const bumpFilter = (deltaHz: number) => {
    const base = filterHz ?? 500
    const next = Math.min(2000, Math.max(50, base + deltaHz))
    // Never let the clamp invert the direction — "wider" must not narrow (e.g. a stale Phone
    // width above CW's 2 kHz cap right after switching modes, before the next `m` re-read).
    if ((deltaHz > 0 && next <= base) || (deltaHz < 0 && next >= base)) return
    void setFilterWidth(next)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('cw.filter.failed'), 'error'))
  }
  // Source of truth = the engine's actual keyer speed (survives navigation; the
  // old hard-coded 25 silently re-keyed at the wrong speed after a nav round-trip).
  const [wpm, setWpm] = useState(() => snap.radio.cwWpm ?? 25)
  useEffect(() => {
    if (snap.radio.cwWpm != null) setWpm(snap.radio.cwWpm)
  }, [snap.radio.cwWpm])
  // --- CI-V RX DSP + panadapter controls, at PARITY with the Phone cockpit (a CW op wants
  //     AGC speed, NR depth, and the rig's real panadapter just as much). Each control is
  //     capability-gated on what the rig actually reports, so nothing shows on a rig that lacks it.
  const [nr, setNr] = useState(30) // % noise-reduction depth — pushed once touched
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
  // AGC speed — the chip lights on the click (snap.radio.agc is the rig READ-BACK and lags a
  // poll behind), then the rig gets the last word: DERIVED, so there is no mirror to go stale.
  // Showing the pick past a REFUSAL is the case that matters — Hamlib's AGC is an enum and a
  // rig without that step answers RPRT -1, so the read-back never becomes the pick and a
  // remembered mirror would light Mid forever on a radio that is still on Fast.
  const [agcPick, setAgcPick] = useState<string | null>(null)
  const agc =
    agcPick != null && agcPick !== snap.radio.refusedAgc ? agcPick : (snap.radio.agc ?? null)
  const changeAgc = (sp: 'auto' | 'fast' | 'mid' | 'slow' | 'off') => {
    setAgcPick(sp)
    void setAgc(sp)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  // Native scope feed (reported by PhoneScope) → drives the RF-panadapter switch, exactly like
  // Phone. When a Flex/Icom CI-V scope is streaming we show the real RF spectrum (with client
  // RF-zoom + the rig scope controls); otherwise the CW-narrow audio zero-beat view stays.
  const [scopeFeed, setScopeFeed] = useState<{ source: string; loHz: number; hiHz: number } | null>(
    null,
  )
  const nativeRf = scopeFeed != null && isRfScopeSource(scopeFeed.source)
  const civScope = scopeFeed?.source === 'civ'
  const flexScope = scopeFeed?.source === 'flex'
  // The sub-plate under the scope title: the fed span in MHz — a MEASUREMENT, so it is
  // assembled here rather than in the catalog — or the word for what the audio view centres
  // on. The `·` separator is chrome and belongs to neither.
  const scopeSub =
    nativeRf && scopeFeed
      ? `· ${(scopeFeed.loHz / 1e6).toFixed(4)}–${(scopeFeed.hiHz / 1e6).toFixed(4)} MHz`
      : `· ${t('cw.scope.audio.sub')}`
  const [flexRefDbm, setFlexRefDbm] = useState(-80)
  const changeFlexRef = (dbm: number) => {
    setFlexRefDbm(dbm)
    void setFlexPanRef(dbm)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  const [rfSpan, setRfSpan] = useState<(typeof RF_SPANS)[number]>(RF_SPANS[0])
  const [scopeRefTenths, setScopeRefTenths] = useState(0)
  const changeScopeRef = (tenths: number) => {
    setScopeRefTenths(tenths)
    void setScopeRef(tenths)
  }
  // Live single-signal CW decode of the receive audio at the marker pitch — poll the
  // engine ~1.4 Hz (the decode reads a multi-second ring, so faster adds no detail).
  const [decoded, setDecoded] = useState<{ text: string; wpm: number }>({ text: '', wpm: 0 })
  // Decode transcript: bottom-pinned via the shared discipline. The old
  // unconditional `scrollTop = scrollHeight` on every reveal tick (the 50 ms
  // typewriter) made scroll-back physically impossible while copy was flowing —
  // exactly when the operator wants to re-read a callsign that just scrolled
  // off. Pinned follows the copy; scrolled-up reading is never yanked.
  const decodePin = usePinnedScroll<HTMLDivElement>()
  // Cockpit root: the scope-height splitter measures + writes its CSS var here.
  const cockpitRef = useRef<HTMLElement>(null)
  // Decode sensitivity for the internal pitch decoder (now WPM-estimation + AI-off
  // fallback only — the slider left with the classic pane; the stored value still applies).
  const sensitivityRef = useRef<number>(
    (() => {
      const v = parseFloat(localStorage.getItem('nexus.cw.sensitivity') ?? '')
      return Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0.5
    })(),
  )
  // Typewriter reveal: the AI decoder emits each inference pass's text as one batch
  // (window-batch inference — several characters land at once). Reveal appended text
  // character-by-character so the copy FLOWS like a live operator's; the drain rate
  // scales with backlog so a big batch clears in ~2 s and never falls behind. A
  // non-append change (transcript reset/trim) snaps to the full text instantly.
  const [revealLen, setRevealLen] = useState(0)
  const prevTextRef = useRef('')
  useEffect(() => {
    const text = decoded.text
    if (!text.startsWith(prevTextRef.current)) setRevealLen(text.length)
    prevTextRef.current = text
  }, [decoded.text])
  useEffect(() => {
    const backlog = decoded.text.length - revealLen
    if (backlog <= 0) return
    const id = window.setInterval(() => {
      setRevealLen((n) => Math.min(decoded.text.length, n + Math.max(1, Math.ceil((decoded.text.length - n) / 40))))
    }, 50)
    return () => window.clearInterval(id)
  }, [decoded.text, revealLen])
  const revealedText = decoded.text.slice(0, revealLen)
  // TX echo — what we've actually transmitted (macros expanded), polled alongside the decode.
  // Same pin discipline: during an F-key macro run the operator must be able to
  // scroll back and verify what was actually keyed (the whole point of the echo).
  const [sent, setSent] = useState<string[]>([])
  const sentPin = usePinnedScroll<HTMLDivElement>()
  // Panels (Phase 3): scope + keyer/macros/send/log stay pinned; the panes under the scope are
  // removable, and the four content panes (Band Activity / Copilot / Decode / Sent) seam-resize.
  //
  // Whether each conditional pane CAN render at all right now, independent of the ⊞ tick —
  // computed HERE, above the menu, so the entry's note and the pane read ONE boolean each
  // and cannot drift apart. Five CW entries can be ticked with nothing on screen behind
  // them, and each carries a NOTE saying why; the four station-conditional ones clear by
  // themselves the moment the station can feed them:
  //   - Scope Controls command the RADIO's panadapter, so on the audio bandscope that pane
  //     can never mount however its box is ticked (Phone's twin of this entry is what the
  //     operator caught on 2026-08-03).
  //   - DSP Toggles / RX DSP Levels are capability-gated on what the rig reports over CAT,
  //     the same rule the DSP row applies to its own buttons.
  //   - Sent Echo holds this session's transmissions, so at EVERY session start it is empty
  //     and there is nothing to see — the operator's exact complaint, in another cockpit.
  //     Its box still answers to him THERE, which is when he is most likely to use it: an
  //     operator who does not want the echo should not have to transmit first to hide it.
  //   - TX meters here are the unpinned variant — nothing at all on receive — so that entry
  //     says when it has something to show rather than looking dead.
  const cwDspFuncs = CW_DSP_FUNCS.filter((f) => snap.radio[f.key] != null)
  const canRxDsp = snap.radio.nrLevel != null || snap.radio.agc != null
  const host = panels
    ? panelHost(panels, {
        menu: CW_PANEL_IDS,
        side: [],
        main: 'decode',
        labels: cwPanelLabels(),
        notes: {
          scopeCtl: civScope || flexScope ? undefined : NO_NATIVE_SCOPE_REASON,
          dsp: cwDspFuncs.length > 0 ? undefined : NO_DSP_FUNCS_REASON,
          rxdsp: canRxDsp ? undefined : NO_DSP_LEVELS_REASON,
          sent: sent.length > 0 ? undefined : NOTHING_SENT_REASON,
          txmeters: TX_METERS_WHEN,
        },
      })
    : null
  const shown = (id: CwPanelId) => (host ? host.shown(id) : true)
  // A CW-keyer failure surfaced by the radio loop (e.g. the rig rejected CAT send_morse).
  const [keyerError, setKeyerError] = useState<string | null>(null)
  // --- CW copilot: decoded-call chips only (Expert). The Guided/Expert selector + its bar were
  //     removed to reclaim vertical space for the decode transcript (operator ask). ---
  const [cand, setCand] = useState<{ call: string; best: boolean }[]>([])
  const [guide, setGuide] = useState<{
    state: string
    headline: string
    prompt: string
    recommended: string | null
    workedCall: string | null
    rst: string | null
    name: string | null
  }>({
    state: 'listening',
    headline: '',
    prompt: '',
    recommended: null,
    workedCall: null,
    rst: null,
    name: null,
  })
  // True once the operator sets WPM by hand → stop auto-matching to the decoded speed.
  const wpmTouched = useRef(false)
  // F-key macros come from the ACTIVE named CW profile (Settings ▸ CW). A rotating
  // operator can switch profiles right here in the cockpit bar; an empty profile falls
  // back to the built-in defaults — which swap to the Field Day set (with the {EXCH}
  // exchange tokens) while FD mode is on. Keep the full settings so the switcher can
  // persist the new active-profile index without dropping other fields.
  const [cwSettings, setCwSettings] = useState<Settings | null>(null)
  // Models whose CAT CW keyer is UNPROVEN and cannot report its own failure. Fetched from the
  // backend, which owns the rule (`rigmodels::cat_cw_unproven_rig_models`) — the SAME list
  // Settings ▸ CW reads, never a second copy here: membership changes as backends are fixed
  // upstream, and two sources of truth is how the two surfaces come to disagree about a radio.
  // Empty = rule unread (built without the `radio` feature, or the command failed), and no
  // caution is shown — an unreadable rule must not warn an operator off a keyer that works.
  const [catCwUnproven, setCatCwUnproven] = useState<number[]>([])
  const [profiles, setProfiles] = useState<{ name: string; macros: { key: string; label: string; text: string }[] }[]>(
    [],
  )
  const [activeProfile, setActiveProfile] = useState(0)
  useEffect(() => {
    let alive = true
    void getSettings()
      .then((s) => {
        if (!alive) return
        setCwSettings(s)
        setProfiles(s.macros?.cwProfiles ?? [])
        setActiveProfile(s.macros?.activeCwProfile ?? 0)
      })
      .catch(() => {})
    void getCatCwUnprovenRigModels()
      .then((m) => {
        if (alive && Array.isArray(m)) setCatCwUnproven(m)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])
  const profileMacros = profiles[activeProfile]?.macros
  // Typed as CwMacro[] so the row renderer reads ONE shape: a profile macro's label is the
  // operator's own words, a built-in's is either on-air shorthand or a catalog key.
  const macros: CwMacro[] =
    profileMacros && profileMacros.length ? profileMacros : fieldDay ? DEFAULT_FD_MACROS : DEFAULT_MACROS
  // Switch the active macro profile from the cockpit (optimistic) and persist it.
  const switchProfile = (i: number) => {
    setActiveProfile(i)
    if (!cwSettings) return
    const next = { ...cwSettings, macros: { ...cwSettings.macros, activeCwProfile: i } }
    setCwSettings(next)
    void setSettings(next)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('cw.macroProfile.failed'), 'error'))
  }
  const macrosRef = useRef(macros)
  macrosRef.current = macros
  // Reply preview: the exact text each F-key WILL send (macros expanded with the worked
  // call). Refetched from the backend when the worked station changes — cheap, once per QSO.
  const [previews, setPreviews] = useState<Record<string, string>>({})
  useEffect(() => {
    let alive = true
    Promise.all(
      macros.map((m) =>
        previewCw(m.text)
          .then((p) => [m.key, p] as const)
          .catch(() => [m.key, m.text] as const),
      ),
    ).then((entries) => {
      if (alive) setPreviews(Object.fromEntries(entries))
    })
    return () => {
      alive = false
    }
  }, [guide.workedCall, macros])
  useEffect(() => {
    let alive = true
    const tick = () => {
      cwDecode(sensitivityRef.current)
        .then((d) => {
          if (alive) {
            setDecoded({ text: d.text, wpm: d.wpm })
            setSent(d.sent)
            setKeyerError(d.keyerError)
            setCand(d.candidates)
            setGuide({
              state: d.state,
              headline: d.headline,
              prompt: d.prompt,
              recommended: d.recommended,
              workedCall: d.workedCall,
              rst: d.rst,
              name: d.name,
            })
          }
        })
        .catch(() => {})
    }
    tick()
    // Poll the decoded transcript often — the Rust streaming decoder updates every ~20 ms, so a
    // slow poll is pure display lag (the "desktop lags the web decoder" report). 200 ms ≈ 5 Hz
    // keeps copy near real-time without hammering the command channel.
    const id = window.setInterval(tick, 200)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [])
  // Initialize the keyer toggle from the engine's ACTUAL setting (the snapshot is the source
  // of truth) — not a hard-coded 'cat'. A stale local default showed CAT while the backend was
  // on Soundcard, so CW silently went to USB (Soundcard keying = rig in SSB) with no clue why.
  const [keyer, setKeyer] = useState<'cat' | 'soundcard' | 'winkeyer' | 'serial'>(
    () => (snap.radio.cwKeyer as 'cat' | 'soundcard' | 'winkeyer' | 'serial') || 'cat',
  )
  // Keep it in sync if the backend value changes (or arrives after first render).
  useEffect(() => {
    if (snap.radio.cwKeyer) setKeyer(snap.radio.cwKeyer as 'cat' | 'soundcard' | 'winkeyer' | 'serial')
  }, [snap.radio.cwKeyer])
  // The four back-end descriptions, read once per render — the select wears the SELECTED
  // one's and each <option> wears its own.
  const keyerHelpText = keyerHelp()
  // CAT KEYING IS UNPROVEN ON THIS RADIO (field report 2026-08-28, Yaesu FTX-1: "Try send a cw,
  // never went to tx"). The rig's Hamlib backend reports success whether or not it keyed, so the
  // keyer-error banner below can never light for this fault — the operator is told UP FRONT
  // instead. Read off the LIVE `keyer` state, not the saved setting, because this header switch
  // is where a backend gets changed without Settings ever being opened.
  const catCwUnprovenHere =
    keyer === 'cat' && !!cwSettings && catCwUnproven.includes(cwSettings.rigModel)
  const [text, setText] = useState('')
  // Sidetone pitch — local for instant marker response; persisted via set_cw_keyer.
  const [pitch, setPitch] = useState(pitchHz)
  useEffect(() => setPitch(pitchHz), [pitchHz])
  // The audio scope window follows the pitch, so the signal being zero-beat is mid-screen
  // whatever pitch the operator runs (it used to be a fixed 300–1100, centered on 600 only).
  const cwView = cwScopeWindow(pitch, filterHz)
  const changePitch = (v: number) => {
    const p = Math.max(300, Math.min(1200, Math.round(v)))
    setPitch(p)
    void setCwKeyer(keyer, p).then((s) => s && onSnap?.(s))
  }

  // Debounced persist. The live change always goes out immediately; the SAVE lands once the
  // operator stops moving (slider drag and PgUp/PgDn autorepeat both fire continuously, and
  // each save fsyncs the whole settings file). `persist: false` is the decoder's automatic
  // speed-match — it should track the station being worked WITHOUT overwriting the operator's
  // own stored speed. SF ticket #2.
  const wpmCommit = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const changeWpm = (w: number, persist = true) => {
    const v = Math.max(WPM_MIN, Math.min(WPM_MAX, Math.round(w)))
    setWpm(v)
    void setCwWpm(v, false)
    if (!persist) return
    clearTimeout(wpmCommit.current)
    wpmCommit.current = setTimeout(() => void setCwWpm(v, true), 600)
  }
  // Live snapshot ref so the keyboard handler (bound once, `[]` deps) reads the CURRENT
  // TX-allowed privilege state through send() — not whatever existed at mount.
  const snapRef = useRef(snap)
  snapRef.current = snap
  // `line`, not `t` — the catalog lookup is `t()` in every migrated file, so a parameter by
  // that name would shadow it here and nowhere else.
  const send = (line: string) => {
    if (!line.trim()) return
    // The engine blocks keying outside privileges anyway; surface why up front.
    if (!snapRef.current.radio.txAllowed) {
      pushToast(t('cw.send.txLocked'), 'info', 3500)
      return
    }
    void withErrorToast(() => sendCw(line), t('cw.send.failed'))
  }
  const sendTyped = () => {
    send(text)
    setText('')
  }
  const abort = () => {
    // Stop the CW keyer AND drop any tune carrier / stray PTT — a true stop-everything (Esc).
    void stopCw()
    void haltTx()
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
  const changeKeyer = (k: 'cat' | 'soundcard' | 'winkeyer' | 'serial') => {
    setKeyer(k)
    void setCwKeyer(k).then((s) => s && onSnap?.(s))
  }
  // Confirm a decoded station as the one we're working: make it the macro/log peer (so the
  // `!` token + logging use it), match our speed to theirs (unless WPM was set by hand), and
  // prefill the log with the read call/RST/name. Never transmits — the operator still keys.
  const workCall = (call: string) => {
    void selectPeer(call)
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
    if (!wpmTouched.current && decoded.wpm >= WPM_MIN) changeWpm(decoded.wpm, false)
    // The log fills continuously from the confirmed worked station (see cwLive on LogEntry) —
    // call now, then RST + name as they're decoded through the QSO.
  }

  // A click-to-work / spot-click handoff (pendingWork) must ARM the macro peer, or the `!` macros
  // would key the previously-selected chip's call on the new station's frequency. Plain selectPeer
  // (NOT workCall) — don't speed-match to the old decoded WPM (a different signal). The log prefill
  // is handled separately by LogEntry.
  useEffect(() => {
    if (pendingWork?.call) {
      void selectPeer(pendingWork.call)
        .then((s) => s && onSnap?.(s))
        .catch(() => {})
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingWork?.ts])

  // Keyboard: F1–F8 fire macros; Esc aborts; PgUp/PgDn nudge speed (±2, Shift ±4).
  // Live ref so the document listener (bound once) always reads current state.
  const stateRef = useRef({ wpm, text })
  stateRef.current = { wpm, text }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const macro = macrosRef.current.find((m) => m.key === e.key)
      if (macro) {
        e.preventDefault()
        send(macro.text)
      } else if (e.key === 'Escape') {
        e.preventDefault()
        abort()
      } else if (e.key === 'PageUp') {
        e.preventDefault()
        wpmTouched.current = true
        changeWpm(stateRef.current.wpm + (e.shiftKey ? 4 : 2))
      } else if (e.key === 'PageDown') {
        e.preventDefault()
        wpmTouched.current = true
        changeWpm(stateRef.current.wpm - (e.shiftKey ? 4 : 2))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // ── THE PANE REGION (2026-07-30 layout assessment, design3 §3) ───────────────────────
  // Every operator-content block under the scope renders through a CockpitPaneFrame in ONE
  // .cockpit-panes grid; the ⊞ Panels 'removed' gating is unchanged (shown()). TX chrome
  // never enters the region — the F-key macros and the type-ahead send bar live in the
  // pinned .cockpit-txdock, and Tune and Stop TX up in the header.
  //
  // ONLY THE HEADER PAIR IS THE STOP LINE. Stop TX (→ stopCw + haltTx) and Tune render
  // OUTSIDE every ⊞-removable pane and have no id, so no tick can take CW's way to stop off
  // the screen; that is the rule (features/panelState.ts), and Esc reaches the same abort.
  // The macros and the send bar have no id EITHER, and that is this cockpit's own choice
  // rather than a safety constraint — they are SENDERS, and the rule is indifferent to
  // senders (Operate's Tx messages is a hideable one). Said the same way at the dock below.
  //
  // The TX meters ALSO render in the dock and DO have a ⊞ id (`txmeters` is in CW_PANEL_IDS):
  // a readout, not a control, and nothing about the rule turns on where a readout sits.
  //
  // Which panes CAN render right now — the exact conditions that gated them in the old
  // `.cw-lower` JSX, hoisted because the region's column budget (maxCols) depends on them.
  // The four station-conditional ones read the SAME booleans the ⊞ menu was built from
  // (up at the panelHost spec), so the menu cannot claim one thing and the region another.
  const hasDecodePane = shown('decode')
  const hasSentPane = shown('sent') && sent.length > 0
  const hasScopeCtlPane = shown('scopeCtl') && (civScope || flexScope)
  const hasDspPane = shown('dsp') && cwDspFuncs.length > 0
  const hasRxDspPane = shown('rxdsp') && canRxDsp
  const hasBandPane = shown('bandActivity') && onWorkSpot != null
  const hasCopilotPane = shown('copilot')
  // The three rig-control groups share ONE frame (see rigCtlPane), so the frame renders when
  // ANY of them can — each group is still gated on its own ⊞ id inside it.
  const hasRigCtlPane = hasScopeCtlPane || hasDspPane || hasRxDspPane
  const mainPresent = hasDecodePane || hasSentPane
  const auxPresent = hasRigCtlPane || hasBandPane || hasCopilotPane
  // Three columns are decode+sent | aux | log, so the tier is only offered when BOTH the
  // leading (transcript) column and the aux column have something to hold — otherwise a
  // track would sit empty, the operator's "band of empty black" rebuilt. Same collapse
  // panelHost ships as dataCols 'one'|'two' for Operate; it feeds maxCols and is NEVER
  // stamped on the region (useRegionCols owns data-cols='1|2|3').
  //
  // And all the way to ONE when nothing but the log can render (every other CW pane is
  // ⊞-removable): a 2-col template with an empty leading minmax(0,1fr) track is that
  // same band of black at full height. At tier 1 rows are `auto`, so an empty column
  // simply takes no space (fix-round, 2026-07-31).
  const { ref: panesRef, cols } = useRegionCols<HTMLDivElement>(
    mainPresent && auxPresent ? 3 : mainPresent || auxPresent ? 2 : 1,
  )

  // DECODE's own head row is DELETED, and its controls render in the frame head's action
  // cluster — which was rendering EMPTY on every CW pane (2026-08-04 density pass). The row's
  // first span read "DECODE" two lines under a frame head reading "Decode"; that is the only
  // thing actually gone. Everything else — the AI badge, the WPM readout, the AI switch, its
  // status and Clear — is here, in the same left-to-right order, so the toggle still renders
  // BEFORE the optional status text (the status coming and going must not shift the switch
  // under the operator's pointer).
  //
  // WHY THE HEAD AND NOT SOMEWHERE SMALLER: DECODE is the column's only grower and it sits at
  // its fill floor at the default window, so a row deleted from its BODY is a row the
  // transcript gets. The frame head grows by the switch's 26px − the title's line box, and the
  // body loses the whole 26px row plus the panel's gap: net transcript gain, computed in
  // CwCockpit.density.test.tsx rather than asserted here.
  const decodeActions = (
    <>
      <span className="cw-ai-beta">{t('cw.decode.ai.badge')}</span>
      {decoded.wpm > 0 && (
        <span className="cw-decode-wpm">
          {decoded.wpm} {WPM}
        </span>
      )}
      <button
        type="button"
        role="switch"
        aria-checked={snap.aiCw?.enabled ?? false}
        className={`toggle${snap.aiCw?.enabled ? ' on' : ''}`}
        onClick={() => void setAiCw(!(snap.aiCw?.enabled ?? false))}
        title={snap.aiCw?.enabled ? t('cw.decode.ai.on.title') : t('cw.decode.ai.off.title')}
      >
        <span className="toggle-knob" />
      </button>
      {snap.aiCw?.enabled && snap.aiCw.status && (
        <span className="cw-ai-status">{snap.aiCw.status}</span>
      )}
      <button
        className="cw-decode-clear"
        onClick={() => {
          void cwClear()
          setDecoded({ text: '', wpm: 0 })
          setSent([])
          // A wipe re-pins both panes (same as Operate's Erase): an emptied
          // transcript must follow the next copy even if the operator had
          // scrolled up before clearing.
          decodePin.repin()
          sentPin.repin()
        }}
        title={t('cw.decode.clear.title')}
      >
        {t('cw.decode.clear.label')}
      </button>
    </>
  )

  const decodePane = hasDecodePane ? (
    <CockpitPaneFrame title={t('cw.pane.decode.title')} paneId="decode" weight={3} actions={decodeActions}>
      <div className="cw-decode panel" title={t('cw.decode.title', { window: AI_WINDOW_HZ })}>
        {/* The visible transcript animates character-by-character (typewriter) —
            aria-hidden so a screen reader doesn't announce every keystroke; the
            hidden role=log mirror below receives whole batches instead (a log
            region announces only ADDITIONS). */}
        <div className="cw-decode-text" ref={decodePin.ref} onScroll={decodePin.onScroll} aria-hidden="true">
          {revealedText ? (
            revealedText
          ) : (
            <span className="cw-decode-idle">
              {(snap.aiCw?.enabled && snap.aiCw.status) || t('cw.decode.listening')}
            </span>
          )}
        </div>
        <div className="sr-only" role="log" aria-label={t('cw.decode.log.aria')}>
          {decoded.text}
        </div>
      </div>
    </CockpitPaneFrame>
  ) : null

  const sentPane = hasSentPane ? (
    <CockpitPaneFrame title={t('cw.pane.sent.title')} paneId="sent" fit="content">
      {/* No head row: it held ONE span reading "SENT ▲" under a frame head reading "Sent", and
          what makes the echo readable as YOUR transmissions at a glance is the accent stripe
          (`.pane-body > .cw-sent-panel`), which stays. Deleting it also lets the framed floor
          come down from 6em to 4em with the SAME number of visible lines — the 26px the row
          was taking is 26px the transcript gets back (styles.css, `.pane-body >
          .cw-sent-panel`). */}
      <div className="cw-decode cw-sent-panel panel" title={t('cw.sent.title')}>
        <div className="cw-decode-text" ref={sentPin.ref} onScroll={sentPin.onScroll}>
          {sent.map((line, i) => (
            <div key={i} className="cw-sent-line">
              {line}
            </div>
          ))}
        </div>
      </div>
    </CockpitPaneFrame>
  ) : null

  /* Aux panes — ONE merged rig-control strip plus the band + copilot boards. In the 3-column
     tier they take the middle column; below that they append to the transcript column.
     civScope and flexScope are mutually exclusive (one scope feed), so at most one
     scope group renders.

     THE THREE RIG STRIPS SHARE ONE FRAME (2026-08-04 density pass, the move Operate's
     header-density pass made). Scope controls, DSP toggles and RX DSP levels are three
     one-row control groups, and each was wrapped in its own [head + body padding + border +
     column gap] — 44px of chrome around ~30px of chips, twice over. They are now one
     `.cw-rigctl` wrapping strip inside one "Rig controls" frame, and at the tier-2 column
     width they sit on ONE row.

     THE ⊞ VOCABULARY IS UNCHANGED, deliberately: `scopeCtl`, `dsp` and `rxdsp` are still
     three separate ids with three separate menu entries and three separate reason notes, each
     still gating exactly the group it names. Merging the FRAMES is a layout decision; taking
     an operator's three switches away and giving him one would be a different, worse change.
     The frame itself carries `paneId="rigctl"`, which is NOT in CW_PANEL_IDS — the same
     sanctioned spelling `assist` already uses for a frame the menu does not own. */
  const rigCtlPane = hasRigCtlPane ? (
    <CockpitPaneFrame title={t('cw.pane.rigctl.title')} paneId="rigctl" fit="content">
      <div className="cw-rigctl">
      {/* Rig scope controls (native Icom CI-V only) — command the RADIO's real panadapter:
          span sets the hardware sweep width, ref sets weak-signal visibility. Parity with Phone. */}
      {hasScopeCtlPane && civScope && (
          <div className="ph-rigscope" role="group" aria-label={t('cw.rigScope.aria')}>
            <span className="ph-rigscope-lbl" title={t('cw.rigScope.title')}>
              {t('cw.rigScope.label')}
            </span>
            <div className="ph-span">
              {RIG_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('cw.rigScope.span.title', { span: sp.label })}
                  onClick={() => void setScopeSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('cw.rigScope.ref.title')}>
              <span>{t('cw.scope.ref.label')}</span>
              <input
                type="range"
                min={-200}
                max={200}
                step={5}
                value={scopeRefTenths}
                onChange={(e) => changeScopeRef(Number(e.target.value))}
                aria-label={t('cw.rigScope.ref.aria')}
              />
              <span className="ph-power-val">
                {(scopeRefTenths / 10).toFixed(1)} {DB}
              </span>
            </label>
          </div>
      )}

      {/* FlexRadio SmartSDR panadapter controls — bandwidth + reference. Parity with Phone. */}
      {hasScopeCtlPane && flexScope && (
          <div className="ph-rigscope" role="group" aria-label={t('cw.flexPan.aria')}>
            <span className="ph-rigscope-lbl" title={t('cw.flexPan.title')}>
              {t('cw.flexPan.label')}
            </span>
            <div className="ph-span">
              {FLEX_SPANS.map((sp) => (
                <button
                  key={sp.label}
                  type="button"
                  className="theme-chip"
                  title={t('cw.flexPan.span.title', { span: sp.label })}
                  onClick={() => void setFlexPanSpan(sp.hz).then((s) => onSnap?.(s)).catch(() => {})}
                >
                  {sp.label}
                </button>
              ))}
            </div>
            <label className="ph-rigscope-ref" title={t('cw.flexPan.ref.title')}>
              <span>{t('cw.scope.ref.label')}</span>
              <input
                type="range"
                min={-140}
                max={-20}
                step={5}
                value={flexRefDbm}
                onChange={(e) => changeFlexRef(Number(e.target.value))}
                aria-label={t('cw.flexPan.ref.aria')}
              />
              <span className="ph-power-val">
                {flexRefDbm} {DBM}
              </span>
            </label>
          </div>
      )}

      {/* DSP toggles (NB/NR/Notch) — capability-gated; only funcs the rig reports render. */}
      {hasDspPane && (
          <div className="ph-dsp" role="group" aria-label={t('cw.dsp.aria')}>
            <span className="ph-dsp-label">{DSP}</span>
            {cwDspFuncs.map((f) => {
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
                      .catch(() => pushToast(t('cw.dsp.toggleFailed', { func: f.label }), 'error'))
                  }
                >
                  {f.label}
                </button>
              )
            })}
          </div>
      )}

      {/* RX DSP levels — NR depth + AGC speed, each shown only when the rig reports it. Parity
          with the Phone cockpit; a CW op leans on AGC speed and NR depth heavily. */}
      {hasRxDspPane && (
          <div className="ph-dsp-levels" role="group" aria-label={t('cw.rxDsp.aria')}>
            {snap.radio.nrLevel != null && (
              <label className="ph-dsplev" title={t('cw.rxDsp.nr.title')}>
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
                  aria-label={t('cw.rxDsp.nr.aria')}
                />
                <span className="ph-power-val">{nr}%</span>
              </label>
            )}
            {snap.radio.agc != null && (
              <div
                className="ph-agc"
                role="group"
                aria-label={t('cw.rxDsp.agc.aria')}
                title={t('cw.rxDsp.agc.title')}
              >
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
      )}
      </div>
    </CockpitPaneFrame>
  ) : null

  const auxPanes = (
    <>
      {rigCtlPane}

      {/* CW spot band-activity strip; ⧉ pops the vertical band map into its own window. */}
      {hasBandPane && onWorkSpot && (
        <CockpitPaneFrame title={t('cw.pane.bandActivity.title')} paneId="bandActivity" fit="content">
          <BandStrip
            band={snap.radio.band}
            dialMhz={snap.radio.dialMhz}
            txAllowed={snap.radio.txAllowed}
            spots={spots ?? []}
            spotMode="CW"
            needByCall={needByCall}
            typeByCall={typeByCall}
            onWorkSpot={onWorkSpot}
            onPopOut={() => void openPanelWindow('bandmapCw')}
            // Wheel-tune from the strip (#96): same step + sensitivity + gate as the scope above.
            sideband={snap.radio.sideband || 'USB'}
            tuneEnabled={catOk && !snap.radio.txBusyReason && !snap.radio.transmitting}
            stepHz={tuneStep}
            wheelSensitivity={wheelSensitivity}
            onSnap={onSnap}
          />
        </CockpitPaneFrame>
      )}

      {/* CW copilot — decoded-call chips + (Guided) the next-step prompt. Configurable for
          new hams (Guided: plain-English prompts + the next key highlighted) vs experienced
          ops (Expert: just the chips). Nothing here transmits — the operator always keys. */}
      {hasCopilotPane && (
        <CockpitPaneFrame title={t('cw.pane.copilot.title')} paneId="copilot" fit="content">
          <div className="cw-copilot panel expert">
            <div className="cw-copilot-chips">
              {guide.workedCall ? (
                <span className="cw-copilot-label">{t('cw.copilot.working.label')}</span>
              ) : cand.length > 0 ? (
                <span className="cw-copilot-label">{t('cw.copilot.heard.label')}</span>
              ) : (
                <span className="cw-copilot-label dim">{t('cw.copilot.empty')}</span>
              )}
              {guide.workedCall && (
                <span className="cw-chip worked" title={t('cw.copilot.worked.title')}>
                  {guide.workedCall}
                  {guide.rst ? ` · ${guide.rst}` : ''}
                  {guide.name ? ` · ${guide.name}` : ''}
                </span>
              )}
              {cand
                .filter((c) => c.call !== guide.workedCall)
                .map((c) => (
                  <button
                    key={c.call}
                    type="button"
                    className={`cw-chip${c.best ? ' best' : ''}`}
                    onClick={() => workCall(c.call)}
                    title={t('cw.copilot.work.title', { call: c.call })}
                  >
                    {c.call}
                  </button>
                ))}
            </div>
          </div>
        </CockpitPaneFrame>
      )}
    </>
  )

  const logPane = (
    <CockpitPaneFrame title={t('cw.pane.log.title')} paneId="log">
      {/* compactRecall died here (2026-07-31) — same reasoning as PhoneCockpit's log pane: the
          pane grid made this pane's .pane-body the scroller, so the FULL recall card (photo /
          bearing / history) can no longer crush the cockpit the way it did pre-overhaul. */}
      <LogEntry
        snap={snap}
        mode="CW"
        defaultRst="599"
        exchange="terrestrial"
        // The frame head above already reads LOG and is this pane's accessible name.
        titled={false}
        onSpot={(call) => {
          setSpotCall(call)
          setSpotOpen(true)
        }}
        pendingWork={pendingWork}
        onConsumeWork={onConsumeWork}
        cwLive={{
          call: guide.workedCall ?? cand.find((c) => c.best)?.call ?? null,
          rst: guide.rst,
          name: guide.name,
          confirmed: guide.workedCall != null,
        }}
        fieldDay={fieldDay}
        fdMode="CW"
      />
    </CockpitPaneFrame>
  )

  return (
    <main className="layout single cw-cockpit" ref={cockpitRef}>
      <CockpitHeader
        snap={snap}
        onSnap={onSnap}
        modeIndicator={
          <span className="cw-mode-badge" title={snap.radio.catDetail || t('cw.header.mode.title')}>
            {CW}
          </span>
        }
        bandControl={<BandPicker snap={snap} mode="cw" onSnap={onSnap} />}
        onCommitDial={commitDial}
        actions={
          host && panels ? (
            <PanelsMenu
              items={host.menuItems}
              onToggle={(id, show) => panels.setPanelState(id as CwPanelId, show ? 'docked' : 'removed')}
              onUndo={panels.undo}
              canUndo={panels.canUndo}
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
        onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
        onAtuTune={() =>
          void atuTune()
            .then((s) => onSnap?.(s))
            .catch((e) => pushToast(String(e), 'error'))
        }
        onStopTx={abort}
      >
        <label
          className="cw-wpm"
          // Compact Mac keyboards have no PgUp/PgDn keys — Fn+↑/↓ is what sends them, which is
          // a second statement rather than a tail on the first: one whole sentence each.
          title={IS_MAC ? t('cw.wpm.title.mac') : t('cw.wpm.title')}
        >
          <span>{t('cw.wpm.label')}</span>
          <input
            type="range"
            min={WPM_MIN}
            max={WPM_MAX}
            value={wpm}
            onChange={(e) => {
              wpmTouched.current = true
              changeWpm(Number(e.target.value))
            }}
            aria-label={t('cw.wpm.aria')}
          />
          <span className="cw-wpm-val">
            {wpm} {WPM}
          </span>
        </label>
        {/* THE KEYER BACK-END, one select (2026-08-04 density pass — the move Operate's header
            made with its DXped chips). Four always-visible option buttons stood ~292px wide in
            a header that also carries the band picker, the tuning strip, Tune, Stop TX, speed,
            pitch, macros, BW, memories and the rotator; at 1024 that width is what wraps the
            header onto a second and third row, and a wrapped header is what pushes Stop TX
            toward the shell's scrolling valve. ~110px now.
            EVERY WORD OF THE FOUR TITLES SURVIVES: each is on its own <option>, and the title
            of the SELECTED back-end is on the select itself, so the one an operator is
            actually keying with is one hover away — including the soundcard warning, which is
            the one that matters on the air (route audio to the rig, keep drive below ALC). */}
        <label className="cw-wpm" title={keyerHelpText[keyer]}>
          <span>{t('cw.keyer.label')}</span>
          <select
            className="settings-input cw-keyer-select"
            value={keyer}
            onChange={(e) => changeKeyer(e.target.value as typeof keyer)}
            aria-label={t('cw.keyer.aria')}
          >
            {/* Every `value` is the stored token. Two of the four LABELS are tokens as well —
                CAT is a protocol and WinKeyer K1EL's product — and the other two are words. */}
            <option value="cat" title={keyerHelpText.cat}>
              {CAT}
            </option>
            <option value="serial" title={keyerHelpText.serial}>
              {t('cw.keyer.serial.label')}
            </option>
            <option value="winkeyer" title={keyerHelpText.winkeyer}>
              {WINKEYER}
            </option>
            <option value="soundcard" title={keyerHelpText.soundcard}>
              {t('cw.keyer.soundcard.label')}
            </option>
          </select>
        </label>
        <label className="cw-wpm" title={t('cw.pitch.title')}>
          <span>{t('cw.pitch.label')}</span>
          <input
            type="number"
            className="settings-input cw-pitch"
            min={300}
            max={1200}
            step={10}
            value={pitch}
            onChange={(e) => changePitch(Number(e.target.value))}
            aria-label={t('cw.pitch.aria')}
          />
        </label>
        {profiles.length > 1 && (
          <label className="cw-wpm" title={t('cw.macroProfile.title')}>
            <span>{t('cw.macroProfile.label')}</span>
            <select
              className="settings-input"
              value={activeProfile}
              onChange={(e) => switchProfile(Number(e.target.value))}
              aria-label={t('cw.macroProfile.aria')}
            >
              {profiles.map((p, i) => (
                <option key={i} value={i}>
                  {/* The operator's own profile NAME, never translated; only what an unnamed
                      one is called is prose. */}
                  {p.name || t('cw.macroProfile.unnamed', { n: i + 1 })}
                </option>
              ))}
            </select>
          </label>
        )}
        {catOk && (
          <div className="ph-filter" title={t('cw.filter.title')}>
            <span className="ph-filter-lbl">{BW}</span>
            <button
              type="button"
              className="ph-filter-step"
              onClick={() => bumpFilter(-FILTER_STEP_HZ)}
              title={t('cw.filter.narrower.title', { step: FILTER_STEP_HZ })}
            >
              −
            </button>
            <span className="ph-filter-val mono">{filterHz ? `${filterHz}` : '—'}</span>
            <button
              type="button"
              className="ph-filter-step"
              onClick={() => bumpFilter(FILTER_STEP_HZ)}
              title={t('cw.filter.wider.title', { step: FILTER_STEP_HZ })}
            >
              +
            </button>
          </div>
        )}
        {onRecallMemory && (
          <MemoryStrip
            dialMhz={snap.radio.dialMhz}
            mode="CW"
            onRecall={onRecallMemory}
            onManage={onOpenMemories}
          />
        )}
        <RotorStrip
          onOpenSettings={onOpenSettings}
          targetCall={guide.workedCall}
          onPointAt={(call) =>
            pointRotatorAtCall(call)
              .then((bearing) =>
                pushToast(t('cw.rotator.pointed', { call, bearing: Math.round(bearing) }), 'info'),
              )
              .catch((e) =>
                pushToast(
                  t('cw.rotator.failed', { error: e instanceof Error ? e.message : String(e) }),
                  'error',
                ),
              )
          }
        />
        {/* ⭐ A REAL SPLIT CONTROL, not a read-only plate. Until 2026-08-26 this header only
            DISPLAYED that split was on; there was no way to set it from the CW cockpit at all.
            A General working a DX in the Extra-only CW bottom — RX 14.015, TX 14.026, which is
            simply how DX is worked — had to reach for the radio's front panel and then found
            Nexus refusing to key, because the privilege gate had no way to learn where he was
            transmitting. Fixing the gate without this left the fix unreachable by the operator
            who reported it, and he was a CW operator.

            Gated on `catOk` like Phone's: with no CAT there is nothing to command, and the
            header is width-critical at 1024 (see the density note above), so it costs nothing
            when there is no radio to talk to. NOT a stop control — see SplitControl's header. */}
        {catOk && (
          <SplitControl
            snap={snap}
            onSnap={onSnap}
            onError={(m) => pushToast(m, 'error')}
          />
        )}
        {/* Dot + "REC", the same `.ph-rec` class Phone uses. This header already carries the band
            picker, tuning strip, Tune, Stop TX, speed, pitch, macros, BW, memories and the rotator,
            and its own density note records that width here is what wraps it at 1024, so this was
            first built glyph-only to cost ~24px. That is exactly what made Phone's copy unfindable
            (operator, 2026-08-08), so it carries the label and costs ~50px. The accessible name is
            explicit rather than left to the glyph, and the glyph is aria-hidden so it is not read
            twice. */}
        <button
          type="button"
          className={`ph-rec${recording ? ' on' : ''}`}
          onClick={toggleRecord}
          disabled={recBusy}
          aria-label={recording ? t('cw.record.stop.aria') : t('cw.record.start.aria')}
          title={recording ? t('cw.record.on.title') : t('cw.record.off.title')}
        >
          <span className="ph-rec-dot" aria-hidden="true">
            {recording ? '■' : '●'}
          </span>
          {REC}
        </button>
      </CockpitHeader>

      {keyerError && (
        <div className="cw-keyer-warn" role="alert">
          ⚠ {keyerError}
        </div>
      )}

      {/* The standing caution, BELOW a live error because an error outranks a notice. Same
          sanctioned `.cw-keyer-warn` shell-child kind (the census admits one alert kind here,
          not two), warning-toned rather than error-toned via `.caution`: this never blocks and
          never disables the keyer, which keeps working if it works. The sentence is the SAME
          catalog string Settings ▸ CW renders. */}
      {catCwUnprovenHere && (
        <div className="cw-keyer-warn caution" role="status">
          ⚠ <T k="settings.cw.keyer.unproven" tags={{ b: <strong /> }} />
        </div>
      )}

      {/* THE SCOPE STRIP, ⊞-hideable since 2026-08-16 — the Phone twin of this gate carries the
          full reasoning. In short: the shell's four-child census is unchanged (one of the four is
          simply conditional), the gate is at the SHELL CHILD so no empty bordered box is left
          holding its `flex: 0 1 13%` basis and 8em floor, the Splitter goes with it (it drags
          --cw-scope-h, which is that basis, and would otherwise strand a seam), and
          `.cockpit-panes { flex: 1 1 0 }` takes the freed height with no rule change.

          Nothing here stops a transmission: the strip is a display plus click- and scroll-to-tune,
          and Stop TX / Tune sit in the header, outside every ⊞ id (THE STOP LINE). Scroll-to-tune
          survives a hide/show because useWheelTune re-attaches on the TARGET's identity, not the
          hook's mount — which it did not do until this change; see the note there. */}
      {shown('scope') && (
        <>
      <section className="ph-scope-panel" ref={scopeRef} title={t('cw.scope.tuneHint')}>
        <div className="ph-scope-head">
          {/* When a native panadapter drives the scope, name it honestly (real RF spectrum);
              otherwise it's the CW-narrow audio view for zero-beating. */}
          <span
            className="ph-scope-title"
            title={
              nativeRf
                ? t('cw.scope.nativeRf.title')
                : t('cw.scope.audio.title', { lo: cwView.loHz, hi: cwView.hiHz })
            }
          >
            {nativeRf ? t('cw.scope.nativeRf.label') : t('cw.scope.audio.label')}{' '}
            <span className="ph-scope-sub">{scopeSub}</span>
          </span>
          {/* ⭐ THE ZERO-BEAT INDICATOR sits in the SCOPE HEAD, beside the marker it
              completes: the scope below draws your pitch, this says where the received
              tone actually is, and its needle runs in the scope's own axis so the two can
              never disagree. It is chrome in an existing row — no new shell child, no new
              pane, no ⊞ id — and it is a display only: nothing here can move the radio. It
              goes with the scope when the strip is hidden, which is right, because it is
              the other half of that picture. */}
          <ZeroBeat targetHz={pitch} filterHz={filterHz} />
          <span className="ph-scope-head-label">{t('cw.scope.colors.label')}</span>
          <PalettePicker />
        </div>
        {nativeRf && (
          // Native RF panadapter: client-side RF-width zoom around the dial (mirror of Phone).
          <div className="ph-span" role="group" aria-label={t('cw.rfZoom.aria')}>
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
        )}
        {/* CW-narrow view — 800 Hz CENTERED ON THE PITCH (cwScopeWindow) — unless a native RF
            scope is streaming, in which case we show the real RF spectrum around the dial. The
            dashed hairline is YOUR pitch, and it now sits mid-screen where a rig puts it. */}
        <PhoneScope
          transmitting={snap.radio.transmitting}
          theme={theme}
          smeterDb={smeterDb}
          viewLoHz={nativeRf ? rfSpan.lo : cwView.loHz}
          viewHiHz={nativeRf ? rfSpan.hi : cwView.hiHz}
          markerHz={nativeRf ? undefined : pitch}
          sideband={scopeMode}
          dialHz={snap.radio.dialMhz > 0 ? Math.round(snap.radio.dialMhz * 1e6) : null}
          onFeed={(source, loHz, hiHz) => setScopeFeed({ source, loHz, hiHz })}
          onTune={onScopeTune}
          filterWidthHz={filterHz ?? 500}
          pitchHz={pitch}
          cwPitchRefDial={keyer !== 'soundcard'}
          traceHoldMs={TRACE_HOLD_MS.fast}
          interactive={catOk && !snap.radio.txBusyReason && !snap.radio.transmitting && snap.radio.dialMhz > 0}
        />
      </section>
      <Splitter
        axis="y"
        varName="--cw-scope-h"
        target={cockpitRef}
        storageKey="nexus.split.cw.scope"
        min={SCOPE_SPLIT_MIN}
        max={SCOPE_SPLIT_MAX}
        defaultPct={13}
        label={t('cw.scope.splitter.label')}
      />
        </>
      )}

      {/* THE PANE REGION — one CockpitPaneFrame grid for every operator-content block.
          useRegionCols OWNS data-cols (measured from the region itself, stamped
          imperatively — never rendered here); the JSX renders exactly as many
          .cockpit-col groups as the tier: at 1 the two-column grouping simply stacks and
          the region is the scroller; at 2 it is transcripts+aux | log; at 3 DECODE takes
          the leading (widest) track on its own, which is what "the decode is the primary
          pane" means once growth is an fr share instead of a sole flex grower. Columns
          are implicit-row grids, so a pane hidden from ⊞ Panels leaves no empty cell.

          The leading column is not rendered when it would be EMPTY — every CW pane but the
          log is ⊞-removable, and an empty `.cockpit-col` is a `minmax(0,1fr)` grid row of
          dead space beside the log: the "band of empty black", one level below the empty
          TRACK maxCols already collapses. (It used to be merely cheap, because one track
          meant content-height rows; the track count is a content budget, not a width claim,
          so that state is now genuinely bounded — see useRegionCols. Phone's twin gate is
          `leadPresent`.) The key stays "main", so the log column reconciles unmoved.

          The columns are KEYED so a tier flip reconciles them by identity: the log
          column (and the transcript column) keep their fibers across 2↔3, because a
          pane that lands under a different column div is UNMOUNTED by React — which
          wiped every in-progress LogEntry field mid-QSO (fix-round D1, 2026-07-31;
          guarded by CwCockpit.structure.test.tsx). Aux panes still change columns on a
          2↔3 flip and do remount — their state (guide, candidates, slider values) lives
          in this component, so the residual is cosmetic and accepted. */}
      <div className="cockpit-panes" ref={panesRef}>
        {cols === 3 ? (
          <>
            <div className="cockpit-col" key="main">
              {decodePane}
              {sentPane}
            </div>
            <div className="cockpit-col" key="aux">{auxPanes}</div>
            <div className="cockpit-col" key="log">
              {logPane}
            </div>
          </>
        ) : (
          <>
            {(mainPresent || auxPresent) && (
              <div className="cockpit-col" key="main">
                {decodePane}
                {sentPane}
                {auxPanes}
              </div>
            )}
            <div className="cockpit-col" key="log">
              {logPane}
            </div>
          </>
        )}
      </div>

      {/* TX DOCK — the transmit chrome, pinned OUTSIDE the pane region so no pane layout,
          stored or hand-edited, can move, hide or scroll it away. Tune and Stop TX are up in
          the header and have no ⊞ id: that is THE STOP LINE, and it is the only part of this
          the rule requires. The F-key macros and the send bar have no id either — they are
          SENDERS, which the rule is indifferent to (Operate's Tx messages is a hideable one),
          so that is this cockpit keeping its one-click transmits where the operator put them,
          not a safety constraint. Same reading as the pane-region comment above. The TX meters
          keep their ⊞ id (`txmeters` in CW_PANEL_IDS — a readout, not a control) and render at
          the TOP of the dock: they mount only while keyed, and growing the dock downward would
          shift the Send button under the operator's pointer mid-QSO. */}
      <div className="cockpit-txdock">
        {/* Live transmit meters (SWR / ALC / Po / COMP) — self-gating: shown only while keyed,
            and only the meters the rig reports. A CW op wants SWR + Po as they send. */}
        {shown('txmeters') && <TxMeters radio={snap.radio} />}

        <div className="cw-macros" role="group" aria-label={t('cw.macros.aria')}>
          {/* The buttons ADVERTISE their F-keys, and default Mac keyboards eat bare F-keys
              as media keys — the tooltip carries the cure there (mac QA audit). */}
          {macros.map((m) => (
            <button
              key={m.key}
              type="button"
              className="cw-macro"
              onClick={() => send(m.text)}
              title={`${previews[m.key] || m.text}${IS_MAC ? `\n${FN_KEY_HINT}` : ''}`}
            >
              <span className="cw-macro-key">{m.key}</span>
              {/* A built-in caption that is a WORD arrives as a key; one that is on-air
                  shorthand (CQ, 73, AGN, TU) and an operator's own profile label are both
                  written as they stand. */}
              <span className="cw-macro-label">{m.labelKey ? t(m.labelKey) : m.label || m.key}</span>
            </button>
          ))}
        </div>

        <div className="cw-send">
          <input
            className="settings-input cw-type"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                sendTyped()
              }
            }}
            placeholder={t('cw.compose.placeholder')}
            autoComplete="off"
            spellCheck={false}
          />
          <button type="button" className="cw-send-btn" onClick={sendTyped} disabled={!text.trim()}>
            {t('cw.compose.send.label')}
          </button>
        </div>
      </div>
      <SpotDialog
        open={spotOpen}
        onClose={() => setSpotOpen(false)}
        initialCall={spotCall}
        freqMhz={snap.radio.dialMhz}
        defaultComment="CW"
      />
    </main>
  )
}

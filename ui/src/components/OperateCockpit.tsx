// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every reading in this
// cockpit is DATA and stays in the code — the dial, the audio offsets in Hz, the band, the
// tier, the decode depth, the split TX frequency, the next-slot seconds — and so does the
// technical vocabulary the header is built out of: the mode chips' own names and T/R slot
// lengths, `Native`/`Companion` (the backend's own words for the signal source, which the
// group tooltip interpolates beside them), the `Rx`/`Tx` of the two DF spinners, the `Hz`
// unit and the `SPLIT ▲` and `HOUND` annunciators. What moved is the prose around them.
//
// Nothing that stops or keys a transmission is in this file: Operate's stop line is Stop TX
// and Tune in `OperateQsoStrip.tsx` (both deferred, see that file's header) plus the Esc
// binding below, which is a keyboard handler with no string of its own.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { t } from '../i18n'
import { engagedInQso } from '../alerts'
import type {
  AppSnapshot,
  BandChannel,
  ModeRequest,
  LoggedQso,
  NeedAlert,
  NeedTag,
  QrzLookup,
  Settings,
  SourceKind,
  Tier,
} from '../types'
import { isRxOnly, isBeacon } from '../types'
import type { NeedBandScopes } from '../features/needs'
import { bandKey, callHistory, entitySlots, isNewEntity, modeKey } from '../features/callHistory'
import { bandLabelForMhz } from '../band'
import {
  clampOffsetHz,
  cqDirFromText,
  genStdMessages,
  snrForCall,
  stdMessageList,
  toggleIgnored,
} from '../txMessages'
import { atuTune, openPanelWindow, getSettings, notifyErase, setSettings, setMsk144Period, type FdRulesetDto } from '../api'
import { FdAdvisories } from './FdAdvisories'
import { pointRotatorAtCall, redecode, startCq, startQsoRecording, stopQsoRecording } from '../api'
import { setDecodeDepth } from '../api'
import { setSkipTx1 as setSkipTx1Cmd } from '../api'
import { getLog, qrzLookup, resolveEntity } from '../api'
import { pushToast } from '../toast'
import { SplitControl } from './SplitControl'
import { RotorStrip } from './RotorStrip'
import { FastGraph } from './FastGraph'
import { Waterfall } from './Waterfall'
import { FT_PALETTE_SCOPE } from '../waterfallPalette'
import { Splitter } from './Splitter'
import { SplitterSeam } from './SplitterSeam'
import { buildHighlightMap, OperateDecodes } from './OperateDecodes'
import { DecodeHistory } from '../decodeHistory'
import { OperateQsoStrip } from './OperateQsoStrip'
import { TxMeters, TX_METERS_WHEN } from './TxMeters'
import { SpotDialog } from './SpotDialog'
import { OperateRoster } from './OperateRoster'
import { RecallPanel } from './RecallPanel'
import { TxPanel } from './TxPanel'
import { CockpitHeader } from './CockpitHeader'
import { PanelsMenu } from './PanelsMenu'
import { WATERFALL_DETACHED_KEY, type OperatePanelId, type PanelLayoutApi } from '../features/panelState'
import { panelHost, type PanelHostSpec } from '../features/panelHost'
import { FrequencyControl } from './FrequencyControl'
import { TuningStrip } from './TuningStrip'
import { IS_MAC, FN_KEY_HINT } from '../platform'

interface Props {
  /** Configured companion UDP listen address (Settings) — shown instead of a
   * hardcoded :2237 so a moved WSJT-X port reads truthfully. */
  companionAddr?: string
  /** Field Day master switch + the active event's ruleset facts — the warn-only
   * banned-mode chip in the header (a passive status div, outside every
   * ⊞-removable pane; nothing is ever removed or disabled by rule). */
  fdActive?: boolean
  fdRuleset?: FdRulesetDto | null
  snap: AppSnapshot
  theme: string
  /** Active mode/tier (authoritative from the snapshot's link). */
  tier: Tier
  onTierChange: (t: Tier) => void
  /** Open the Logbook filtered to a callsign (#192) — handed to the recall card in the side
   *  rail, whose previous-contact rows become clickable when it is present. Omitted ⇒ inert. */
  onOpenLogbook?: (call: string) => void
  /** Switch the RX signal source (native engine vs WSJT-X companion over UDP). */
  onSourceChange: (k: SourceKind) => void
  /** Click-to-tune on the waterfall: left=TX, right=RX, both buttons=TX+RX. */
  onTune: (freqHz: number, target: 'tx' | 'rx' | 'both') => void
  /** Work / answer a decoded station (double-click a decode or roster row). */
  onCall: (call: string, grid?: string, message?: string, snr?: number, freq?: number) => void
  /** The persistent blocked-callsigns list (settings.blockedCalls). When provided, the
   * cockpit's ignore surfaces read it instead of the session-only set. */
  blockedCalls?: string[]
  /** Toggle a call on the persistent blocklist (api.setBlockedCalls — the narrow write).
   * When absent, Alt-double-click falls back to the session-only dimming set. */
  onToggleBlocked?: (call: string) => void
  /** Set the TX audio drive level (0.0–1.0) — the Pwr slider. */
  onSetTxLevel: (level: number) => void
  /** Band plan (channel select) shown in the header — FT8/FT4 freq+band moved
   * out of the global TopBar into the cockpit header. */
  bandPlan: BandChannel[]
  /** Commit a dial/band/mode change (the app's setFrequency handler). */
  onSetFrequency: (dialMhz: number, band: string, mode: string) => void
  /** Switch the QSO sequencer role (Call CQ / Monitor). */
  onSetMode: (mode: ModeRequest) => void
  /** Set the transmit period (Tx 1st/even vs Tx 2nd/odd). */
  onSetTxEven: (even: boolean) => void
  onSetTxCycleAuto: (auto: boolean) => void
  /** Re-arm the current QSO message. */
  onResend: () => void
  /** Send in-QSO free text (Tx5). */
  onFreetext: (text: string) => void
  /** Log the active QSO now (inline button). */
  onLog: () => void
  /** WSJT-X Tx-slot click: force `text` as the next transmission to `call`. */
  onOverrideTx: (call: string, grid: string | null, text: string) => void
  /** Halt TX immediately (the Esc key — same api as the Stop TX button). */
  onHaltTx: () => void
  /** TX-control cluster consolidated into the QSO strip (beside CQ/S&P). */
  onSetTxEnabled?: (on: boolean) => void
  onSetTune?: (on: boolean) => void
  onSetHoldTxFreq?: (on: boolean) => void
  /** Apply a fresh snapshot returned by a cockpit-local api call. */
  onSnap?: (s: AppSnapshot) => void
  /** Roger the final with RRR instead of RR73 (Settings preferRrr). */
  preferRrr?: boolean
  /** Bumps when a QSO was logged with "Clear DX call after logging" on — the
   * panel's DX fields wipe (stock WSJT-X option). */
  dxClearTick?: number
  /** Operator QSO macros — the Tx5 free-text datalist suggestions. */
  qsoMacros?: string[]
  /** The compact Call Roster (a wired StationList) shown in the Classic side column. */
  roster: ReactNode
  /** Award-need tier per call — drives the Roster layout's Need column + sort. */
  needByCall: Map<string, NeedTag>
  /** Full NeedAlerts per call — drives the band-activity decode feed's need icons +
   * row colour. Forwarded to every OperateDecodes instance. */
  needAlertsByCall?: Map<string, NeedAlert[]>
  /** The operator's per-type alert band scopes — forwarded to every OperateDecodes so the
   * need icons honour them exactly as the sound/toast does. The ROSTER needs none: its
   * alerts arrive already scoped. */
  needScopes?: NeedBandScopes
  /** Currently selected/open station (highlighted in the Roster layout). */
  selectedCall: string | null
  /** Select (open) a station from the Roster layout (single click). */
  onSelect: (call: string) => void
  /** Layout: 'classic' (WSJT-X — Band Activity dominant + compact roster aside) or
   * 'roster' (GridTracker — the full sortable Call Roster dominant). */
  layoutMode: 'classic' | 'roster'
  onLayoutMode: (m: 'classic' | 'roster') => void
  /** Open Operate in its own window (omit when already standalone). */
  onPopOut?: () => void
  /** Which panels this surface shows, hides, or has torn off. Owned by the HOST
   * (App's `.operate-host` / DetachedPanel), never here — the cockpit remounts and
   * would lose it. */
  panels: PanelLayoutApi<OperatePanelId>
  /** Recall a saved memory (App applies settings + retune + cockpit switch).
   * Absent when the Memories feature is disabled — the MEM strip then hides. */
  /** Open Settings at a section id (see settings/registry.ts). Absent ⇒ the surfaces that
   * point at Settings stay plain text. */
  onOpenSettings?: (target: string) => void
  /** Wheel sensitivity (Settings) — how much scroll one tuning step costs on the readout. */
  wheelSensitivity?: number
  /** True when the cockpit is the active view. The cockpit stays MOUNTED across
   * navigation (so Band Activity keeps accumulating in the background); this flag
   * pauses the waterfall's render loop while it's hidden. */
  active?: boolean
}

/** Mode chips, in the order the cockpit presents them (popular modes first). */
// The DX-area cockpit operates the structured WSJT-X modes only. TempoFast/TempoDeep live in
// the MSG (Chat) area — no mixed tier picker.
/** The cockpit header's own mode row stays FT8/FT4 ONLY. The full tier set —
 *  including the six receive-only WSJT-X modes — lives in the TOP BAR pills,
 *  which is where the operator already picks a tier; duplicating ten tiles into
 *  the cockpit header made the same choice appear in two places, one of which
 *  was easy to miss. */
//
// THE TIER NAMES AND THEIR T/R SLOT LENGTHS ARE TOKENS and stay written here; only the
// explanation is a word, and it resolves through a getter so the record is not frozen to
// whichever locale loaded this module first (the registry rule, batch 3).
const MODES: { tier: Tier; label: string; slot: string; title: string }[] = [
  {
    tier: 'FT8',
    label: 'FT8',
    slot: '15s',
    get title() {
      return t('operate.mode.ft8.title')
    },
  },
  {
    tier: 'FT4',
    label: 'FT4',
    slot: '7.5s',
    get title() {
      return t('operate.mode.ft4.title')
    },
  },
  // FT2 sits beside its siblings here because it IS one: a slotted digital QSO
  // tier running the same FT8/FT4 sequencer, just faster. Decodium's mode, not
  // WSJT-X's — hence the attribution in the tooltip, where the operator can see
  // which population they are calling into.
  {
    tier: 'FT2',
    label: 'FT2',
    slot: '3.75s',
    get title() {
      return t('operate.mode.ft2.title')
    },
  },
]

/** WSJT-X's DXpedition ROLE name and the annunciator that reports it — protocol tokens (an
 *  expedition announces itself as running "Fox/Hound"), not words a translator replaces. */
const HOUND_LABEL = 'Hound'
const HOUND_BADGE = 'HOUND'


/** The two RX signal sources, named exactly as the BACKEND names them: `radio.sourceLabel`
 *  is interpolated into the group tooltip beside these buttons, so a translated button would
 *  disagree with the live readout next to it. Tokens, like a mode name. */
const SOURCE_NATIVE = 'Native'
const SOURCE_COMPANION = 'Companion'

/** WSJT-X's stock UDP endpoint, shown when Settings names no other. A HOST AND PORT never
 *  belongs inside a translatable sentence — it is interpolated in, like a frequency. */
const DEFAULT_COMPANION_ADDR = '127.0.0.1:2237'

/** The two audio-offset spinners: WSJT-X's Rx/Tx Hz fields. `Rx`/`Tx` are direction tokens
 *  that also read out in each field's accessible name, and `Hz` is a unit symbol. */
const DF_RX = 'Rx'
const DF_TX = 'Tx'
const HZ_UNIT = 'Hz'

/** DXpedition special-op chip definitions. */
const SPECIAL_OPS: {
  value: NonNullable<Settings['specialOp']>
  label: string
  title: string
}[] = [
  {
    value: 'none',
    get label() {
      return t('operate.dxped.off.label')
    },
    get title() {
      return t('operate.dxped.off.title')
    },
  },
  {
    value: 'hound',
    label: HOUND_LABEL,
    get title() {
      return t('operate.dxped.hound.title')
    },
  },
  // SuperFox (superhound) retired by operator decision — the QPC table file's
  // license bars vendoring the native decoder outside WSJT-X. A settings file
  // that still says 'superhound' loads fine and behaves as plain Hound.
]

const NO_MACROS: string[] = []

/** Operator-facing names for the removable panels (the ⊞ Panels menu). Resolved when the
 *  menu is BUILT — a module constant would freeze the first locale loaded. */
const panelLabels = (): Record<OperatePanelId, string> => ({
  waterfall: t('operate.panel.waterfall'),
  bandActivity: t('operate.panel.bandActivity'),
  callRoster: t('operate.panel.callRoster'),
  rxfreq: t('operate.panel.rxfreq'),
  txmsgs: t('operate.panel.txmsgs'),
  stations: t('operate.panel.stations'),
  txmeters: t('operate.panel.txmeters'),
})

/** What each layout actually renders — the menu lists only these, so a panel the
 *  current layout has no place for can't be ticked into nowhere. */
const LAYOUT_PANELS: Record<'classic' | 'roster', readonly OperatePanelId[]> = {
  classic: ['waterfall', 'bandActivity', 'txmsgs', 'rxfreq', 'stations', 'txmeters'],
  roster: ['waterfall', 'callRoster', 'bandActivity', 'rxfreq', 'txmeters'],
}

/** Side-rail occupants per layout — the rail unmounts when all of them are removed.
 *  Classic's rail is the Stations roster alone since the decode-first rebuild: the
 *  Rx-Frequency pane + Tx machine hold their own middle column (.cockpit-qsocol). */
const SIDE_PANELS: Record<'classic' | 'roster', readonly OperatePanelId[]> = {
  classic: ['stations'],
  roster: ['bandActivity', 'rxfreq'],
}

/** Classic three-column geometry: Band Activity | the promoted Rx-Frequency column
 *  (decode pane + Tx1–Tx6 machine) | the Stations roster. Drives the populated-column
 *  count (`data-cols`) so removing panels collapses the grid track-for-track. */
const CLASSIC_COLUMNS: readonly (readonly OperatePanelId[])[] = [
  ['bandActivity'],
  ['rxfreq', 'txmsgs'],
  ['stations'],
]

/**
 * The Operate cockpit — the nerve center's primary operating surface. The
 * waterfall is the centerpiece (not a detached rail); a prominent mode selector
 * drives the live native decoder (FT8/FT4/TempoFast/TempoDeep); the Band Activity table
 * accumulates, freezes-on-hover, filters and sorts. Click the waterfall to tune
 * RX (or shift-click for TX); single-click a decode to select it into the Tx
 * panel; double-click to work the station (stock WSJT-X click model).
 */
export function OperateCockpit({
  snap,
  theme,
  tier,
  onTierChange,
  onOpenLogbook,
  bandPlan,
  onSetFrequency,
  onSourceChange,
  onTune,
  onCall,
  blockedCalls,
  onToggleBlocked,
  onSetTxLevel,
  onSetMode,
  onSetTxEven,
  onSetTxCycleAuto,
  onResend,
  onFreetext,
  onLog,
  onOverrideTx,
  onHaltTx,
  onSetTxEnabled,
  onSetTune,
  onSetHoldTxFreq,
  onSnap,
  preferRrr = false,
  dxClearTick = 0,
  qsoMacros = NO_MACROS,
  roster,
  needByCall,
  needAlertsByCall,
  needScopes,
  selectedCall,
  onSelect,
  layoutMode,
  onLayoutMode,
  onPopOut,
  panels,
  active = true,
  companionAddr,
  fdActive = false,
  fdRuleset = null,
  onOpenSettings,
  wheelSensitivity,
}: Props) {
  // Container the waterfall-height splitter measures + writes its CSS var on.
  const bodyRef = useRef<HTMLDivElement>(null)
  // The two resizable side-rail panes in roster mode (Band Activity above, Rx Frequency
  // below) + the seam that splits them. A sole survivor auto-fills because flex-grow is
  // share-driven (`--pane-share`), so deleting Band Activity grows Rx Frequency to fill.
  const decodesSideRef = useRef<HTMLDivElement>(null)
  const rxfreqRef = useRef<HTMLDivElement>(null)
  // Classic 3-col grid: the container (its template consumes the --op-col-a/-b
  // fr tokens the column seam paints on it), the middle qsocol and the roster
  // column — the x-axis seam splits the latter two.
  const lowerClassicRef = useRef<HTMLDivElement>(null)
  const qsocolRef = useRef<HTMLDivElement>(null)
  const classicSideRef = useRef<HTMLElement>(null)
  // Only apply a stored share; an un-dragged pane keeps the CSS default proportions.
  const shareStyle = (id: OperatePanelId): React.CSSProperties | undefined => {
    const s = panels.layout.share[id]
    return s != null ? ({ '--pane-share': s } as React.CSSProperties) : undefined
  }
  // Persisted Classic column shares → fr tokens on the grid container (a grid template
  // cannot read a var off its children). Keys: 'txmsgs' = the qsocol column (its Tx
  // machine — deliberately NOT 'rxfreq', whose share the roster-mode y-seam already
  // owns; sharing the key would let a roster drag silently reshape the Classic
  // columns), 'stations' = the roster column. Undragged → the CSS defaults apply.
  // Values are clamped on load by coercePanelLayout ([MIN_SHARE, 2−MIN_SHARE]).
  const classicColStyle = (): React.CSSProperties | undefined => {
    const a = panels.layout.share['txmsgs']
    const b = panels.layout.share['stations']
    if (a == null && b == null) return undefined
    const s: Record<string, string> = {}
    if (a != null) s['--op-col-a'] = `${a}fr`
    if (b != null) s['--op-col-b'] = `${b}fr`
    return s as React.CSSProperties
  }
  const source = snap.radio.source

  // Live next-slot countdown: the snapshot's nextSlotMs only updates each poll,
  // so anchor it to wall-clock on each new value and tick locally for a smooth
  // 1-second cadence (the WSJT-X period clock operators watch).
  // Cockpit-owned decode histories, one per pane role, shared by BOTH layouts —
  // the cockpit stays mounted across layout switches (and navigation), so the
  // decode window survives Classic ↔ Roster instead of wiping to "no decodes"
  // mid-session (operator report 2026-07-21).
  const bandHistRef = useRef(new DecodeHistory())
  const rxHistRef = useRef(new DecodeHistory())
  const slotBase = useRef({ ms: snap.radio.nextSlotMs, at: Date.now() })
  useEffect(() => {
    slotBase.current = { ms: snap.radio.nextSlotMs, at: Date.now() }
  }, [snap.radio.nextSlotMs])
  const [, tick] = useState(0)
  useEffect(() => {
    // Gated on `active` for the same reason the waterfall loop is: the cockpit
    // stays MOUNTED across navigation (so Band Activity keeps accumulating), so an
    // ungated 4 Hz ticker re-rendered the hidden cockpit — and its two decode
    // panes, up to 300 unvirtualized rows each — on EVERY other section of the app.
    // The JS cost is the same everywhere; the PAINT cost of that DOM churn is what
    // diverges on a machine without GPU compositing, which is where it was noticed.
    if (!active) return
    const id = window.setInterval(() => tick((t) => (t + 1) % 1000), 250)
    return () => window.clearInterval(id)
  }, [active])
  const nextSlotSec = Math.max(
    0,
    Math.ceil((slotBase.current.ms - (Date.now() - slotBase.current.at)) / 1000),
  )

  // --- QSO recording (audio bridge) — same toggle as the Phone cockpit; the
  // global TopBar ● REC badge is the persistent stop once recording is on.
  const [recBusy, setRecBusy] = useState(false)
  // Tuning step (Hz) for the header's TuningStrip nudge/step — shared with the
  // step selector, same control CW/Phone carry (dial-nudge + VFO/RIT/XIT).
  // Tuning step, persisted per cockpit ('nexus.operate.tuneStep'): the cockpit unmounts on every
  // mode switch, so plain state reset the step to 100 Hz on each round-trip (FTDX10
  // report 2026-07-21 — the third remount-state-loss bug today). localStorage so a
  // CW operator's 10 Hz survives restarts too, like nexus.cw.sensitivity.
  const [tuneStep, setTuneStep] = useState(() => {
    try {
      const v = Number(localStorage.getItem('nexus.operate.tuneStep'))
      return Number.isFinite(v) && v > 0 ? v : 100
    } catch {
      return 100
    }
  })
  useEffect(() => {
    try {
      localStorage.setItem('nexus.operate.tuneStep', String(tuneStep))
    } catch {
      /* ignore */
    }
  }, [tuneStep])
  // Skip Tx1 (WSJT-X parity) — session-only UI state; the backend flag is likewise not
  // persisted, so both reset to off each launch. The toggle pushes to the engine.
  const [skipTx1, setSkipTx1] = useState(false)
  const handleSkipTx1 = useCallback((v: boolean) => {
    setSkipTx1(v)
    setSkipTx1Cmd(v).catch(() => {})
  }, [])
  const recording = snap.radio.qsoRecording
  const toggleRecord = () => {
    if (recBusy) return
    setRecBusy(true)
    const fn = recording ? stopQsoRecording : startQsoRecording
    fn()
      .then((s) => onSnap?.(s))
      // ONE whole sentence per outcome, never "Could not " plus a verb.
      .catch(() =>
        pushToast(
          recording
            ? t('operate.header.record.stopFailed')
            : t('operate.header.record.startFailed'),
          'error',
        ),
      )
      .finally(() => setRecBusy(false))
  }

  // --- DXpedition special-op selector ---
  // Fetch once on mount (lightweight — just reads the one field). After the
  // operator picks a chip we patch specialOp + save, then update local state.
  const [specialOp, setSpecialOp] = useState<NonNullable<Settings['specialOp']>>('none')
  const specialOpLoaded = useRef(false)
  useEffect(() => {
    let alive = true
    getSettings()
      .then((s) => {
        if (alive) {
          setSpecialOp(s.specialOp ?? 'none')
          specialOpLoaded.current = true
        }
      })
      .catch(() => {})
    return () => { alive = false }
  }, [])

  const handleSpecialOp = (val: NonNullable<Settings['specialOp']>) => {
    if (val === specialOp) return
    setSpecialOp(val)
    // Patch the one field: fetch current saved settings, apply the change, save.
    getSettings()
      .then((s) => setSettings({ ...s, specialOp: val }))
      .then((freshSnap) => onSnap?.(freshSnap))
      .catch(() => {})
  }

  // --- JTAlert highlight map (built once per highlights array reference) ---
  const highlightMap = useMemo(
    () => buildHighlightMap(snap.highlights),
    [snap.highlights],
  )

  // --- WSJT-X Tx1–Tx6 message machine (the Classic layout's Tx panel) ---
  const [dxCall, setDxCall] = useState('')
  // Spot-to-cluster popup: open state + the call to pre-fill (from the toolbar button or a roster row).
  const [spotOpen, setSpotOpen] = useState(false)
  const [spotSeedCall, setSpotSeedCall] = useState('')
  const openSpot = (call: string) => {
    setSpotSeedCall(call)
    setSpotOpen(true)
  }
  const [dxGrid, setDxGrid] = useState('')
  // Tx5 free text: auto-tracks the generated baseline until the operator edits
  // it; Generate Std Msgs resets the edit and re-baselines (stock behavior).
  const [tx5, setTx5] = useState('')
  const tx5Edited = useRef(false)
  // Tx6 free text: same auto-track pattern as Tx5, but follows msgs.tx6
  // (the CQ message) until the operator edits it for a directed CQ.
  const [tx6, setTx6] = useState('')
  const tx6Edited = useRef(false)
  // Locally picked "next" row (0-based) until qso.txNow confirms one.
  const [localNext, setLocalNext] = useState<number | null>(null)
  // The blocked-callsigns set (Alt-double-click a decode/roster row). PERSISTED and
  // engine-honored when App wires `blockedCalls`/`onToggleBlocked` (the auto-responder
  // never answers a listed call); the session-only useState survives as the fallback for
  // hosts that don't (detached panels, tests) — there it remains display-only dimming.
  const [sessionIgnored, setSessionIgnored] = useState<ReadonlySet<string>>(() => new Set())
  const ignored: ReadonlySet<string> = useMemo(
    () =>
      blockedCalls != null
        ? new Set(blockedCalls.map((c) => c.trim().toUpperCase()))
        : sessionIgnored,
    [blockedCalls, sessionIgnored],
  )

  // --- Panel visibility (⊞ Panels). The record is per-SURFACE and host-owned; here we
  // only read it and render accordingly. A panel with no stored state is docked. The
  // render glue (shown/sideShown/mainShown/dataCols/menuItems) is derived by the reusable
  // panelHost primitive from this layout's spec — so a new cockpit is a spec, not a copy.
  const { stateOf, setPanelState } = panels
  const panelSpec: PanelHostSpec<OperatePanelId> = {
    menu: LAYOUT_PANELS[layoutMode],
    side: SIDE_PANELS[layoutMode],
    main: layoutMode === 'roster' ? 'callRoster' : 'bandActivity',
    labels: panelLabels(),
    // The meters read on transmit — here as the pinned strip, which holds the last
    // readings dimmed between overs, so the entry says WHEN it is populated rather than
    // leaving an operator to guess mid-menu (the same words the strip shows when idle).
    notes: { txmeters: TX_METERS_WHEN },
    ...(layoutMode === 'classic' ? { columns: CLASSIC_COLUMNS } : {}),
  }
  const { shown, sideShown, dataCols, menuItems } = panelHost(panels, panelSpec)
  const wfState = stateOf('waterfall')

  // Waterfall pop-out: 'popped' unmounts the docked copy so the decode lists + roster
  // reclaim the space (that's the whole point of popping it out) and leaves the re-dock
  // bar behind; 'removed' renders nothing at all. The torn-off window still signals its
  // close through the app-global legacy flag (localStorage is shared across Tauri
  // windows), so closing it re-docks automatically — but only a POPPED waterfall
  // follows, or that signal would resurrect one the operator deleted.
  const wfStateRef = useRef(wfState)
  wfStateRef.current = wfState
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== WATERFALL_DETACHED_KEY || e.newValue === '1') return
      if (wfStateRef.current === 'popped') setPanelState('waterfall', 'docked')
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [setPanelState])
  const popOutWaterfall = () => {
    setPanelState('waterfall', 'popped')
    void openPanelWindow('waterfall')
  }
  const redockWaterfall = () => setPanelState('waterfall', 'docked')

  // RPT = the DX's current heard SNR (case-insensitive), −10 when unheard.
  const dxSnr = snrForCall(snap.stations, dxCall)
  const msgs = useMemo(
    () =>
      genStdMessages({
        dxCall,
        myCall: snap.mycall,
        myGrid: snap.mygrid,
        snr: dxSnr,
        preferRrr,
      }),
    [dxCall, snap.mycall, snap.mygrid, dxSnr, preferRrr],
  )
  useEffect(() => {
    if (!tx5Edited.current) setTx5(msgs.tx5)
  }, [msgs.tx5])
  useEffect(() => {
    if (!tx6Edited.current) setTx6(msgs.tx6)
  }, [msgs.tx6])

  const handleTx5 = (v: string) => {
    tx5Edited.current = true
    setTx5(v)
  }
  const handleTx6 = (v: string) => {
    tx6Edited.current = true
    setTx6(v)
  }
  const handleGenerate = () => {
    tx5Edited.current = false
    setTx5(msgs.tx5)
    tx6Edited.current = false
    setTx6(msgs.tx6)
  }
  const clearDx = useCallback(() => {
    setDxCall('')
    setDxGrid('')
    tx5Edited.current = false
    setTx5('')
    // ⚠️ TX6 IS DELIBERATELY NOT CLEARED. It is the CQ message, and it has nothing to do with
    // the DX call this clears — the stock option is "Clear DX call and grid after logging",
    // and that is exactly what it promises. Wiping Tx6 here reset the operator's DIRECTED CQ
    // after every single contact: type "CQ DX KR4FQG EM64", work one station, and the next CQ
    // went out bare (reported 2026-08-23: "it will work for one call, then revert back to just
    // CQ unless I go back to Classic and change it again").
    //
    // WSJT-X keeps the two apart for the same reason: editing Tx6 sets `m_CQtype`
    // (`mainwindow.cpp on_tx6_editingFinished`), a member the DX-clear never touches, so a
    // directed CQ persists across contacts until the operator edits it back. `cqDirFromText`
    // re-reads this field on every Tx6 fire, so keeping the text IS keeping the direction —
    // and clearing it back to a plain CQ stays one edit away.
    setLocalNext(null)
  }, [])

  // Stock "Clear DX call and grid after logging": App bumps the tick when a
  // QSO is logged with the option on. Skip the mount tick.
  const dxClearSeen = useRef(dxClearTick)
  useEffect(() => {
    if (dxClearTick !== dxClearSeen.current) {
      dxClearSeen.current = dxClearTick
      clearDx()
    }
  }, [dxClearTick, clearDx])

  // A retarget/abandon makes a remembered Tx-slot pick meaningless — without
  // this the next-dot stayed lit on a stale row of the NEW station's panel.
  //
  // This is also the ONE place the DX fields get reconciled with the engine. They used to be
  // seeded only by a decode-row single-click, which meant every other way a QSO starts left
  // them empty or holding the PREVIOUS station: the Work/Call buttons, a roster double-click,
  // Shift+Enter, the JTAlert/GridTracker UDP Reply path, an engine-side retarget — and above
  // all the CQ auto-answer, where a caller replies and the sequencer starts the QSO with no
  // click anywhere. An empty dxCall makes genStdMessages return blanks, so Tx1–Tx4 render "—",
  // the DX Grid box is empty and the Tx buttons are dead, all while the QSO runs correctly and
  // logs correctly. Seeding from the snapshot covers every one of those paths at once, and it
  // closes a worse hazard: pressing a Tx row with a STALE dxCall retargets the live QSO to the
  // wrong station. Operator report, 2026-07-25.
  const lastDx = useRef<string | null>(null)
  useEffect(() => {
    const dx = snap.qso?.dxcall ?? null
    // The engine resolves the grid the same way the logged GRIDSQUARE does (the exchange's
    // grid, else the roster's), so screen and log agree by construction.
    const grid = (snap.qso?.dxgrid ?? '').toUpperCase()
    if (dx !== lastDx.current) {
      lastDx.current = dx
      setLocalNext(null)
      if (dx) {
        setDxCall(dx.toUpperCase())
        setDxGrid(grid)
      }
    } else if (dx && grid) {
      // The grid can land LATER than the call — a caller who answered with a bare report has
      // no grid until we decode one from them. Fill an empty box; never overwrite a typed one.
      setDxGrid((prev) => (prev.trim() ? prev : grid))
    }
  }, [snap.qso?.dxcall, snap.qso?.dxgrid])

  // The six panel rows (Tx5 + Tx6 = the live editable texts). The "next" dot
  // follows qso.txNow when it matches a row, else the operator's local pick.
  const rowTexts = stdMessageList({ ...msgs, tx5, tx6 })
  const txNow = snap.qso?.txNow ?? null
  const liveNext = txNow ? rowTexts.indexOf(txNow) : -1
  const nextIndex = liveNext >= 0 ? liveNext : localNext

  /** Fire Tx row n (1-based).
   * Tx6 = Call CQ — parse the editable Tx6 text for a directed token and call
   * startCq(dir | null) directly; apply the returned snapshot via onSnap.
   * Tx1–Tx5 force the row's text as the next transmission to the DX. */
  const doTx = (n: number) => {
    if (n === 6) {
      setLocalNext(5)
      const dir = cqDirFromText(tx6, snap.mycall)
      // dir === undefined → parse failed / malformed → fall back to plain CQ
      const resolved = dir === undefined ? null : dir
      startCq(resolved).then((s) => onSnap?.(s)).catch(() => {})
      return
    }
    const call = dxCall.trim().toUpperCase()
    const text = rowTexts[n - 1]?.trim()
    if (!call || !text) return
    setLocalNext(n - 1)
    onOverrideTx(call, dxGrid.trim().toUpperCase() || null, text)
  }

  /** Re-decode the last period (WSJT-X Decode / F6). */
  const handleRedecode = () => {
    redecode().then((s) => onSnap?.(s)).catch(() => {})
  }

  /** Single-click SELECT from a decode: populate DX Call/Grid only — no RF
   * action, no TX (stock WSJT-X). A decoded grid wins; otherwise a stale grid
   * from a previous DX is cleared. */
  const selectDecode = (call: string, grid?: string) => {
    const up = call.trim().toUpperCase()
    if (grid) setDxGrid(grid.toUpperCase())
    else if (up !== dxCall) setDxGrid('')
    setDxCall(up)
  }

  const handleToggleIgnore = (call: string) => {
    if (onToggleBlocked) onToggleBlocked(call)
    else setSessionIgnored((prev) => toggleIgnored(prev, call))
  }
  const handleSetRx = (hz: number) => onTune(hz, 'rx')

  // Cockpit keyboard (stock WSJT-X): Esc = halt TX, F4 = clear DX, F6 = re-decode,
  // Alt+1…6 = the Tx buttons. Window-level, active-view only, and never while
  // typing in an input/textarea. Handlers ride a ref so the listener binds once
  // per activation without re-subscribing on every keystroke of state.
  const keyRef = useRef({ doTx, clearDx, halt: onHaltTx, redecode: handleRedecode })
  keyRef.current = { doTx, clearDx, halt: onHaltTx, redecode: handleRedecode }
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      // Escape is an ABORT key, not an editing key — it must halt TX even while the
      // operator is typing in the Tx5 free-text field. It is checked BEFORE the
      // typing guard; F4 / F6 / Alt+1–6 stay behind it.
      if (e.key === 'Escape') {
        e.preventDefault()
        keyRef.current.halt()
        return
      }
      const t = e.target as HTMLElement | null
      const tag = t?.tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || t?.isContentEditable) return
      if (e.key === 'F4') {
        e.preventDefault()
        keyRef.current.clearDx()
        return
      }
      if (e.key === 'F6') {
        e.preventDefault()
        keyRef.current.redecode()
        return
      }
      const m = e.altKey && !e.ctrlKey && !e.metaKey ? /^Digit([1-6])$/.exec(e.code) : null
      if (m) {
        e.preventDefault()
        keyRef.current.doTx(Number(m[1]))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active])

  // Click-model props shared by every decode pane (Band Activity + Rx Freq in
  // both layouts) so select/ignore behave identically everywhere.
  const decodeClickProps = {
    onSelectDecode: selectDecode,
    onSetRx: handleSetRx,
    selectedCall: dxCall || null,
    ignoredCalls: ignored,
    onToggleIgnore: handleToggleIgnore,
    highlights: highlightMap,
    clearTick: snap.clearTick ?? 0,
  }

  // Active special-op badge shown next to the TX indicator.
  const specialOpBadge =
    specialOp === 'hound' ? (
      <span className="cockpit-specialop-badge hound" title={t('operate.dxped.hound.title')}>
        {HOUND_BADGE}
      </span>
    ) : specialOp === 'superhound' ? (
      // Retired option still present in an old settings file: same discipline.
      <span className="cockpit-specialop-badge hound" title={t('operate.dxped.hound.title')}>
        {HOUND_BADGE}
      </span>
    ) : null

  // Commit a typed dial from the shared header readout — routes through the
  // app's setFrequency handler (same path the old global TopBar readout used);
  // rejects out-of-plan frequencies with a toast.
  const commitDial = (mhz: number) => {
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13), so a typed WWV/shortwave/inter-band frequency tunes there. This used to toast
    // "outside the band plan" and discard the entry.
    onSetFrequency(mhz, bandLabelForMhz(mhz), snap.radio.sideband || 'USB')
  }

  // THE CALLSIGN CARD (#168), rendered into the SIDE RAIL of whichever layout is up — one
  // node, two placements, so the two layouts cannot drift into showing different cards.
  //
  // WHY THE RAIL AND NOT A SHELL-LEVEL BLOCK. This cockpit's four-child shell is header /
  // waterfall / QSO strip / `.cockpit-lower`, and the card belongs to none of them: a fifth
  // shell child would sit above `.cockpit-lower`, which is `flex: 1; min-height: 0` and would
  // simply give up the card's height — the pre-overhaul crush, reintroduced. `.cockpit-side`
  // is the one container here that is already an interposed scroller (`overflow-y: auto`,
  // styles.css), which is the same structural reason the CW and Phone log panes can host the
  // full card: it scrolls INSIDE the rail instead of squeezing the cockpit. It also lands
  // where the click did in Classic — the Stations roster is directly beneath it.
  //
  // It costs the rail NOTHING until a call is selected: unselected there is no element at
  // all, so the idle layout is unchanged. Selected, it takes rail height from the roster
  // (which keeps its `flex: 1.8` share of what is left) and the rail scrolls if the pair no
  // longer fits. Two known limits, stated rather than papered over: with the whole side rail
  // ⊞-hidden the card goes with it, and in Roster layout the rail is opposite the roster the
  // click happened in.
  // WHICH station the card is about, and the order is the whole of it: an explicit
  // click WINS over the sequencer. `selectedCall` is the operator saying "show me this
  // one"; `snap.qso.dxcall` is the station the sequencer is actually working, and it
  // fills in when they have not clicked — which is most of an FT8 session, because
  // double-clicking a decode starts a QSO without ever selecting a peer. Reading only
  // the click left the card blank through every contact the operator ran; reading only
  // the QSO would yank it away from a station they had deliberately opened mid-run.
  // Same field the roster highlights as `workingCall`, so the two can never disagree
  // about who is being worked.
  const recallCall = selectedCall || snap.qso?.dxcall || null
  // The decode panes' hide-filter exemption: the station the sequencer is actively working,
  // and nobody after Done — the same "engaged" line the alerts draw.
  const partnerCall = engagedInQso({
    state: snap.fieldDay?.state ?? snap.qso?.state ?? null,
    dxcall: snap.qso?.dxcall ?? null,
  })
    ? (snap.qso?.dxcall ?? null)
    : null
  const recallCard = recallCall ? (
    <OperateRecall snap={snap} call={recallCall} mode={tier} onOpenLog={onOpenLogbook} />
  ) : null

  return (
    <main className="layout single operate-cockpit">
      <CockpitHeader
        snap={snap}
        onSnap={onSnap}
        modeIndicator={
          <div className="cockpit-modes" role="group" aria-label={t('operate.header.modes.aria')}>
            {MODES.map((m) => (
              <button
                key={m.tier}
                type="button"
                className={`cockpit-mode${tier === m.tier ? ' active' : ''}`}
                aria-pressed={tier === m.tier}
                onClick={() => onTierChange(m.tier)}
                title={m.title}
              >
                <span className="cm-name">{m.label}</span>
                <span className="cm-slot">{m.slot}</span>
              </button>
            ))}
            {tier === 'MSK144' && (
              /* WSJT-X shows its T/R spinner on the main window exactly in fast mode
                 (sbTR, mainwindow.cpp:8387) — the period is an operating decision on
                 meteor scatter, not configuration, so it lives here and not only in
                 Settings. Narrow write: a full settings apply is the #54 mid-QSO reset. */
              <select
                className="cockpit-mode cm-trperiod"
                aria-label={t('operate.header.msk144Period.aria')}
                title={t('operate.header.msk144Period.title')}
                value={String(snap.link.periodSecs || 15)}
                onChange={(e) => void setMsk144Period(Number(e.target.value)).then((s2) => onSnap?.(s2))}
              >
                {[5, 10, 15, 30].map((p) => (
                  <option key={p} value={p}>{`${p}s`}</option>
                ))}
              </select>
            )}
          </div>
        }
        bandControl={
          <FrequencyControl
            channels={bandPlan}
            dialMhz={snap.radio.dialMhz}
            band={snap.radio.band}
            mode={snap.radio.sideband}
            variant="compact"
            showReadout={false}
            showModeToggle={false}
            onSet={onSetFrequency}
          />
        }
        onCommitDial={commitDial}
        digitTune
        wheelSensitivity={wheelSensitivity}
        actions={
          <>
            <div
              className="cockpit-decode-depth"
              role="group"
              aria-label={t('operate.header.decodeDepth.aria')}
              title={t('operate.header.decodeDepth.title')}
            >
              {([
                [1, t('operate.header.decodeDepth.fast')],
                [2, t('operate.header.decodeDepth.norm')],
                [3, t('operate.header.decodeDepth.deep')],
              ] as [number, string][]).map(([d, label]) => (
                <button
                  key={d}
                  type="button"
                  className={`cockpit-depth-chip${snap.radio.decodeDepth === d ? ' active' : ''}`}
                  aria-pressed={snap.radio.decodeDepth === d}
                  onClick={() =>
                    void setDecodeDepth(d)
                      .then((s) => onSnap?.(s))
                      .catch(() => {})
                  }
                >
                  {label}
                </button>
              ))}
            </div>
            <PanelsMenu
              items={menuItems}
              onToggle={(id, show) => setPanelState(id as OperatePanelId, show ? 'docked' : 'removed')}
              onUndo={panels.undo}
              canUndo={panels.canUndo}
              onReset={panels.reset}
            />
          </>
        }
        frequencyExtras={
          <TuningStrip
            snap={snap}
            onSnap={onSnap}
            step={tuneStep}
            onStep={setTuneStep}
            showReadout={false}
          />
        }
        power={{
          value: snap.radio.txLevel,
          unit: 'drive',
          onChange: onSetTxLevel,
          // A CONFIGURATION control on the transmit path is not a transmit control — the
          // batch-13 ruling, where the drive slider moved and Prove TX did not.
          label: t('operate.header.power.label'),
          title: t('operate.header.power.title'),
        }}
        txState={false}
      >
        {/* DXpedition special-op selector — one compact select (was a 3-chip group;
            header-density pass 2026-08), always visible in both layouts. Edits
            settings.specialOp. */}
        <div className="cockpit-specialop">
          <span className="cockpit-specialop-label">{t('operate.header.dxped.label')}</span>
          <select
            className="cockpit-specialop-select"
            aria-label={t('operate.header.dxped.aria')}
            value={specialOp === 'superhound' ? 'hound' : specialOp}
            onChange={(e) => handleSpecialOp(e.target.value as NonNullable<Settings['specialOp']>)}
            title={SPECIAL_OPS.find((op) => op.value === (specialOp === 'superhound' ? 'hound' : specialOp))?.title}
          >
            {SPECIAL_OPS.map((op) => (
              <option key={op.value} value={op.value} title={op.title}>
                {op.label}
              </option>
            ))}
          </select>
        </div>

        {/* Warn-only Field Day banned-mode chip (e.g. FT8 at WFD — this cockpit is
            where a banned mode would actually be keyed). A PASSIVE status div in
            the header, outside every ⊞-removable pane: it is a status line, not a
            control, so it carries no panel-vocabulary id and no stop-line role. */}
        <FdAdvisories fdActive={fdActive} ruleset={fdRuleset} activeMode={tier} />

        <div className="cockpit-meta">
          <div
            className="cockpit-source"
            role="group"
            aria-label={t('operate.header.source.aria')}
            // TWO whole sentences: "listening …" is a statement about where we are
            // listening, not a tail glued onto the one before it.
            title={
              source === 'companion'
                ? t('operate.header.source.title.listening', {
                    active: snap.radio.sourceLabel || SOURCE_NATIVE,
                    addr: companionAddr || DEFAULT_COMPANION_ADDR,
                  })
                : t('operate.header.source.title', {
                    active: snap.radio.sourceLabel || SOURCE_NATIVE,
                    addr: companionAddr || DEFAULT_COMPANION_ADDR,
                  })
            }
          >
            <button
              type="button"
              className={`cs-opt${source === 'native' ? ' active' : ''}`}
              aria-pressed={source === 'native'}
              onClick={() => onSourceChange('native')}
              title={t('operate.header.source.native.title')}
            >
              ◉ {SOURCE_NATIVE}
            </button>
            <button
              type="button"
              className={`cs-opt${source === 'companion' ? ' active' : ''}`}
              aria-pressed={source === 'companion'}
              onClick={() => onSourceChange('companion')}
              title={t('operate.header.source.companion.title', {
                addr: companionAddr || DEFAULT_COMPANION_ADDR,
              })}
            >
              ⇄ {SOURCE_COMPANION}
            </button>
          </div>
          {/* The active-source label folded into the source group's tooltip
              (header-density pass 2026-08 — the label alone was ~220px). */}
          {/* DF readouts: type an exact audio offset and commit on Enter/blur
              (clamped to the 200–4000 Hz passband) — WSJT-X's Rx/Tx Hz spinners. */}
          <div className="cockpit-offsets" role="group" aria-label={t('operate.header.offsets.aria')}>
            <DfField label={DF_RX} hz={snap.radio.rxOffsetHz} onCommit={(hz) => onTune(hz, 'rx')} />
            <DfField label={DF_TX} hz={snap.radio.txOffsetHz} onCommit={(hz) => onTune(hz, 'tx')} />
          </div>
          {/* Decode button — re-run the decoder over the last period's audio (F6). */}
          <button
            type="button"
            className="cockpit-decode-btn"
            onClick={handleRedecode}
            // The F6 the button advertises is a media key on default Mac keyboards. The hint
            // itself is FN_KEY_HINT (platform.ts) and migrates with that module.
            title={
              IS_MAC
                ? t('operate.header.decode.title.mac', { hint: FN_KEY_HINT })
                : t('operate.header.decode.title')
            }
          >
            {t('operate.header.decode.label')}
          </button>
          <button
            type="button"
            className={`ph-rec${recording ? ' on' : ''}`}
            onClick={toggleRecord}
            disabled={recBusy}
            aria-label={
              recording
                ? t('operate.header.record.stop.aria')
                : t('operate.header.record.start.aria')
            }
            title={
              recording
                ? t('operate.header.record.stop.title')
                : t('operate.header.record.start.title')
            }
          >
            {recording ? '■' : '●'}
          </button>
          {/* ⭐ A REAL SPLIT CONTROL. This was an annunciator only — it told you split was on
              and gave you no way to set it. Operate is the FT8 cockpit, so it is where a
              DXpedition pile-up is actually worked, and "UP 5" is the ordinary case: the spot
              parser already reads the offset out of the comment and commands it, but an
              operator who tuned to the DX by hand had no control at all. NOT a stop control —
              see SplitControl's header. */}
          {snap.radio.catOk === true && (
            <SplitControl
              snap={snap}
              onSnap={onSnap}
              onError={(m) => pushToast(m, 'error')}
            />
          )}
          <div className="cockpit-layout-toggle" role="group" aria-label={t('operate.header.layout.aria')}>
            <button
              type="button"
              className={`clt-opt${layoutMode === 'classic' ? ' active' : ''}`}
              aria-pressed={layoutMode === 'classic'}
              onClick={() => onLayoutMode('classic')}
              title={t('operate.header.layout.classic.title')}
            >
              {t('operate.header.layout.classic.label')}
            </button>
            <button
              type="button"
              className={`clt-opt${layoutMode === 'roster' ? ' active' : ''}`}
              aria-pressed={layoutMode === 'roster'}
              onClick={() => onLayoutMode('roster')}
              title={t('operate.header.layout.roster.title')}
            >
              {t('operate.header.layout.roster.label')}
            </button>
          </div>
          {/* No MemoryStrip here, deliberately (operator ruling, 2026-08-16). Memories are
              repeaters, nets and calling frequencies — Phone/CW things. In this header a
              favorite chip was worse than clutter: one click retuned the rig off the FT8
              band mid-sequence. Phone and CW keep the strip; the global recall hotkeys are
              unaffected. */}
          <button
            type="button"
            className="cockpit-popout icon"
            onClick={() => openSpot(dxCall || selectedCall || '')}
            aria-label={t('operate.header.spot.aria')}
            title={t('operate.header.spot.title')}
          >
            📢
          </button>
          {onPopOut && (
            <button
              type="button"
              className="cockpit-popout icon"
              onClick={onPopOut}
              aria-label={t('operate.header.popOut.aria')}
              title={t('operate.header.popOut.title')}
            >
              ⧉
            </button>
          )}
        </div>
      </CockpitHeader>

      {/* The old .cockpit-status row is GONE — its TX-state/txNow display duplicated
          the QSO strip's; its protected controls (period, Skip Tx1, next-Ns, HOUND
          badge, rotor) merged INTO the strip below. ~42px of the reported blank gray
          space lived here. */}
      <div className="cockpit-body" ref={bodyRef}>
        {/* Waterfall: a short full-width strip (not a tall column) — the spectrum
            is a glance tool; the real estate goes to the decode lists + roster. */}
        {wfState === 'popped' && (
          <button
            type="button"
            className="wf-redock"
            onClick={redockWaterfall}
            title={t('operate.waterfall.redock.title')}
          >
            {t('operate.waterfall.redock.label')}
          </button>
        )}
        {wfState === 'docked' && (
          <>
            <section className="cockpit-waterfall panel">
              {tier === 'MSK144' ? (
                /* MSK144 is a TIME display, not a frequency one — every signal sits at
                 * 1500 Hz and lives for milliseconds, so the waterfall shows one unmoving
                 * stripe while the actual event (the ping) is invisible. WSJT-X hides its
                 * waterfall here and shows the Fast Graph; so does Nexus. Same strip, same
                 * splitter — only the picture changes. */
                <FastGraph
                  periodS={snap.link.periodSecs || 15}
                  decodes={snap.recentDecodes?.filter((d) => d.tier === 'MSK144')}
                  theme={theme}
                />
              ) : (
                <Waterfall
                  onPopOut={popOutWaterfall}
                  transmitting={snap.radio.transmitting}
                  rxOffsetHz={snap.radio.rxOffsetHz}
                  txOffsetHz={snap.radio.txOffsetHz}
                  theme={theme}
                  onTune={onTune}
                  active={active}
                  paletteScope={FT_PALETTE_SCOPE}
                  // An FT over is 13 s: the dark band reads as "that was us", and there is
                  // genuinely no receiver to picture during it. Explicit here because the
                  // default is OFF — the shared Waterfall also draws RTTY's and SSTV's band,
                  // where an over runs minutes and a black band reads as a dead display.
                  txBlanks
                />
              )}
            </section>
            <Splitter
              axis="y"
              varName="--cockpit-wf-h"
              target={bodyRef}
              storageKey="nexus.split.operate.waterfall"
              min={88}
              max={420}
              defaultPct={22}
              label={t('operate.waterfall.splitter.label')}
            />
          </>
        )}

        {/* THE merged operating strip (one row at lg/xl): state cap + sequencer +
            Call CQ / S&P + the TX cluster + now-sending + Tx5 free text, PLUS the
            controls absorbed from the deleted status row (period, Skip Tx1, next-Ns,
            HOUND badge, rotor) and the fixed-width TX telemetry cell. */}
        <OperateQsoStrip
          qso={snap.qso}
          radio={snap.radio}
          rxOnly={isRxOnly(tier)}
          beacon={isBeacon(tier)}
          onSetTxEnabled={onSetTxEnabled}
          onSetTune={onSetTune}
          onAtuTune={() =>
            // A control that keys the transmitter must never fail silently: the backend
            // returns every refusal (TX off, busy, lockout) WITH ITS REASON — show it.
            void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
          onHaltTx={onHaltTx}
          onSetHoldTxFreq={onSetHoldTxFreq}
          onSetMode={onSetMode}
          onCallCq={() => {
            // The labelled "Call CQ" is always a PLAIN run — it also clears a
            // sticky directed token so a leftover "CQ DX" can't surprise.
            void startCq(null).then((s) => onSnap?.(s)).catch(() => {})
          }}
          onResend={onResend}
          onFreetext={onFreetext}
          onLog={onLog}
          onSetTxEven={onSetTxEven}
          onSetTxCycleAuto={onSetTxCycleAuto}
          skipTx1={skipTx1}
          onSkipTx1={handleSkipTx1}
          nextSlotSec={nextSlotSec}
          specialOpBadge={specialOpBadge}
          rotor={
            <RotorStrip
              active={active}
              onOpenSettings={onOpenSettings}
              targetCall={selectedCall}
              onPointAt={(call) =>
                pointRotatorAtCall(call)
                  .then((bearing) =>
                    pushToast(t('operate.rotor.pointed', { call, deg: Math.round(bearing) }), 'info'),
                  )
                  // `{{error}}` is the backend's own refusal, passed through as a value.
                  .catch((e) =>
                    pushToast(
                      t('operate.rotor.failed', {
                        error: e instanceof Error ? e.message : String(e),
                      }),
                      'error',
                    ),
                  )
              }
            />
          }
          // Transmit meters (SWR / ALC / Po / COMP): the old PERMANENT body row is
          // gone (the operator's "eating up a whole line" complaint) — the same
          // pinned semantics (live while keyed, last readings dimmed between overs)
          // now render into the strip's fixed-width telemetry cell, so the FT TX
          // cycle still cannot bounce the layout. Still a ⊞ Panels entry.
          telemetry={shown('txmeters') ? <TxMeters radio={snap.radio} inline /> : undefined}
        />

        {/* data-cols collapses the grid to ONE column once the main cell or the whole
            side rail is gone — otherwise the survivor keeps its 2fr/1fr track and the
            removed panel's space is never actually reclaimed. */}
        <div
          className={`cockpit-lower ${layoutMode}`}
          data-cols={dataCols}
          ref={layoutMode === 'classic' ? lowerClassicRef : undefined}
          style={layoutMode === 'classic' ? classicColStyle() : undefined}
        >
          {layoutMode === 'roster' ? (
            <>
              {/* Roster layout (GridTracker-style): the full sortable Call Roster is
                  the centerpiece; Band Activity + Rx Frequency move to a side rail. */}
              {shown('callRoster') && (
                <div className="cockpit-roster-main panel">
                  <OperateRoster
                    stations={snap.stations}
                    myGrid={snap.mygrid}
                    currentSlot={snap.radio.slot}
                    needByCall={needByCall}
                    needAlertsByCall={needAlertsByCall}
                    // Scope the need chips to what this cockpit can actually close, so a
                    // cross-band or cross-mode alert for the same callsign can't claim a
                    // need here (a CW "new mode" on a 30m FT8 roster).
                    band={snap.radio.band}
                    feedMode={tier}
                    selectedCall={selectedCall}
                    // The station the sequencer is working, straight off the QSO status. NOT
                    // `selectedCall` — App binds that to `activePeer`, which is the Tempo CHAT
                    // peer and is null throughout an FT8 session, so the roster had nothing to
                    // highlight and #16 was reported.
                    workingCall={snap.qso?.dxcall ?? null}
                    onSelect={onSelect}
                    onCall={onCall}
                    ignoredCalls={ignored}
                    onToggleIgnore={handleToggleIgnore}
                    // Open the reviewable Spot popup pre-filled with this station (posting to a public
                    // cluster deserves a glance before it goes out); the dialog seeds the dial + a mode
                    // comment itself.
                    onSpot={(call) => openSpot(call)}
                  />
                </div>
              )}
              {sideShown && (
                <aside className="cockpit-side">
                  {recallCard}
                  {shown('bandActivity') && (
                    <div className="cockpit-decodes-side panel" ref={decodesSideRef} style={shareStyle('bandActivity')}>
                      {/* The FULL decode window (filters + sort), not the compact
                          strip — roster mode = decode window + roster on one page
                          (operator request); only Rx Frequency stays compact. */}
                      <OperateDecodes
                        history={bandHistRef.current}
                        decodes={snap.recentDecodes}
                        slot={snap.radio.slot}
                        rxOffsetHz={snap.radio.rxOffsetHz}
                        band={snap.radio.band}
                        tier={tier}
                        harqRescues={snap.harqRescues}
                        onCall={onCall}
                        needAlertsByCall={needAlertsByCall}
                        needScopes={needScopes}
                        myGrid={snap.mygrid}
                        {...decodeClickProps}
                    partnerCall={partnerCall}
                        onErase={() => notifyErase(0)}
                        title={t('operate.decodes.title')}
                      />
                    </div>
                  )}
                  {shown('bandActivity') && shown('rxfreq') && (
                    <SplitterSeam
                      above={decodesSideRef}
                      below={rxfreqRef}
                      varName="--pane-share"
                      onCommit={(av, bv) => panels.setShares({ bandActivity: av, rxfreq: bv })}
                      label={t('operate.seam.bandActivityRxFreq.label')}
                    />
                  )}
                  {shown('rxfreq') && (
                    <div className="cockpit-rxfreq panel" ref={rxfreqRef} style={shareStyle('rxfreq')}>
                      <OperateDecodes
                        history={rxHistRef.current}
                        decodes={snap.recentDecodes}
                        slot={snap.radio.slot}
                        rxOffsetHz={snap.radio.rxOffsetHz}
                        band={snap.radio.band}
                        tier={tier}
                        harqRescues={snap.harqRescues}
                        onCall={onCall}
                        needAlertsByCall={needAlertsByCall}
                        needScopes={needScopes}
                        myGrid={snap.mygrid}
                        {...decodeClickProps}
                    partnerCall={partnerCall}
                        onErase={() => notifyErase(1)}
                        lockedFilter="rx"
                        // This pane is situational awareness, not a chase list: a station
                        // from an excluded country sitting on OUR frequency is exactly what
                        // we need to see, so the exclusion stops at Band Activity.
                        hideExcludedCountries={false}
                        compact
                        title={t('operate.decodes.rxFreq.title', {
                          hz: Math.round(snap.radio.rxOffsetHz),
                        })}
                      />
                    </div>
                  )}
                </aside>
              )}
            </>
          ) : (
            <>
              {/* Classic layout — DECODE-FIRST (2026-08 field feedback from an advanced
                  DX operator): a three-column grid whose first two columns are the
                  WSJT-X pair. Band Activity (col 1) beside the PROMOTED full-height
                  Rx-Frequency pane — the QSO stream with the operator's own TX lines
                  interleaved — with the compact Tx1–Tx6 machine docked beneath it
                  (col 2, WSJT-X's bottom-right geometry), and the Stations roster as
                  col 3. Click a call in either pane, watch the exchange line-by-line
                  in col 2, next TX visible right below. */}
              {shown('bandActivity') && (
                <div className="cockpit-decodes panel">
                  <OperateDecodes
                    history={bandHistRef.current}
                    decodes={snap.recentDecodes}
                    slot={snap.radio.slot}
                    rxOffsetHz={snap.radio.rxOffsetHz}
                    band={snap.radio.band}
                    tier={tier}
                    harqRescues={snap.harqRescues}
                    onCall={onCall}
                    needAlertsByCall={needAlertsByCall}
                    needScopes={needScopes}
                    myGrid={snap.mygrid}
                    {...decodeClickProps}
                    partnerCall={partnerCall}
                    onErase={() => notifyErase(0)}
                  />
                </div>
              )}
              {(shown('rxfreq') || shown('txmsgs')) && (
                <div className="cockpit-qsocol" ref={qsocolRef}>
                  {shown('rxfreq') && (
                    <div className="cockpit-rxfreq panel">
                      {/* Identical decodeClickProps spread as Band Activity — click
                          parity is free and STAYS free because the props are shared,
                          never forked (guarded by OperateCockpit.structure.test). */}
                      <OperateDecodes
                        history={rxHistRef.current}
                        decodes={snap.recentDecodes}
                        slot={snap.radio.slot}
                        rxOffsetHz={snap.radio.rxOffsetHz}
                        band={snap.radio.band}
                        tier={tier}
                        harqRescues={snap.harqRescues}
                        onCall={onCall}
                        needAlertsByCall={needAlertsByCall}
                        needScopes={needScopes}
                        myGrid={snap.mygrid}
                        {...decodeClickProps}
                    partnerCall={partnerCall}
                        onErase={() => notifyErase(1)}
                        lockedFilter="rx"
                        // This pane is situational awareness, not a chase list: a station
                        // from an excluded country sitting on OUR frequency is exactly what
                        // we need to see, so the exclusion stops at Band Activity.
                        hideExcludedCountries={false}
                        compact
                        title={t('operate.decodes.rxFreq.title', {
                          hz: Math.round(snap.radio.rxOffsetHz),
                        })}
                      />
                    </div>
                  )}
                  {shown('txmsgs') && (
                    <TxPanel
                      compact
                      dxCall={dxCall}
                      dxGrid={dxGrid}
                      onDxCall={setDxCall}
                      onDxGrid={setDxGrid}
                      messages={msgs}
                      tx5={tx5}
                      onTx5={handleTx5}
                      tx6={tx6}
                      onTx6={handleTx6}
                      nextIndex={nextIndex}
                      onTx={doTx}
                      onGenerate={handleGenerate}
                      onClear={clearDx}
                      qsoMacros={qsoMacros}
                    />
                  )}
                  {/* Column seam: lets the operator tune the pair/roster balance by
                      drag. Not a grid CHILD (that would burn a track/cell) and not
                      inside the aside (whose overflow clips): it rides the qsocol's
                      right edge, overlaying the grid gap, via absolute CSS. The drag
                      paints --op-col-a/-b fr tokens on the grid container — which
                      ONLY the three-column template consumes, so the seam renders
                      only at data-cols='three' (at 'two' the drag would be visually
                      dead while still rewriting the persisted shares). */}
                  {dataCols === 'three' && (
                    <SplitterSeam
                      above={qsocolRef}
                      below={classicSideRef}
                      axis="x"
                      columnsOn={lowerClassicRef}
                      varName="--op-col"
                      onCommit={(av, bv) => panels.setShares({ txmsgs: av, stations: bv })}
                      label={t('operate.seam.qsocolStations.label')}
                    />
                  )}
                </div>
              )}
              {sideShown && (
                <aside className="cockpit-side" ref={classicSideRef}>
                  {recallCard}
                  {shown('stations') && <div className="cockpit-roster panel">{roster}</div>}
                </aside>
              )}
            </>
          )}
        </div>
      </div>
      <SpotDialog
        open={spotOpen}
        onClose={() => setSpotOpen(false)}
        initialCall={spotSeedCall}
        freqMhz={snap.radio.dialMhz}
        defaultComment={String(snap.link.tier).toUpperCase()}
      />
    </main>
  )
}

/**
 * THE CALLSIGN CARD FOR THE FT COCKPIT (#168 — "the details card is missing here").
 *
 * The card is `RecallPanel`, the same one the CW and Phone log strips have shown since
 * 2026-07-31, and none of what it shows is new: the prior-contact list, the confirmed
 * count, the DXCC/band/mode slot flags, the distance/bearing line and the private note all
 * already worked. What was missing was a PATH to it from this cockpit. `RecallPanel` had
 * exactly one caller — LogEntry — and Operate hosts no LogEntry, so clicking a roster row
 * here only ever armed the Spot button.
 *
 * This is therefore the assembly LogEntry does inline, with the log FORM left out: read the
 * logbook, resolve the award entity, ask the callbook, hand the result over. Three
 * deliberate differences from LogEntry, each because this cockpit is not a log strip:
 *
 *   · THE LOGBOOK IS RE-READ ON EVERY SELECTION. LogEntry reads once per mount and again
 *     after it logs, which is complete for a strip that is the only thing writing. Operate
 *     logs in the BACKGROUND — the sequencer files a contact the moment the exchange
 *     completes, with no click — so a once-per-mount read would show a stale "previous
 *     contacts" list for the rest of the session, and would tell an operator they had never
 *     worked a station they worked ten minutes ago. A roster click is an operator action,
 *     not a poll.
 *   · THE DECODED GRID SEEDS THE CARD. The station is on screen because we decoded it, and
 *     an FT8 frame carries a square. Using it when the callbook has none is what puts a
 *     distance and a bearing on the card for an operator with no QRZ subscription at all —
 *     the callbook's own square still wins when there is one, being the finer of the two.
 *   · THE LOOKUP IS SILENT AND DEBOUNCED. No credentials, no subscription, a call the
 *     callbook does not know: all of those are ordinary here, and none may put a toast on
 *     screen for a roster click. The card degrades to identity + history + badges, exactly
 *     as the CW cockpit's does today.
 */
function OperateRecall({
  snap,
  call,
  mode,
  onOpenLog,
}: {
  snap: AppSnapshot
  call: string
  mode: string
  onOpenLog?: (call: string) => void
}) {
  const cu = call.trim().toUpperCase()
  const [log, setLog] = useState<LoggedQso[]>([])
  const [book, setBook] = useState<QrzLookup | null>(null)
  const [entity, setEntity] = useState<string | null>(null)

  useEffect(() => {
    let stale = false
    void getLog()
      .then((l) => {
        if (!stale) setLog(l)
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [cu])

  // The award identity comes from cty.dat via the CALL — never the callbook's country
  // string, which spells entities differently enough ("Germany" vs "Fed. Rep. of Germany")
  // that NEW ONE fired forever on every contact with one. LogEntry's ruling, same reason.
  useEffect(() => {
    let stale = false
    void resolveEntity(cu)
      .then((e) => {
        if (!stale) setEntity(e)
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [cu])

  useEffect(() => {
    setBook(null)
    if (cu.length < 3) return
    let stale = false
    // Debounced: arrowing down the roster walks through calls, and each one must not spend
    // a callbook request on a station the operator did not stop at.
    const id = setTimeout(() => {
      void qrzLookup(cu)
        .then((r) => {
          if (!stale) setBook(r)
        })
        .catch(() => {})
    }, 400)
    return () => {
      stale = true
      clearTimeout(id)
    }
  }, [cu])

  const station = snap.stations.find((s) => s.call.trim().toUpperCase() === cu) ?? null
  const hist = useMemo(
    () => callHistory(log, cu, snap.radio.band, mode, snap.b4MatchMode ?? false),
    [log, cu, snap.radio.band, mode, snap.b4MatchMode],
  )
  // The roster's country is cty.dat-resolved from the call, like `resolveEntity` — so it is
  // the right thing to stand in with while that request is in flight, and the badges do not
  // flicker through "new one" on the way to the truth.
  const entityForBadge = entity ?? station?.country ?? book?.country ?? null
  const newEntity = useMemo(() => isNewEntity(log, entityForBadge), [log, entityForBadge])
  const slots = useMemo(() => entitySlots(log, entityForBadge), [log, entityForBadge])
  const liveBand = bandKey({ band: snap.radio.band, freqMhz: snap.radio.dialMhz })
  const newBandSlot =
    slots.workedEver &&
    !slots.bandUnknown &&
    liveBand !== null &&
    !slots.bandsWorked.includes(liveBand)
  const newModeSlot =
    slots.workedEver && !newBandSlot && !slots.modesWorked.includes(modeKey(mode))

  return (
    <RecallPanel
      call={cu}
      band={snap.radio.band}
      // The operator's QRZ nickname over their full name, for the same reason the log strip
      // prefers it: it is what they answer to on the air.
      name={book?.nickname || book?.name}
      qth={book?.qth}
      grid={book?.grid || station?.grid}
      lat={book?.lat ?? null}
      lon={book?.lon ?? null}
      country={book?.country}
      image={book?.image}
      myGrid={snap.mygrid}
      hist={hist}
      newEntity={newEntity}
      newBandSlot={newBandSlot}
      newModeSlot={newModeSlot}
      // No Lookup button in this cockpit — the card must not tell the operator to press one.
      hasLookup={false}
      // The rail is SHARED with the Stations roster; unbounded this card took it down to
      // ~2 rows at 1024x768 and off-screen at 175 % zoom. See `.cockpit-recall`.
      bounded
      onOpenLog={onOpenLog}
    />
  )
}

/**
 * A compact labeled DF (audio offset) entry: tracks the snapshot value while
 * idle, commits on Enter/blur (rounded + clamped 200–4000 Hz), and reverts on
 * garbage input. Enter just blurs — the single commit happens in onBlur.
 */
function DfField({
  label,
  hz,
  onCommit,
}: {
  label: string
  hz: number
  onCommit: (hz: number) => void
}) {
  const [text, setText] = useState(() => String(Math.round(hz)))
  const [editing, setEditing] = useState(false)
  const editingRef = useRef(editing)
  editingRef.current = editing
  // Track the live prop ONLY when it actually changes — keying the effect on
  // `editing` too made the blur's clamped value flash back to the stale prop
  // for the IPC round-trip (commit sets text + editing in the same render).
  useEffect(() => {
    if (!editingRef.current) setText(String(Math.round(hz)))
  }, [hz])
  const commit = () => {
    setEditing(false)
    const n = Number(text)
    if (text.trim() !== '' && Number.isFinite(n)) {
      const clamped = clampOffsetHz(n)
      setText(String(clamped))
      if (clamped !== Math.round(hz)) onCommit(clamped)
    } else {
      setText(String(Math.round(hz))) // revert garbage
    }
  }
  return (
    <label className="df-field" title={t('operate.header.df.title', { label })}>
      <span className="df-label">{label}</span>
      <input
        type="number"
        inputMode="numeric"
        min={200}
        max={4000}
        step={1}
        value={text}
        aria-label={t('operate.header.df.aria', { label })}
        onFocus={() => setEditing(true)}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            ;(e.target as HTMLInputElement).blur()
          }
        }}
      />
      <span className="df-unit">{HZ_UNIT}</span>
    </label>
  )
}

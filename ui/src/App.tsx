import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, LoggedQso, ModeRequest, Settings, SourceKind, Tier } from './types'
import {
  callStation as apiCallStation,
  overrideNextTx as apiOverrideNextTx,
  setArea as apiSetArea,
  openPanelWindow,
  qsoResend as apiQsoResend,
  qsoFreetext as apiQsoFreetext,
  logCurrentQso as apiLogCurrentQso,
  confirmPendingLog as apiConfirmPendingLog,
  discardPendingLog as apiDiscardPendingLog,
  getBandPlan,
  getSettings,
  logOperators,
  getSnapshot,
  selectPeer as apiSelectPeer,
  archiveConversation as apiArchiveConversation,
  sendMessage as apiSendMessage,
  broadcast as apiBroadcast,
  setChatCq as apiSetChatCq,
  resumeChatCq as apiResumeChatCq,
  setFrequency as apiSetFrequency,
  sstvTune as apiSstvTune,
  setBlockedCalls as apiSetBlockedCalls,
  tuneChannel as apiTuneChannel,
  aprsTune,
  setMode as apiSetMode,
  setTier as apiSetTier,
  setSource as apiSetSource,
  setTxEnabled as apiSetTxEnabled,
  setTxLevel as apiSetTxLevel,
  setActiveRadio as apiSetActiveRadio,
  setPegLock as apiSetPegLock,
  setTune as apiSetTune,
  haltTx as apiHaltTx,
  setTxEven as apiSetTxEven,
  setTxCycleAuto as apiSetTxCycleAuto,
  setBeacon as apiSetBeacon,
  setRxOffset as apiSetRxOffset,
  setTxOffset as apiSetTxOffset,
  setHoldTxFreq as apiSetHoldTxFreq,
  subscribeSnapshot,
} from './api'
import { withErrorToast, pushToast } from './toast'
import { setUnitsMirror } from './units'
import { doubleBeep, processDecodes, txEarcon } from './alerts'
import { openingToastSpec } from './openingAlert'
import { announce } from './announce'
import { Announcer } from './components/Announcer'
import { ErrorBoundary } from './components/ErrorBoundary'
import { loadWatchlist, type WatchFilter } from './watchlist'
import { useTheme } from './useTheme'
import { useFieldMode } from './useFieldMode'
import { useScale } from './useScale'
import { useViewport } from './useViewport'
import { useDensity } from './useDensity'
import { useMotion } from './useMotion'
import { useBandEdgeTones } from './useBandEdgeTones'
import { useAchievements } from './useAchievements'
import { useJourneyUnlocks } from './useJourneyUnlocks'
import { useFeatures } from './useFeatures'
import { useReveals } from './useReveals'
import { sectionFeatures, featureById, type FeatureId } from './features/registry'
import { resolveBootView, coerceArea } from './features/bootView'
import { visibleNeeds, workTarget, modeClassOf, topNeedByCall, alertsByCall, activityTypeByCall } from './features/needs'
import { OPERATE_PANELS, CW_PANELS, PHONE_PANELS, RTTY_PANELS, SSTV_PANELS, usePanelLayout } from './features/panelState'
import { surfaceGet, surfaceSet } from './features/windowScope'
import { usePaneWidths, clampLeft, clampRight } from './usePaneWidths'
import { TopBar } from './components/TopBar'
import { StationList } from './components/StationList'
import { Conversation } from './components/Conversation'
import { TempoHeader } from './components/TempoHeader'
import { Waterfall } from './components/Waterfall'
import { FT_PALETTE_SCOPE } from './waterfallPalette'
import { LinkPill } from './components/LinkPill'
import { ModeNav, type View, type DigitalMode } from './components/ModeNav'
import { OperateCockpit } from './components/OperateCockpit'
import { NowBar } from './components/NowBar'
import { AwardsJourney } from './components/AwardsJourney'
import { CwCockpit } from './components/CwCockpit'
import { PhoneCockpit } from './components/PhoneCockpit'
import { RttyCockpit } from './components/RttyCockpit'
import { SstvView } from './components/SstvView'
import { AprsCockpit } from './components/AprsCockpit'
import { PotaSotaView, type OtaSpotClickArg } from './components/PotaSotaView'
import { StatsView } from './components/StatsView'
import { DxpeditionsView } from './components/DxpeditionsView'
import { SatellitesView } from './components/SatellitesView'
import { RadioProgView } from './components/RadioProgView'
import { MemoriesView } from './components/MemoriesView'
import { ConnectView } from './components/ConnectView'
import {
  getPropagation,
  getFeedHealth,
  getNeedAlerts,
  getAllSpots,
  getXrayNow,
  getDxpedWindows,
  getSatSchedule,
  getSatTrackStatus,
  getIssPass,
  getTleStatus,
  sstvArm,
  sstvAutoDisarm,
  setSettings as apiSetSettings,
  setFdOperator,
  setSidebandOverride,
  testCat,
  setOperatingMode,
  workSpot,
  setLicenseClass,
  stopQsoRecording,
  pointRotatorAtCall,
  qsySetEnabled as apiQsySetEnabled,
  radioLaunchInfo,
  chooseRadio,
  useSingleRadio,
  resendChat,
  type RadioLaunchInfo,
} from './api'
import {
  hotkeyRecallTarget,
  markRecalled,
  memoriesStore,
  planRecall,
  type Memory,
} from './features/memories'
import { dueNetReminders, reminderKey, untilPhrase } from './features/nets'
import { bandLabelForMhz } from './band'
import { processFlare, effectiveXray } from './flareAlert'
import { processDxpedAlerts } from './features/dxpedChase'
import { checkDxpedAlarms } from './features/dxpedAlarm'
import { checkSatAlarms, satAlarmMap } from './features/satAlarm'
import { tickSatPassAlert } from './features/satPassAlert'
import { tickIssAutoArm } from './features/issAutoArm'
import { satElementsLane } from './features/satLane'
import { dxpedWorkMode } from './components/connect/paneFormat'
import { setStatus } from './status'
import type { PropagationSnapshot, FeedHealth, NeedAlert, SpotRow, DxpedWindow, WorkableCard, CatTestResult } from './types'
import { NeededPanel } from './components/NeededPanel'
import { SpotsPanel } from './components/SpotsPanel'
import { LogConfirm } from './components/LogConfirm'
import { FieldDayView } from './components/FieldDayView'
import { OperateDecodes } from './components/OperateDecodes'
import { Logbook } from './components/Logbook'
import { RoamPanel } from './components/RoamPanel'
import { SettingsPanel } from './components/SettingsPanel'
import { Toasts } from './components/Toasts'
import { OnboardingBanner } from './components/OnboardingBanner'
import { PounceBanner } from './components/PounceBanner'
import { UpdateBanner } from './components/UpdateBanner'
import { useSelfUpdate } from './useSelfUpdate'
import { usePounce, type PounceAlert } from './usePounce'
import { RevealNudge } from './components/RevealNudge'
import { SetupWizard, type WizardDraft } from './components/SetupWizard'
import { GettingStartedGuide } from './components/GettingStartedGuide'
import { RadioPicker } from './components/RadioPicker'
import { PROFILES, type ProfileId } from './features/profiles'
import { maybeCheckForUpdate } from './features/updateCheck'

const ONBOARD_KEY = 'tempo-onboarded'
// First-run setup wizard: shown once on a fresh install, re-openable from Settings.
const WIZARD_KEY = 'nexus.features.wizardSeen'
function wizardSeen(): boolean {
  try {
    return localStorage.getItem(WIZARD_KEY) === '1'
  } catch {
    return true // storage blocked → don't nag
  }
}
function markWizardSeen(): void {
  try {
    localStorage.setItem(WIZARD_KEY, '1')
  } catch {
    /* storage blocked — wizard simply won't persist as seen */
  }
}
// Read-only storage (e.g. Safari private mode): writes throw while reads succeed,
// so "seen" can't persist. Suppress the wizard there so it can't re-nag on every
// reload — it'd be shown forever otherwise.
function storageWritable(): boolean {
  try {
    const k = 'nexus.__probe'
    localStorage.setItem(k, '1')
    localStorage.removeItem(k)
    return true
  } catch {
    return false
  }
}

// Macros fall back to these until settings load (keeps chips populated).
const DEFAULT_MACROS: Settings['macros'] = {
  // 'chat' chips are DIRECTED replies to the selected peer. (CQ moved to 'band':
  // calling CQ is a broadcast, not a message to one station.)
  chat: ['73', 'QSL', 'Name?', 'QTH?', 'GE'],
  qso: ['R-09', 'RRR', 'RR73', '73'],
  // 'band' chips are open free-text BROADCASTS. A CQ goes through the structured
  // Call-CQ button (not a free-text chip, which chunked into a gridless "A12CQ").
  band: ['QRZ?', 'PSE K', '73', 'GL'],
}

/** The tiers the Operate (Digital) cockpit owns — FT8/FT4 plus the six
 *  receive-only WSJT-X modes. Kept beside the picker in OperateCockpit's MODES
 *  so a tier added there and forgotten here cannot be silently coerced back to
 *  FT8, which is exactly what happened when Q65 and friends were first wired. */
const OPERATE_TIERS: Tier[] = [
  'FT8',
  'FT4',
  'Q65',
  'MSK144',
  'FST4',
  'FST4W',
  'JT65',
  'WSPR',
]

export default function App() {
  const [theme, setTheme] = useTheme()
  // Field mode (outdoor/POTA): high contrast via data-contrast on <html>, larger auto-fit via
  // the useScale argument. Global — a fact about the station, like the theme.
  const [fieldMode, setFieldMode] = useFieldMode()
  const { scale, mode: scaleMode, cap: scaleCap, setMode: setScaleMode, setCap: setScaleCap } = useScale(fieldMode)
  // Publishes the zoom-aware `data-viewport` size class on <html> (live on resize
  // AND on scale change) so the layout adapts to the EFFECTIVE width.
  useViewport(scale)
  // Density (row heights / padding). Comfortable ↔ Compact toggle lives in Settings.
  const [density, setDensity] = useDensity()
  useMotion()
  // Modular features (toggles + profiles). Drives nav, view-gating, and the
  // gamification/achievements layer.
  const features = useFeatures()
  useAchievements(features.isOn('gamification'))
  useJourneyUnlocks(features.isOn('gamification'))
  const reveal = useReveals(features)
  // First-run setup wizard (goal-driven). Only on a genuinely fresh install.
  const [showWizard, setShowWizard] = useState<boolean>(
    () => features.firstRun && storageWritable() && !wizardSeen(),
  )
  // Getting started guide — Help ▸ Getting started, and the wizard's optional
  // walkthrough offer. Pure documentation: it writes nothing and is never
  // shown unasked, so there is no "seen" flag to persist.
  const [showGuide, setShowGuide] = useState(false)
  // `scale` so the rail clamps re-run on zoom change (ceilings are zoom-relative).
  const { commitLeft, commitRight, resetWidths } = usePaneWidths(scale)
  const layoutRef = useRef<HTMLElement>(null)
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  // Two-radio launch picker: shown only when simultaneous-radios is on, ≥2 radios are configured,
  // and this window launched without a profile. `null` = not a picker launch (the common case),
  // so a single-radio station never sees any of this.
  const [radioPicker, setRadioPicker] = useState<RadioLaunchInfo | null>(null)
  useEffect(() => {
    let live = true
    radioLaunchInfo()
      .then((i) => {
        if (live && i.showPicker) setRadioPicker(i)
      })
      .catch(() => {}) // no picker on error — fall through to normal operation
    return () => {
      live = false
    }
  }, [])
  // Live mirror of our callsign for callbacks with empty dep lists (handleCall
  // guards against working yourself without re-creating the callback per snap).
  const mycallRef = useRef('')
  useEffect(() => {
    mycallRef.current = snap?.mycall ?? ''
  }, [snap?.mycall])
  // Live mirror of the snapshot for the app-wide 30 s poll effect (empty deps —
  // it can't read `snap` directly). The ISS SSTV auto-arm reads the current dial.
  const snapRef = useRef<AppSnapshot | null>(null)
  useEffect(() => {
    snapRef.current = snap
  }, [snap])
  // Click-to-work handoff: a Needed-board click on a voice/CW spot seeds this, the
  // matching cockpit consumes it to prefill the log. `ts` makes a re-click of the same
  // call refire the cockpit's prefill effect. Cleared once consumed.
  const [pendingWork, setPendingWork] = useState<{
    call: string
    view: 'cw' | 'phone'
    ts: number
  } | null>(null)
  // Roam settings panel (inside the Tempo cockpit) open/closed.
  const [roamOpen, setRoamOpen] = useState(false)
  const [view, setView] = useState<View>(() => {
    // Deeplink > legacy merged-section deeplink > persisted view > profile landing —
    // the precedence and the clamp live in resolveBootView (pure, test-pinned): a
    // deeplink or restored view is honored only if it is an enabled section of THIS
    // build, so an id removed in an update falls back to the landing instead of
    // opening a dead view (SF ticket #3; re-pinned after the 0.24.6 black screen).
    //
    // SAFETY: restoring the VIEW must not command the radio. That is guaranteed by the mount
    // guard on the rig-mode effect below — without it, restoring to CW/Phone/RTTY would both
    // reconfigure the rig and ARM TRANSMIT at launch (engine.rs set_operating_mode arms TX for
    // the manual modes). Do not remove that guard.
    // PER-SURFACE: "reopen where I left off" describes THIS window.
    const h = typeof window !== 'undefined' ? window.location.hash.slice(1) : ''
    return resolveBootView(
      h,
      surfaceGet('nexus.view'),
      sectionFeatures().map((f) => f.id),
      (id) => features.enabled[id as FeatureId] !== false,
      features.landing,
    )
  })
  // Bird handed off from a map satellite click — the Satellites section opens on it.
  const [satFocus, setSatFocus] = useState<string | null>(null)
  // Bumped when the wizard closes: remounts SettingsPanel so a stale full-struct
  // form under the modal can't Save over what the wizard just persisted.
  const [wizardGen, setWizardGen] = useState(0)
  // Per-section rig-mode policy. Only ENTERING an actual operating cockpit changes the rig:
  // the workspace sections (FT8/FT4, Tempo, contest…) + the global CW/Phone cockpits. A
  // global, non-operating view (Map, Logbook, Settings, Propagation, Awards…) leaves the rig
  // exactly as the last operating section set it — glancing at the map mid-CW-QSO must never
  // touch the VFO or mode, and (crucially) must not advance the guard, so a later Operate
  // click still QSYs. `followFreq` is true only for the three explicit mode tabs (Phone / CW /
  // Digital-Operate) — entering one drops the rig to that mode's home freq; the other digital
  // cockpits (chat/qso/…) set the mode only and keep their own band picker's frequency.
  const lastOpModeRef = useRef<'digital' | 'phone' | 'cw' | 'rtty'>('digital')
  // Guard refs for the rig-mode effect below. `opModeSeeded` is set once, by whichever comes
  // first: the persisted-operating-mode seed (that effect lives further down, where `settings`
  // is in scope) or a genuine view change — an operator's click outranks any seed, because the
  // click IS the intent.
  const opModeSeeded = useRef(false)
  const didMountOpMode = useRef(false)

  // Remember the section for next launch (read back in the `view` initializer above). One
  // effect rather than 16 `setView` call sites — every navigation path, present and future,
  // is covered by construction. Writing the restored value again on mount is a harmless no-op.
  useEffect(() => {
    surfaceSet('nexus.view', view)
  }, [view])

  useEffect(() => {
    // ── LAUNCH IS A READ-ONLY ACT ──────────────────────────────────────────────────
    // This effect also runs on mount, where `view` is merely the restored/landing section —
    // NOT a statement of operator intent. Letting it fire commanded the rig into DATA at every
    // launch and PERSISTED that over the operator's real saved mode (set_operating_mode saves
    // settings), so a station parked on 40m LSB for a net came up in DATA-L, unrecoverable by
    // relaunching. It would also ARM TRANSMIT, since set_operating_mode arms TX for the manual
    // modes (engine.rs). SF ticket #3.
    //
    // This is a MOUNT guard, NOT the same-value guard the comment below rightly forbids —
    // every subsequent view change still re-asserts unconditionally.
    if (!didMountOpMode.current) {
      didMountOpMode.current = true
      return
    }
    opModeSeeded.current = true // an explicit view change outranks the booted-mode seed
    // ⚠️ AN EXPLICIT MAP, NOT A FALLTHROUGH. This used to be
    //   `featureById(view)?.workspace || view === 'cw' | 'phone' | 'rtty'`, then
    //   `… : 'digital'` for everything else — so ANY view carrying a `workspace` that was not
    //   one of the three named modes silently commanded the rig into DATA. POTA/SOTA declares
    //   `workspace: 'dx'` for LAYOUT reasons and is not a mode at all: it is a hunting board,
    //   and you work a park on whatever the activator is running. Opening it flipped an FT-991A
    //   from USB to D-U with no operator action and no way to tell why (#80).
    //   Listing the views that OWN a rig mode makes that class of bug unrepresentable: a new
    //   workspace view now asserts NOTHING until someone deliberately adds it here.
    const RIG_MODE_BY_VIEW: Partial<Record<string, 'cw' | 'phone' | 'rtty' | 'digital'>> = {
      cw: 'cw',
      phone: 'phone',
      rtty: 'rtty',
      operate: 'digital', // the FT8/FT4 cockpit
      chat: 'digital', // Tempo is a digital mode
    }
    const mode = RIG_MODE_BY_VIEW[view]
    if (!mode) return
    // ALWAYS (re)assert the rig mode on entering an operating view. We must NOT skip it with a
    // same-value guard: the guard ref drifts out of sync with the real rig (handleDigitalMode
    // and the Needed click set the mode without going through here), which left the rig stuck
    // in the wrong mode while the VFO read-back kept working. The backend is idempotent and
    // re-arms an immediate retune, so re-asserting is cheap. Only RE-HOME the frequency on a
    // genuine mode change, so returning to a mode you were already in never yanks the VFO.
    const changed = mode !== lastOpModeRef.current
    lastOpModeRef.current = mode
    const followFreq =
      changed && (view === 'operate' || view === 'cw' || view === 'phone' || view === 'rtty')
    void setOperatingMode(mode, followFreq)
      .then((s) => s && setSnap(s))
      .catch(() => {})
  }, [view])
  const [prop, setProp] = useState<PropagationSnapshot | null>(null)
  // Operate layout mode: Classic (WSJT-X — Band Activity dominant) vs Roster
  // (GridTracker — the Call Roster dominant). Persisted UI pref; Roster is the
  // default (the friendlier at-a-glance view), and die-hards can pick Classic —
  // an explicit 'classic' choice is kept, everyone else gets Roster.
  // PER-SURFACE: which panel dominates the Operate grid is this window's layout.
  const [operateLayout, setOperateLayout] = useState<'classic' | 'roster'>(() => {
    const v = surfaceGet('nexus.operateLayout')
    return v === 'classic' || v === 'roster' ? v : 'roster'
  })
  const handleOperateLayout = useCallback((m: 'classic' | 'roster') => {
    setOperateLayout(m)
    surfaceSet('nexus.operateLayout', m)
  }, [])
  // Which Operate panels the operator keeps (⊞ Panels). Owned HERE, beside the
  // `.operate-host` keep-alive below — never inside the cockpit, which remounts and
  // would drop the record. Survives Classic ↔ Roster because visibility is keyed by
  // panel id, not by layout.
  const operatePanels = usePanelLayout(OPERATE_PANELS)
  // Host SSTV's panel record above its keep-alive view so removals + share edits survive the
  // view's remounts (panelState.ts: owned by a host that outlives the view).
  const sstvPanels = usePanelLayout(SSTV_PANELS)
  // Phone has NO removable panels (operator 2026-07-25) — every pane is fixed and framed.
  const phonePanels = usePanelLayout(PHONE_PANELS)
  const cwPanels = usePanelLayout(CW_PANELS)
  const rttyPanels = usePanelLayout(RTTY_PANELS)

  // One-shot on launch: check the release feed for a newer version (throttled to once/day + cached,
  // silent when offline). Surfaces a dismissible "update available" toast; nothing auto-downloads.
  useEffect(() => {
    void maybeCheckForUpdate()
  }, [])

  // Operate MODE: 'dx' (FT8/FT4 structured cockpit) or 'msg' (Tempo two-way
  // calling). The FT8/FT4 ⇄ Tempo switch binds the radio tier+mode and swaps only
  // the cockpit; Connect/Map/Prop/Logbook/Awards are GLOBAL views selected from the
  // sidebar (they never retune the radio). Default FT8/FT4 (the 80% case).
  const [area, setArea] = useState<'dx' | 'msg'>(() => {
    try {
      // coerceArea (test-pinned) also migrates the retired 'connect' area to
      // FT8/FT4 (Connect is now a global view).
      return coerceArea(localStorage.getItem('nexus.workspace'))
    } catch {
      return 'dx' /* unreadable storage */
    }
  })
  // Sync the engine to the persisted mode once on load (atomic tier+mode).
  const areaSyncedRef = useRef(false)
  useEffect(() => {
    if (areaSyncedRef.current || !snap) return
    areaSyncedRef.current = true
    void apiSetArea(area).then((s) => s && setSnap(s))
    // Reconcile the cockpit view with the mode (a persisted Tempo mode must not
    // open on the FT8/FT4 cockpit, and vice-versa). Global views are left alone.
    setView((v) =>
      area === 'msg' && v === 'operate' ? 'chat' : area === 'dx' && v === 'chat' ? 'operate' : v,
    )
  }, [snap, area])

  // Pop the Needed board out into its own window at launch — but honor the
  // operator's persisted choice (the board previously force-opened EVERY start
  // with no opt-out; closing it each session was a standing annoyance). The
  // toggle lives on the Needed panel header; default stays auto-open.
  const neededPoppedRef = useRef(false)
  useEffect(() => {
    if (neededPoppedRef.current || !snap) return
    if (features.enabled.needed === false) return
    neededPoppedRef.current = true
    try {
      if (localStorage.getItem('nexus.needed.autopop') === 'off') return
    } catch {
      /* storage blocked — keep the default behavior */
    }
    void openPanelWindow('needed').catch(() => {})
  }, [snap, features.enabled])

  const handleWorkspace = useCallback((w: 'dx' | 'msg') => {
    setArea(w)
    try {
      localStorage.setItem('nexus.workspace', w)
    } catch {
      /* ignore */
    }
    // Switching mode lands on that mode's cockpit (FT8/FT4 → Operate, Tempo → Chat).
    setView(w === 'dx' ? 'operate' : 'chat')
    void withErrorToast(() => apiSetArea(w), 'Could not switch mode').then((s) => {
      if (s) setSnap(s)
    })
  }, [])
  // Surface a dead radio engine (audio_error) in the persistent status lane —
  // it was only visible deep in Settings ▸ CAT, i.e. effectively invisible.
  useEffect(() => {
    const err = snap?.radio.audioError
    if (err) {
      setStatus('audio', { tier: 'critical', message: 'RADIO STOPPED', detail: err })
    } else {
      setStatus('audio', null)
    }
  }, [snap?.radio.audioError])

  // Surface a serial COM-port collision (two radios on one port) in the status
  // lane — otherwise it only shows as an unexplained red radio pill.
  useEffect(() => {
    const warn = snap?.radio.radioConfigWarning
    setStatus(
      'radioConfig',
      warn ? { tier: 'warning', message: 'RADIO CONFIG', detail: warn } : null,
    )
  }, [snap?.radio.radioConfigWarning])

  // A per-QSO recording that could not be written. The contact IS logged — only the audio failed —
  // so this is a warning in the lane rather than an error on the log action, and it names the full
  // path because "it did not save" without saying where is exactly the report that prompted it.
  useEffect(() => {
    const warn = snap?.radio.recordingWarning
    setStatus(
      'recording',
      warn ? { tier: 'warning', message: 'RECORDING', detail: warn } : null,
    )
  }, [snap?.radio.recordingWarning])

  // Connector auto-upload outcomes (QRZ/ClubLog/eQSL) now happen in the backend
  // log funnel; the engine bumps uploadTick per outcome and we toast it here —
  // the operator SEES every upload land (or fail) regardless of which path
  // logged the QSO (the auto-logged FT8 path included).
  const uploadTickRef = useRef(0)
  useEffect(() => {
    const tick = snap?.uploadTick ?? 0
    if (tick !== uploadTickRef.current) {
      uploadTickRef.current = tick
      if (snap?.uploadNote) {
        pushToast(snap.uploadNote, snap.uploadOk ? 'success' : 'error')
      }
    }
  }, [snap?.uploadTick, snap?.uploadNote, snap?.uploadOk])


  // Per-(band,mode) last-alert time so a band coming alive toasts once, not every
  // poll (defence in depth — the backend tracker already flags `isNew` once).
  const openingAlertRef = useRef<Map<string, number>>(new Map())
  // Freshest fast-lane X-ray reading (60 s poller below) — merged with each prop
  // snapshot so the flare heads-up fires app-wide, whatever view is open.
  const xrayFastRef = useRef<number | null>(null)
  // Chased-DXpedition alert inputs: the latest windows sweep (10-min poller
  // below), the current QSO partner (kept fresh by the decode-alert effect),
  // and the work action (assigned once handleWorkMapSpot exists).
  const dxpedWindowsRef = useRef<Map<string, DxpedWindow> | null>(null)
  const qsoPartnerRef = useRef<string | null>(null)
  const workDxpedRef = useRef<((c: WorkableCard) => void) | null>(null)
  // Latest link tier (TempoFast/TempoDeep/FT8/FT4) — the rail's Digital button preserves it.
  const tierRef = useRef<Tier>('FT8')
  useEffect(() => {
    let live = true
    const OPENING_ALERT_COOLDOWN_MS = 10 * 60_000
    const load = () =>
      getPropagation()
        .then((p) => {
          if (!live) return
          setProp(p)
          // Solar-flare heads-up (edge-triggered; flareAlert.ts owns the dedup).
          processFlare(effectiveXray(xrayFastRef.current, p.spaceWx.xrayLong))
          // Chased-expedition window alerts (dxpedChase.ts owns the dedup).
          processDxpedAlerts(
            p.dxpeditions.workableNow,
            dxpedWindowsRef.current,
            qsoPartnerRef.current,
            (c) => workDxpedRef.current?.(c),
          )
          // Armed wake-me alarms (dxpedAlarm.ts owns persistence + never-twice).
          checkDxpedAlarms(dxpedWindowsRef.current, Date.now())
          // Satellite pass alarms fire app-wide, not only with the section open.
          // Armed birds only (no IPC otherwise); a 2 h horizon covers the max lead.
          const armedSats = Object.keys(satAlarmMap())
          if (armedSats.length > 0) {
            getSatSchedule(armedSats, 2)
              .then((passes) => checkSatAlarms(passes, Date.now()))
              .catch(() => {})
          }
          // ISS SSTV auto-arm (opt-in, default off): at AOS tune 145.800 FM + arm
          // the decoder, at LOS restore the dial. Only fetch the pass when the
          // opt-in is ON (no IPC otherwise); when OFF we still tick so an arm in
          // flight unwinds — the disabled path ignores the pass.
          // `tuneChannel` for the 145.800 ARM (the app's own channel — never remembered as
          // the operator's 2 m dial); `setFrequency` for the LOS restore, which puts back a
          // dial that really is theirs.
          const issDeps = {
            tuneChannel: handleTuneChannel,
            setFrequency: handleSetFrequency,
            sstvArm,
            sstvAutoDisarm,
          }
          if (settingsRef.current?.issSstvAutoArm === true) {
            getIssPass()
              .then((issPass) =>
                tickIssAutoArm(issPass, snapRef.current?.radio, true, issDeps, Date.now() / 1000),
              )
              // Rejection is a ROUTINE state (elements past 30 d make
              // get_iss_pass refuse) — it must still tick with no pass, or an
              // arm in flight when the refusal begins never unwinds and the
              // rig strands on 145.800 with SSTV armed (review catch).
              .catch(() =>
                tickIssAutoArm(null, snapRef.current?.radio, true, issDeps, Date.now() / 1000),
              )
          } else {
            tickIssAutoArm(null, snapRef.current?.radio, false, issDeps, Date.now() / 1000)
          }
          // One-shot alert when a band comes alive, TIERED by propagation mode:
          // Es/F2/aurora loud (rare + fleeting, aurora with operating guidance),
          // tropo an informative note (openingAlert.ts owns the tiering).
          const tnow = Date.now()
          for (const o of p.openings) {
            if (!o.isNew) continue
            const key = `${o.band}|${o.mode}`
            const last = openingAlertRef.current.get(key) ?? 0
            if (tnow - last < OPENING_ALERT_COOLDOWN_MS) continue
            openingAlertRef.current.set(key, tnow)
            const spec = openingToastSpec(o)
            if (spec.beepHz != null) doubleBeep(spec.beepHz)
            pushToast(spec.message, spec.kind, spec.ttlMs, spec.prominent ? { prominent: true } : {})
          }
          // Honest-state: surface non-live propagation in the Now-Bar lane.
          if (p.source === 'offline') {
            setStatus('prop', {
              tier: 'warning',
              message: 'Prop: no live data',
              detail:
                'No live propagation data yet — set your callsign in Settings and check your internet connection.',
            })
          } else if (p.source === 'cached') {
            const ageMin = Math.max(0, Math.round((Date.now() / 1000 - p.asOf) / 60))
            setStatus('prop', {
              tier: 'warning',
              message: `Prop: cached ${ageMin}m`,
              detail: 'Live propagation refetch failed — showing the last-good snapshot.',
            })
          } else {
            setStatus('prop', null)
          }
        })
        .catch(() => {})
    load()
    const id = setInterval(load, 30_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // AOS/LOS alerts for the ARMED pass track ("Work this pass") — the operator
  // armed it from Satellites but may be anywhere in the app when the bird
  // rises, so the watch lives HERE, not in a view (every view-scoped poll dies
  // with its view). 2 s matches the section's own cadence; the read is a mutex
  // clone backend-side. Edge detection + all firing rules live in
  // satPassAlert.ts; this effect only feeds it answers in arrival order.
  useEffect(() => {
    let live = true
    // The 2 s tick can lap a slow answer, and a lapped answer applied late
    // re-seeds the module's previous-track state with a dead pass as "live".
    // The module's per-pass fired keys already make that harmless, but a
    // straggler must still not REORDER the observed truth — drop it.
    let issued = 0
    let applied = 0
    const load = () => {
      const seq = ++issued
      getSatTrackStatus()
        .then((t) => {
          if (!live || seq < applied) return
          applied = seq
          tickSatPassAlert(t, Math.floor(Date.now() / 1000), {
            soundOff: settingsRef.current?.satPassAlertSoundOff === true,
            rotPostPass: settingsRef.current?.rotPostPass ?? 'stop',
          })
        })
        .catch(() => {})
    }
    load()
    const id = setInterval(load, 2000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // Element currency in the Now-Bar `sat` lane (beside `prop` above). The
  // decision itself — the Celestrak hard stop vs an all-rotten cache vs the
  // 14 d stale line, and when each deserves the operator's attention — is the
  // pure satElementsLane (features/satLane.ts). Polling get_tle_status also
  // kicks a due background refresh, so currency self-heals app-wide, not only
  // while a satellite surface is open.
  useEffect(() => {
    let live = true
    const load = () =>
      getTleStatus()
        .then((s) => {
          if (!live) return
          setStatus('sat', satElementsLane(s, Date.now() / 1000))
        })
        .catch(() => {})
    load()
    const id = setInterval(load, 60_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // X-ray fast lane (60 s): flare ONSET reaches the operator in ~1 min instead of
  // the 5-min prop-snapshot cadence. Best-effort — a failed fetch just leaves the
  // snapshot's slower value driving the watcher.
  useEffect(() => {
    let live = true
    const load = () =>
      getXrayNow()
        .then((x) => {
          if (!live) return
          xrayFastRef.current = x.flux
          processFlare(effectiveXray(x.flux, null))
        })
        .catch(() => {})
    load()
    const id = setInterval(load, 60_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // DXpedition best-shot windows for the chase alerts (server-cached climatology;
  // 10 min is generous). Best-effort — without it the loud spotted-alert still
  // works from the snapshot's cards; only the quiet modelled-only toast needs it.
  useEffect(() => {
    let live = true
    const load = () =>
      getDxpedWindows()
        .then((list) => {
          if (live) dxpedWindowsRef.current = new Map(list.map((w) => [w.call.toUpperCase(), w]))
        })
        .catch(() => {})
    load()
    const id = setInterval(load, 600_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // Live-feed liveness for the Now-Bar connector pills (same cadence as prop).
  const [feedHealth, setFeedHealth] = useState<FeedHealth | null>(null)
  useEffect(() => {
    let live = true
    const load = () =>
      getFeedHealth()
        .then((h) => live && setFeedHealth(h))
        .catch(() => {})
    load()
    const id = setInterval(load, 30_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  const [needAlerts, setNeedAlerts] = useState<NeedAlert[]>([])
  // Refetch the needed board. Called on a 30 s poll AND immediately after any log so a
  // just-worked station drops off the roster/needs at once (the backend rebuilds needs
  // from the full log, excluding the QSO we just wrote) instead of lingering up to 30 s.
  const refreshNeeds = useCallback(() => {
    getNeedAlerts()
      .then((alerts) => setNeedAlerts(alerts))
      .catch(() => {})
  }, [])
  useEffect(() => {
    refreshNeeds()
    const id = setInterval(refreshNeeds, 30_000)
    return () => clearInterval(id)
  }, [refreshNeeds])
  // Raw spot firehose for the Spots panel (ungated, all modes). Polled faster than needs
  // since it's a live "what's on the air" view; the backend command just reads the buffer.
  const [allSpots, setAllSpots] = useState<SpotRow[]>([])
  useEffect(() => {
    let live = true
    const load = () =>
      getAllSpots()
        .then((s) => {
          if (live) setAllSpots(s)
        })
        .catch(() => {})
    load()
    const id = setInterval(load, 15_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])
  // Gate CW/Phone needs by the operator's enabled modes — the backend emits voice/CW
  // needs unconditionally, visibility is the frontend's call. A pure-digital op's board,
  // roster colouring, and map highlight all derive from THIS gated set, so they stay
  // exactly as before the feature.
  const cwEnabled = features.isOn('cw')
  const phoneEnabled = features.isOn('phone')
  const rttyEnabled = features.isOn('rtty')
  // Work-a-spot navigation: whenever a spot is worked — from THIS window's
  // boards or a pop-out (which can't navigate the main window itself) — follow
  // to the matching cockpit (the operator's report: "if I click a contact, it
  // should bring me into the right section"). Baselined on the first snapshot
  // so a webview reload never replays the engine's last work action.
  const workNavRef = useRef<number | null>(null)
  useEffect(() => {
    const tick = snap?.workTick ?? 0
    if (workNavRef.current === null) {
      workNavRef.current = tick
      return
    }
    if (tick === workNavRef.current) return
    workNavRef.current = tick
    const v = snap?.workView
    const target: View =
      v === 'cw' ? 'cw' : v === 'phone' ? 'phone' : v === 'rtty' ? 'rtty' : 'operate'
    // Never navigate into a feature-disabled (hidden) cockpit — same gate as
    // handleWorkNeeded; the rig already switched, the view just stays put.
    if (
      (target === 'cw' && !cwEnabled) ||
      (target === 'phone' && !phoneEnabled) ||
      (target === 'rtty' && !rttyEnabled)
    )
      return
    // Sync the rig-mode guard BEFORE navigating (same as handleWorkNeeded) —
    // otherwise the [view] effect sees a mode change and re-homes the dial to
    // the segment start, yanking the rig OFF the exact spot frequency the
    // workSpot click just tuned (review-caught on the pop-out path).
    lastOpModeRef.current = target === 'operate' ? 'digital' : target
    setView(target)
    // Prefill the log Call from a work action fired in ANOTHER window (e.g. the pop-out band
    // map), matching the prefill an in-window click gets via handleWorkNeeded. Digital
    // auto-sequences on a decode double-click, so it takes no prefill; RTTY's cockpit takes
    // no prefill either (only the CW/Phone log forms consume pendingWork).
    const wc = snap?.workCall
    if (wc && (target === 'cw' || target === 'phone')) {
      setPendingWork({ call: wc, view: target, ts: Date.now() })
    }
  }, [snap?.workTick, snap?.workView, cwEnabled, phoneEnabled, rttyEnabled])
  const visibleAlerts = useMemo(
    () => visibleNeeds(needAlerts, { cw: cwEnabled, phone: phoneEnabled }),
    [needAlerts, cwEnabled, phoneEnabled],
  )
  // Activity TYPE per heard call (POTA / SOTA / DXpedition), independent of the need tier.
  // needByCall keeps only tags[0], so a POTA activator that's ALSO a new band shows the band
  // colour and loses the program tag — this map surfaces the program badge regardless, so the
  // band strip/map can flag "this is a park/summit/DXped" even when a higher need wins the colour.
  const typeByCall = useMemo(() => activityTypeByCall(visibleAlerts), [visibleAlerts])
  // Full alert set per heard call (all bands/modes) for the band-activity decode feed's
  // need-icons + row colouring — richer than needByCall's top-tag-only map. Keyed
  // UPPERCASE; from the same GATED visibleAlerts so a disabled mode never tags a row.
  const needAlertsByCall = useMemo(() => alertsByCall(visibleAlerts), [visibleAlerts])
  // Need-tier per heard call for roster/map/band-strip colouring — each call's STRONGEST
  // need, derived from the grouped set above so there is ONE definition of that (shared with
  // the roster's "sort by need"). From the GATED alerts, so a disabled mode never colours a
  // station the board hides.
  const needByCall = useMemo(() => topNeedByCall(needAlertsByCall), [needAlertsByCall])
  const [typingTick, setTypingTick] = useState(0)
  const [bandPlan, setBandPlan] = useState<BandChannel[]>([])
  const [settings, setSettings] = useState<Settings | null>(null)
  // Operators this log has already seen (#25) — the seat-swap roster. Refreshed when the
  // operator changes, which is the moment a new name can have entered the log.
  const [opRoster, setOpRoster] = useState<string[]>([])
  useEffect(() => {
    void logOperators()
      .then(setOpRoster)
      .catch(() => {}) // older core / no bridge — the chip just offers no switch targets
  }, [settings?.fdOperator])
  /** Switch (or clear) the operator at the key. Narrow write — read-modify-write through
      the whole settings struct ran the heavyweight save, which ends the QSO the seat swap
      was made during (#54). */
  const handleSetOperator = useCallback(async (call: string) => {
    const op = call.trim().toUpperCase()
    await setFdOperator(op)
    setSettings((prev) => (prev ? { ...prev, fdOperator: op } : prev))
  }, [])
  // Live mirror for the app-wide 30 s poll effect — the ISS SSTV auto-arm reads
  // its opt-in here (the effect's empty deps can't see `settings` directly).
  const settingsRef = useRef<Settings | null>(null)
  useEffect(() => {
    settingsRef.current = settings
  }, [settings])
  // Seed the rig-mode guard from the mode the operator last worked in (persisted). Settings
  // load async, so this can't live in the mount guard above; keying on the VALUE rather than
  // the settings object means it settles once and doesn't re-fire. Without seeding, the guard
  // would sit at its 'digital' default, so the operator's first click into a section could
  // compute `changed` wrongly and needlessly yank the VFO. See the mount guard above.
  useEffect(() => {
    if (opModeSeeded.current) return
    const booted = settings?.operatingMode
    if (booted === 'phone' || booted === 'cw' || booted === 'rtty' || booted === 'digital') {
      lastOpModeRef.current = booted
      opModeSeeded.current = true
    }
  }, [settings?.operatingMode])
  // Ding/dong when the dial crosses your TX privileges (default on).
  useBandEdgeTones(snap?.radio.txAllowed, settings?.bandEdgeTones ?? true)
  // User watch list (localStorage) — fed to the decode alerter. Re-synced when the manager
  // edits it (it dispatches `nexus:watchlist-changed`), so alerts pick up changes live.
  const [watchlist, setWatchlist] = useState<WatchFilter[]>(() => loadWatchlist())
  useEffect(() => {
    const resync = () => setWatchlist(loadWatchlist())
    window.addEventListener('nexus:watchlist-changed', resync)
    return () => window.removeEventListener('nexus:watchlist-changed', resync)
  }, [])
  const [onboardDismissed, setOnboardDismissed] = useState<boolean>(
    () => localStorage.getItem(ONBOARD_KEY) === '1',
  )

  // Track how many messages we'd "read" per peer to compute unread counts.
  const readCounts = useRef<Record<string, number>>({})

  const reloadSettings = useCallback(() => {
    getSettings().then(setSettings).catch(() => {})
  }, [])

  // Mirror the units setting to localStorage so the display helpers (many called from
  // components that never receive the full settings object) resolve it synchronously via
  // useUnits() — the app-global pattern the country exclude uses. (F4MQS.)
  useEffect(() => {
    setUnitsMirror(settings?.units)
  }, [settings?.units])

  // initial load + live subscription
  useEffect(() => {
    let mounted = true
    getSnapshot().then((s) => mounted && setSnap(s))
    getBandPlan()
      .then((b) => mounted && setBandPlan(b))
      .catch(() => {})
    getSettings()
      .then((s) => mounted && setSettings(s))
      .catch(() => {})
    const unsub = subscribeSnapshot((s) => {
      if (mounted) setSnap(s)
    })
    return () => {
      mounted = false
      unsub()
    }
  }, [])

  // Refetch the band plan when the tier changes: FT8/FT4 use the standard WSJT-X
  // watering holes (14.074 …), TempoFast/TempoDeep the native off-cluster plan. So picking a
  // band always lands you where that mode actually calls.
  const activeTier = snap?.link.tier
  useEffect(() => {
    let live = true
    getBandPlan()
      .then((b) => live && setBandPlan(b))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [activeTier])

  // Periodic re-eval ticker so the unread badges refresh smoothly between snapshot
  // polls (the unread memos read a ref cursor that a dep change alone won't catch).
  useEffect(() => {
    const id = window.setInterval(() => setTypingTick((t) => t + 1), 400)
    return () => window.clearInterval(id)
  }, [])


  const activePeer = snap?.activePeer ?? null

  // mark the active conversation as read whenever it updates
  useEffect(() => {
    if (!snap) return
    // Prune read-cursors for threads that no longer exist (archived, or rebuilt) so
    // a re-created thread starts unread from 0 instead of inheriting a stale cursor
    // that would collapse its unread count to zero.
    const live = new Set(snap.conversations.map((c) => c.peer))
    for (const k of Object.keys(readCounts.current)) {
      if (!live.has(k)) delete readCounts.current[k]
    }
    if (!activePeer) return
    const conv = snap.conversations.find((c) => c.peer === activePeer)
    if (conv) readCounts.current[activePeer] = conv.messages.length
  }, [snap, activePeer])

  const unreadByPeer = useMemo(() => {
    const out: Record<string, number> = {}
    if (!snap) return out
    for (const c of snap.conversations) {
      // the "*" broadcast peer is an engine-internal bus; skip unread badges
      if (c.peer === '*') continue
      const read = readCounts.current[c.peer] ?? 0
      const inbound = c.messages.filter((m) => !m.outbound).length
      const readInbound = Math.min(read, c.messages.length)
      // unread = inbound messages beyond what we've seen
      const unread = c.messages.slice(readInbound).filter((m) => !m.outbound).length
      if (c.peer !== activePeer && unread > 0) out[c.peer] = unread
      void inbound
    }
    return out
  }, [snap, activePeer, typingTick])

  // Unread on the "*" band feed (CQs/broadcasts from others). Tracked separately
  // from unreadByPeer (which is per-station) and shown on the pinned Band row; the
  // read cursor is bumped by the same effect above when "*" is the active peer.
  const bandUnread = useMemo(() => {
    if (!snap || activePeer === '*') return 0
    const band = snap.conversations.find((c) => c.peer === '*')
    if (!band) return 0
    const read = readCounts.current['*'] ?? 0
    const readInbound = Math.min(read, band.messages.length)
    return band.messages.slice(readInbound).filter((m) => !m.outbound).length
  }, [snap, activePeer, typingTick])

  const handleSelect = useCallback((call: string) => {
    void withErrorToast(() => apiSelectPeer(call), 'Could not select station').then(
      (s) => s && setSnap(s),
    )
  }, [])

  // Confirm here rather than in StationList so every host (cockpit, detached panel) gets
  // the same guard. Unconditional is right: the recents list only renders threads that
  // have messages, so there is no empty-thread case to skip. The copy names the
  // non-obvious consequence — deleting also cancels still-queued outbound traffic.
  const handleArchive = useCallback((peer: string) => {
    if (
      !window.confirm(
        `Delete the conversation with ${peer}? Any messages still waiting to send will be cancelled. This can't be undone.`,
      )
    )
      return
    void withErrorToast(
      () => apiArchiveConversation(peer),
      'Could not delete conversation',
    ).then((s) => s && setSnap(s))
  }, [])

  // The Map and the roster share ONE selection: the active peer. Clicking a map
  // dot selects (or, if already selected, clears) that station — and the roster
  // highlights it too, since StationList already keys its highlight off activePeer.
  const handleMapSelect = useCallback((call: string | null) => {
    void withErrorToast(() => apiSelectPeer(call), 'Could not select station').then(
      (s) => s && setSnap(s),
    )
  }, [])

  // Pounce — the app's one PUSH channel. The earcon has already played inside the hook by the
  // time `pounceAlert` is set; this is the visual half plus the one-click work.
  const { alert: pounceAlert, dismiss: dismissPounce } = usePounce()
  // Signed self-update: downloads quietly, installs only on an explicit press that the engine
  // refuses while the radio is busy (see useSelfUpdate / update_install_block).
  const selfUpdate = useSelfUpdate()
  const handlePounceWork = useCallback(
    (a: PounceAlert) => {
      // Route through the SAME path the needed board uses, so QSY, rig mode and pileup-split
      // handling behave identically to working any other spot. A separate path here would drift.
      const kind =
        a.mode === 'CW' ? 'cw' : a.mode === 'Phone' ? 'phone' : ('digital' as const)
      if (a.freqMhz) {
        workSpot(kind as 'digital' | 'phone' | 'cw', a.freqMhz, a.band, a.call)
          .then((snap) => setSnap(snap))
          .catch(() => pushToast(`Could not QSY to ${a.call}`, 'error'))
      }
      dismissPounce()
    },
    [dismissPounce],
  )

  const handleCall = useCallback(
    (call: string, grid?: string, message?: string, snr?: number, freq?: number, tier?: Tier | null) => {
      // Clicking your OWN line (your CQ / TX echo): the engine guards against
      // the self-QSO, but without this the command still returns a snapshot and
      // we'd flash a FALSE "Working KD9TAW" success toast.
      const me = mycallRef.current.trim().toUpperCase()
      if (me && call.trim().toUpperCase().split('/')[0] === me.split('/')[0]) {
        pushToast(`${call} is your own call`, 'info', 2500)
        return
      }
      // Route by the CONTACT's protocol, not the current view: a Tempo/TempoFast contact stays in
      // Tempo. Working one means OPENING THE CONVERSATION with them (select_peer) in Chat —
      // NOT running the FT8 call sequence (apiCallStation) or bouncing to the Operate cockpit,
      // which is what wrongly yanked an TempoFast contact into FT8. `handleWorkspace('msg')` switches
      // area+view to Tempo the same way the mode picker does. (tier null/undefined = keep the
      // current FT8/digital behaviour below — the "tier: None = keep" convention.)
      if (tier === 'TempoFast' || tier === 'TempoDeep') {
        handleWorkspace('msg')
        void withErrorToast(() => apiSelectPeer(call), `Could not open ${call}`).then((s) => {
          if (s) setSnap(s)
        })
        pushToast(`▶ ${call} — open in Tempo`, 'success', 3000)
        return
      }
      void withErrorToast(
        () => apiCallStation(call, grid, message, snr, freq),
        `Could not work ${call}`,
      ).then((s) => {
        if (s) {
          setSnap(s)
          // Work the station on the single-screen Operate cockpit — the QSO
          // sequences inline there while the waterfall + decodes stay visible.
          // (Never bounce to the chat-style 'qso' view and lose the band.)
          setView('operate')
          // Immediate confirmation the action took (and TX is now armed for it).
          pushToast(`▶ Working ${call} — transmitting your call`, 'success', 4000)
        }
      })
    },
    [handleWorkspace],
  )

  // Roster/StationCard adapter: a card knows only the callsign + its protocol tier, so it
  // calls onCall(call, tier). handleCall's other params (grid/message/snr/freq) are only used
  // by the FT8 call-sequence path, so pass them undefined here and route by tier.
  const handleWorkStation = useCallback(
    (call: string, tier?: Tier | null) => handleCall(call, undefined, undefined, undefined, undefined, tier),
    [handleCall],
  )

  // Fire decode alerts (beep + toast) whenever the decode feed changes, gated by the
  // user's alert settings. processDecodes dedups internally. The third arg makes each
  // alert toast click-to-work — working the station is what you almost always want next
  // (someone calling you, a new DXCC/grid, a CQ). Placed AFTER handleCall so it's in scope.
  // The QSO context keeps popups quiet while actively working someone / running CQ.
  useEffect(() => {
    if (!snap || !settings) return
    const dxcall = snap.fieldDay?.dxcall ?? snap.qso?.dxcall ?? null
    // Keep the chase-alert suppression in sync (the prop poller reads the ref).
    qsoPartnerRef.current = dxcall
    // Latest tier for the rail's Digital button (preserve FT4 across nav).
    tierRef.current = snap.link.tier
    processDecodes(
      snap.recentDecodes,
      settings,
      (d) => {
        // Pass the decode's TIER — without it an FT1/Tempo decode alert ran the FT8
        // call sequence (the "Work on FT1 wrongly switches to FT8" bug); handleCall
        // routes chat tiers to the Tempo conversation instead.
        if (d.from) handleCall(d.from, undefined, d.message, d.snr, d.freqHz, d.tier)
      },
      // Field Day runs its own sequencer (snap.qso is null there) — its state
      // strings (CallingCq/AwaitExchange/AwaitConfirm/Done) gate identically.
      {
        state: snap.fieldDay?.state ?? snap.qso?.state ?? null,
        dxcall,
      },
      watchlist,
      // Current dial for the per-alert band scopes (grid alerts default to VHF+ only).
      snap.radio.dialMhz > 0 ? snap.radio.dialMhz : undefined,
    )
  }, [snap, settings, handleCall, watchlist])

  // Bumps when a QSO is logged AND "Clear DX call after logging" is on — the
  // cockpit watches it and wipes its DX Call/Grid fields (stock WSJT-X option).
  const [dxClearTick, setDxClearTick] = useState(0)
  const noteLoggedForDxClear = useCallback(() => {
    if (settings?.clearDxAfterLog) setDxClearTick((t) => t + 1)
  }, [settings?.clearDxAfterLog])

  const handleConfirmLog = useCallback(
    (record: LoggedQso) => {
      void withErrorToast(() => apiConfirmPendingLog(record), 'Could not log QSO').then((s) => {
        if (s) {
          setSnap(s)
          noteLoggedForDxClear()
          refreshNeeds() // drop the just-worked station from the roster/needs immediately
          // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
          // (every log path, auto-log included); outcomes toast via uploadTick.
        }
      })
    },
    [noteLoggedForDxClear, refreshNeeds],
  )

  const handleDiscardLog = useCallback(() => {
    void withErrorToast(() => apiDiscardPendingLog(), 'Could not discard QSO').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSend = useCallback(
    (text: string) => {
      if (!activePeer) return
      void withErrorToast(
        () => apiSendMessage(activePeer, text),
        'Message could not be sent',
      ).then((s) => s && setSnap(s)) // instant echo — no 300 ms poll wait
    },
    [activePeer],
  )

  // Call CQ / canned broadcast: not directed at a peer — goes to everyone on
  // frequency (the engine prefixes `DE <MYCALL>` and echoes it into the "*" band
  // feed). Surfacing "*" makes that feed visible so the operator sees their own
  // call go out and any replies land in the same pane.
  const surfaceBandFeed = useCallback((s: AppSnapshot | null) => {
    // Only surface the "*" band feed if the broadcast/CQ actually went out (don't yank
    // the operator into the feed on a failure); route the select through withErrorToast
    // so a select failure can't throw an unhandled rejection.
    if (!s) return
    setSnap(s)
    void withErrorToast(() => apiSelectPeer('*'), 'Could not open the band feed').then(
      (s2) => s2 && setSnap(s2),
    )
  }, [])

  // Tap-to-resend a terminal (no-ack / abandoned) bubble: same text, fresh cycle budget,
  // same bubble (the backend re-points it at the new queue entry).
  const handleResend = useCallback((m: import('./types').ChatMessage) => {
    const peer = m.to
    if (!peer) return
    void withErrorToast(() => resendChat(peer, m.ackId ?? null), `Could not re-send to ${peer}`).then(
      (s) => {
        if (s) {
          setSnap(s)
          pushToast(`↻ Re-sending to ${peer}`, 'success', 3000)
        }
      },
    )
  }, [])

  const handleBroadcast = useCallback(
    (text: string) => {
      void withErrorToast(() => apiBroadcast(text), 'Could not broadcast').then(surfaceBandFeed)
    },
    [surfaceBandFeed],
  )

  // Call CQ sends a STRUCTURED `CQ <call> <grid>` frame + arms TX (distinct from the
  // free-text broadcast). The backend rejects it if the callsign/grid aren't set, so a
  // CQ never goes out malformed — the error surfaces as a toast.
  const handleCallCq = useCallback(() => {
    // The launchpad button now STARTS the CQ run (keep calling until answered) and no
    // longer force-navigates away — the run state lives in the Tempo header, so the
    // affordance never vanishes after one press (the audited CQ dead-end).
    void withErrorToast(() => apiSetChatCq(true), 'Could not call CQ').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleToggleCqRun = useCallback(() => {
    const next = (snap?.chatCq ?? 'off') === 'off'
    void withErrorToast(() => apiSetChatCq(next), 'Could not toggle the CQ run').then((s) => {
      if (s) setSnap(s)
    })
  }, [snap?.chatCq])

  const handleResumeCqRun = useCallback(() => {
    void withErrorToast(() => apiResumeChatCq(), 'Could not resume the CQ run').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleToggleBeacon = useCallback(() => {
    const next = !(snap?.radio.beacon ?? false)
    void withErrorToast(() => apiSetBeacon(next), 'Could not toggle the heartbeat').then((s) => {
      if (s) setSnap(s)
    })
  }, [snap?.radio.beacon])

  const handleSetFrequency = useCallback(
    (dialMhz: number, band: string, mode: string) => {
      void withErrorToast(
        () => apiSetFrequency(dialMhz, band, mode),
        'Could not set frequency',
      ).then((s) => {
        if (s) setSnap(s)
      })
    },
    [],
  )

  // SSTV's tune: same signature, but it also CLAIMS the Phone section (SSTV rides Phone)
  // atomically with the QSY. Entering the SSTV view deliberately asserts nothing (RX-first),
  // so without this a strip pick left the previous section's mode policy commanding the rig —
  // picking 20 m SSTV from RTTY landed DATA-L at 14.230.
  // Toggle a call on the PERSISTENT blocklist (the narrow write — never the heavyweight
  // settings save; the #54 lesson). The engine's auto-responder honors the list on the
  // next slot; the local settings mirror updates so every pane re-derives immediately.
  const handleToggleBlocked = useCallback(
    (call: string) => {
      const k = call.trim().toUpperCase()
      if (!k) return
      const cur = settingsRef.current?.blockedCalls ?? []
      const next = cur.includes(k) ? cur.filter((c) => c !== k) : [...cur, k]
      void withErrorToast(() => apiSetBlockedCalls(next), 'Could not update the blocklist').then(
        (s2) => {
          if (s2) setSnap(s2)
          setSettings((prev) => (prev ? { ...prev, blockedCalls: next } : prev))
        },
      )
    },
    [],
  )

  const handleSstvTune = useCallback(
    (dialMhz: number, band: string, mode: string) => {
      void withErrorToast(
        () => apiSstvTune(dialMhz, band, mode),
        'Could not set frequency',
      ).then((s) => {
        if (s) setSnap(s)
      })
    },
    [],
  )

  // The app parking the dial on one of its OWN channels (the ISS SSTV auto-arm's 145.800 FM).
  // A dedicated verb, not handleSetFrequency: the backend records who authored a dial write,
  // and an app-driven auto-tune must not be remembered as the operator's dial on that band.
  const handleTuneChannel = useCallback((dialMhz: number, band: string, mode: string) => {
    void withErrorToast(
      () => apiTuneChannel(dialMhz, band, mode),
      'Could not set frequency',
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // APRS Tune: QSY to the APRS frequency on 2 m FM simplex, auto-routing to the 2 m-capable radio.
  // A dedicated verb (not handleSetFrequency) because APRS isn't an operating section, so the rig
  // must be put in FM explicitly — otherwise it keeps the prior section's DATA/USB mode and the
  // 2 m packet never demodulates. Clearing the mode override is handled backend-side on the next QSY.
  const handleAprsTune = useCallback((dialMhz: number) => {
    void withErrorToast(() => aprsTune(dialMhz), 'Could not tune to APRS').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxEnabled = useCallback((enabled: boolean) => {
    void withErrorToast(
      () => apiSetTxEnabled(enabled),
      enabled ? 'Could not enable transmit' : 'Could not mute transmit',
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxLevel = useCallback((level: number) => {
    void withErrorToast(() => apiSetTxLevel(level), 'Could not set TX level').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTune = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetTune(on), 'Could not toggle tune').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Prove-TX-path: key the tune carrier for ~2.5 s then auto-drop — a BOUNDED transmit that lets the
  // operator verify CAT→PTT→RF (forward power registers). Always invoked from behind a confirm
  // dialog (operator's TX-approval condition); the TX watchdog backs up the auto-unkey.
  const handleProveTx = useCallback(() => {
    void withErrorToast(() => apiSetTune(true), 'Could not key the transmitter').then((s) => {
      if (s) setSnap(s)
      window.setTimeout(() => {
        void apiSetTune(false)
          .then((s2) => s2 && setSnap(s2))
          .catch(() => {})
      }, 2500)
    })
  }, [])

  const handleHaltTx = useCallback(() => {
    void withErrorToast(() => apiHaltTx(), 'Could not stop transmit').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Dual-radio: switch the active radio (rig loop swaps rigs, carrier dropped first) and toggle
  // peg-lock. The returned snapshot echoes the new active radio + tune instantly.
  const handleSetActiveRadio = useCallback((id: number) => {
    void withErrorToast(() => apiSetActiveRadio(id), 'Could not switch radios').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetPegLock = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetPegLock(on), 'Could not set peg-lock').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // WSJT-X Tx-slot click (Tx1–Tx5 buttons / Alt+N): force the row's text as the
  // next transmission to the DX. The backend starts/retargets the QSO + arms TX;
  // applying the returned snapshot makes the Tx panel's "next" dot land at once.
  const handleOverrideTx = useCallback((call: string, grid: string | null, text: string) => {
    // Same own-call guard as handleCall — the engine no-ops on a self-target
    // but returns a normal snapshot, which read as silent success here.
    const me = mycallRef.current.trim().toUpperCase()
    if (me && call.trim().toUpperCase().split('/')[0] === me.split('/')[0]) {
      pushToast(`${call} is your own call`, 'info', 2500)
      return
    }
    void withErrorToast(
      () => apiOverrideNextTx(call, grid, text),
      `Could not queue TX to ${call}`,
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxEven = useCallback((even: boolean) => {
    void withErrorToast(() => apiSetTxEven(even), 'Could not set transmit period').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxCycleAuto = useCallback((auto: boolean) => {
    void withErrorToast(() => apiSetTxCycleAuto(auto), 'Could not set the cycle mode').then(
      (s) => {
        if (s) setSnap(s)
      },
    )
  }, [])

  const handleSetHoldTxFreq = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetHoldTxFreq(on), 'Could not toggle Hold Tx').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Waterfall click: left-click sets the RX offset (green marker); shift-click
  // sets the TX offset (red marker). TX follows RX unless "Hold Tx" is on.
  const handleTune = useCallback((hz: number, target: 'tx' | 'rx' | 'both') => {
    // Stock WSJT-X gestures (Waterfall dispatches): 'rx' = click, 'tx' = Shift, 'both' = Ctrl.
    const call =
      target === 'rx'
        ? () => apiSetRxOffset(hz)
        : target === 'tx'
          ? () => apiSetTxOffset(hz)
          : () => apiSetTxOffset(hz).then(() => apiSetRxOffset(hz))
    void withErrorToast(call, 'Could not set offset').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // QSY from the Needed panel: move the rig to that band's channel and listen.
  const handleQsy = useCallback(
    // `freqMhz` is the spot's EXACT frequency from the spotting network — the source of truth
    // (a DXpedition may be well off the standard FT8/FT4 dial). Fall back to the band's dial
    // only when the spot carries no frequency (e.g. a PSK Reporter reception report).
    (band: string, freqMhz?: number) => {
      const ch = bandPlan.find((c) => c.band === band)
      if (!ch) {
        pushToast(`No channel for ${band} in the band plan`, 'error', 3000)
        return
      }
      const dial = freqMhz ?? ch.dialMhz
      void withErrorToast(
        () => apiSetFrequency(dial, ch.band, ch.mode),
        `Could not QSY to ${band}`,
      ).then((s) => {
        if (s) {
          setSnap(s)
          pushToast(`QSY ${dial.toFixed(3)} MHz — listening`, 'success', 2500)
        }
      })
    },
    [bandPlan],
  )

  // Recall a saved memory from ANYWHERE (the Memories section or a cockpit MEM strip):
  // apply the phone-submode + repeater plumbing (shift/tone/odd-split), auto-switch to
  // the memory's cockpit, and retune. Sequencing matters: the rig-mode guard is synced
  // BEFORE navigating (the workNav idiom above) so the [view] effect never sees a mode
  // change and re-homes the dial; the setFrequency lands LAST so the recalled frequency
  // is the final word. A feature-disabled cockpit gates only the NAV — the rig still
  // retunes (same behavior as the Needed board's work-click).
  const recallMemory = useCallback(
    (m: Memory) => {
      const plan = planRecall(m)
      const target = plan.view
      const opMode: 'digital' | 'phone' | 'cw' = target === 'operate' ? 'digital' : target
      // The CW/Phone cockpits are opt-in. Recalling into a disabled cockpit would tune the
      // rig but leave the operator with no place to work it — and (finding) half-apply a
      // heavyweight settings write for a hidden feature. Guide them to enable it instead.
      if ((target === 'cw' && !cwEnabled) || (target === 'phone' && !phoneEnabled)) {
        pushToast(
          `Enable the ${target === 'cw' ? 'CW' : 'Phone'} section in Settings to recall this memory`,
          'info',
          4000,
        )
        return
      }
      const mode = m.mode.toUpperCase()
      const band = bandLabelForMhz(plan.freqMhz)
      void (async () => {
        try {
          // FM repeaters (and a phone SUBMODE flip) need phone_mode + shift/tone written
          // BEFORE the atomic tune so the rig keys the machine, not just the output. Guard
          // the heavyweight whole-struct save: FM always (re)writes its plumbing; SSB writes
          // only on a real fm→ssb flip — never on an ssb→ssb net hop.
          const patch = plan.settingsPatch
          if (patch) {
            const s = await getSettings()
            const isFm = patch.rptrShift !== undefined
            if (isFm || patch.phoneMode !== s.phoneMode) {
              const patched = { ...s, ...patch }
              await apiSetSettings(patched)
              setSettings(patched)
            }
          }
          // ATOMIC band+mode+freq — the SAME verb the Needed board uses, so the rig can never
          // land in the new mode at the old dial (no wrong-mode flash) and the mode is always
          // asserted, gated path excluded above.
          const s2 = await workSpot(opMode, plan.freqMhz, band)
          if (!s2) {
            pushToast('Recall failed — check CAT', 'error', 3000)
            return
          }
          setSnap(s2)
          // Honor a saved SSB memory's EXACT sideband over the band-auto convention (some nets
          // run off-convention, e.g. USB below 10 MHz), and drop any stale manual override.
          // Set AFTER the tune — set_frequency clears the override on a band change.
          if (target === 'phone' && (mode === 'USB' || mode === 'LSB')) {
            const s3 = await setSidebandOverride(mode as 'USB' | 'LSB')
            if (s3) setSnap(s3)
          }
          // Keep the rig-mode effect's guard in sync so the nav doesn't re-home the dial.
          lastOpModeRef.current = opMode
          setView(target)
          memoriesStore.update((b) => markRecalled(b, m.id, Math.floor(Date.now() / 1000)))
          // The phone policy commands only SSB/FM; AM/WFM/DV/DN can't be set there, so for a
          // phone memory in one of those we tune the dial but tell the operator to set the mode
          // on the rig rather than claim a mode we didn't command. (CW and digital recalls set
          // their mode via the CW/DATA operating policy, so they're always honest.)
          const phoneUncommandable =
            target === 'phone' && mode !== 'USB' && mode !== 'LSB' && mode !== 'FM'
          pushToast(
            phoneUncommandable
              ? `${m.name} — ${plan.freqMhz.toFixed(3)} MHz · set ${plan.mode} on the rig`
              : `${m.name} — ${plan.freqMhz.toFixed(3)} MHz ${plan.mode}`,
            'success',
            2500,
          )
        } catch (e) {
          pushToast(`Recall failed: ${e instanceof Error ? e.message : e}`, 'error', 4000)
        }
      })()
    },
    [cwEnabled, phoneEnabled],
  )

  // Global quick-recall hotkeys: Ctrl+1..9 recall the 1st..9th ★ favorite from ANY
  // section (the same action as clicking its MEM-strip chip — recallMemory tunes +
  // switches to the right cockpit). Rides a ref so the listener binds once and always
  // sees the latest recallMemory + enabled flag; favorites are read fresh at press time.
  // Ctrl+Digit is clear of the cockpits' Alt+Digit (FT8 Tx) and F-key macros.
  const recallHotkeyRef = useRef({ recall: recallMemory, enabled: features.isOn('memories') })
  recallHotkeyRef.current = { recall: recallMemory, enabled: features.isOn('memories') }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      const tag = t?.tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || t?.isContentEditable) return
      if (!recallHotkeyRef.current.enabled) return
      const target = hotkeyRecallTarget(e, memoriesStore.get())
      if (!target) return
      e.preventDefault()
      recallHotkeyRef.current.recall(target)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  // Opt-in net reminders: a light 30 s poll that fires ONE prominent toast per net
  // occurrence, a per-net lead time before it starts, with a Tune button that recalls
  // the memory. Only nets the operator explicitly enabled (net.alertEnabled) are checked
  // — never a firehose. Reading the store directly avoids a re-render on every tick.
  const firedNetRemindersRef = useRef<Set<string>>(new Set())
  useEffect(() => {
    const check = () => {
      const now = Date.now()
      const due = dueNetReminders(memoriesStore.get().memories, now)
      const fired = firedNetRemindersRef.current
      for (const r of due) {
        const key = reminderKey(r)
        if (fired.has(key)) continue
        fired.add(key)
        pushToast(
          `Net ${untilPhrase(r.startMs, now)}: ${r.memory.name} — ${r.memory.rxMhz.toFixed(3)} ${r.memory.mode}`,
          'info',
          300_000,
          { prominent: true, action: () => recallMemory(r.memory), actionLabel: 'Tune' },
        )
      }
    }
    check()
    const id = window.setInterval(check, 30_000)
    return () => window.clearInterval(id)
  }, [recallMemory])

  // Click-to-work ANY needed spot (CW / Phone / Digital): N1MM-style single click that
  // changes the band, mode, AND frequency to exactly the spot's — in ONE atomic backend call
  // (`workSpot`) so the rig can never end up in the new mode at the old dial (no wrong-mode
  // flash) and the UI never sees a half-applied mode/freq state. Then open the matching
  // cockpit and — for CW/Phone — hand it the callsign to prefill the log. A need with no
  // Point the antenna rotator at a needed call (great-circle bearing from your grid).
  const handlePointAntenna = useCallback(async (call: string) => {
    try {
      const bearing = await pointRotatorAtCall(call)
      pushToast(`↗ Pointing antenna to ${Math.round(bearing)}° (${call})`, 'success', 3000)
    } catch (e) {
      pushToast(typeof e === 'string' ? e : `Couldn't point the antenna at ${call}`, 'error', 4000)
    }
  }, [])

  // resolvable frequency at all falls back to a plain band QSY.
  const handleWorkNeeded = useCallback(
    (alert: NeedAlert) => {
      const t = workTarget(alert, bandPlan)
      if (!t) {
        handleQsy(alert.band)
        return
      }
      // The Needed board now lists ALL modes (W1), but the CW/Phone cockpits are opt-in
      // features. If the target cockpit is disabled, don't navigate into a hidden view
      // (that dumped the operator on the landing page) — just QSY the rig to the spot.
      if ((t.view === 'cw' && !cwEnabled) || (t.view === 'phone' && !phoneEnabled)) {
        handleQsy(alert.band)
        return
      }
      // RTTY is a Digital submode with an opt-in cockpit. When it's disabled, don't strand
      // the operator on a hidden view — QSY to the spot's EXACT frequency (RTTY spots always
      // carry one) so they can still work it from wherever they are.
      if (t.view === 'rtty' && !rttyEnabled) {
        handleQsy(alert.band, t.freqMhz)
        return
      }
      // 'operate' is the digital cockpit, so its operating mode is 'digital'. RTTY passes
      // through as 'rtty' — the backend + [view] rig-mode effect apply the RTTY policy.
      const opMode: 'digital' | 'phone' | 'cw' | 'rtty' =
        t.view === 'operate' ? 'digital' : t.view
      void (async () => {
        // A digital spot carries its protocol in alert.mode — the FT8/FT4 tier rides the
        // SAME atomic workSpot call, else clicking an FT4 spot QSYs but leaves the decoder
        // on FT8. It used to be a separate set_tier call first, and in the gap between the
        // two the radio loop could command the tier's DEFAULT dial (FT8 14.074 → FT4
        // 14.080) to the rig before the spot's exact frequency — the "hits a default
        // first, then switches" double-retune, whose stale read-back could race the
        // correction on slow serial and leave the wrong dial standing (operator report,
        // 2026-08-09). tierRef syncs from the returned snapshot (setSnap → link.tier).
        let tier: 'FT8' | 'FT4' | undefined
        if (opMode === 'digital') {
          const m = alert.mode?.toUpperCase()
          if ((m === 'FT4' || m === 'FT8') && tierRef.current !== m) tier = m
        }
        const s = await withErrorToast(
          () => workSpot(opMode, t.freqMhz, t.band, t.call, tier),
          `Could not work ${t.call} — check CAT`,
        )
        // On failure DON'T navigate or poison the guard ref — the backend made no change
        // (atomic), so the view-effect can still apply the mode on a later nav.
        if (!s) return
        setSnap(s)
        // Keep the rig-mode effect's guard in sync so it doesn't re-fire on the nav.
        lastOpModeRef.current = opMode
        // CW/Phone log forms consume a prefill; the digital + RTTY cockpits don't (digital
        // auto-sequences on a decode double-click, RTTY has its own net picker) — just the QSY.
        if (t.view === 'cw' || t.view === 'phone') {
          setPendingWork({ call: t.call, view: t.view, ts: Date.now() })
        }
        setView(t.view)
        pushToast(`▶ ${t.call} — ${alert.mode} ${t.band}, ready to log`, 'success', 4000)
      })()
    },
    [bandPlan, handleQsy, cwEnabled, phoneEnabled, rttyEnabled],
  )

  // Work a spot double-clicked on the MAP — the same atomic path as the Needed
  // board (workSpot → rig jumps band+mode+freq, cockpit opens). The source-reported
  // mode routes the cockpit: CW→CW, SSB/FM→Phone, FT8/unknown→Digital.
  const handleWorkMapSpot = useCallback(
    (t: { call: string; band: string; mode: string | null; freqMhz: number | null }) => {
      handleWorkNeeded({
        call: t.call,
        entity: '',
        band: t.band,
        zone: 0,
        tags: [],
        priority: 0,
        headline: '',
        // Keep FT4/FT8 specific so the tier-switch guard fires (same as handleWorkSpot);
        // everything else collapses to its class for cockpit routing.
        mode: t.mode === 'FT4' || t.mode === 'FT8' ? t.mode : modeClassOf(t.mode),
        freqMhz: t.freqMhz,
      })
    },
    [handleWorkNeeded],
  )
  // The chase toast's "Work" action — assigned via ref because the prop poller
  // (defined above, deps []) closes over its startup scope. Routes by the
  // expedition's announced modes like every other DXpedition work path (a
  // CW-only op must open the CW cockpit at the CW activity freq, not FT8).
  workDxpedRef.current = (c: WorkableCard) =>
    handleWorkMapSpot({ call: c.call, band: c.band, mode: dxpedWorkMode(c.modes), freqMhz: null })

  // Hunt a POTA/SOTA activator: tag the next QSO with the park/summit reference AND
  // QSY to the spot's exact frequency — the same atomic workSpot path as handleWorkNeeded.
  // PotaSotaView calls setHuntTarget itself (and hands us the fresh snap via onSnap),
  // then calls this handler which only needs to do the QSY + navigation + toast.
  const handleHuntSpot = useCallback(
    (arg: OtaSpotClickArg) => {
      // Build a minimal NeedAlert-shaped object so we can reuse handleWorkNeeded's
      // existing workSpot → cockpit-open → pendingWork path exactly.
      handleWorkNeeded({
        call: arg.call,
        entity: '',
        band: arg.band,
        zone: 0,
        tags: [],
        priority: 0,
        headline: '',
        mode: arg.modeClass,
        freqMhz: arg.freqMhz,
      })
      // (handleWorkNeeded toasts the QSY itself — one toast per action.)
    },
    [handleWorkNeeded],
  )

  // Work a raw spot from the Spots panel — synthesize a minimal NeedAlert so we reuse
  // handleWorkNeeded's workSpot → cockpit-open path (QSY to the spot's exact freq + mode).
  // Work a spot from a COCKPIT'S OWN band strip: QSY to it and stay put.
  //
  // `handleWorkSpot` (below) routes to the cockpit matching the SPOT's mode — correct for the
  // Needed board, which lists every mode, but wrong from inside a cockpit. Phone's Band Activity
  // shows the whole band, so a CW spot at the band bottom navigated the operator out of Phone and
  // the entire view unmounted. Operator report 2026-07-25: "the whole window with those two
  // things gets removed from the primary window and they disappear" — that was the Phone cockpit
  // being replaced, not a layout fault. Three layout fixes chased it before this was found.
  // ⚠️ TAKES THE COCKPIT'S OWN MODE. This used to read the mode off the SPOT
  // (`s.mode === 'CW' ? 'cw' : … : 'digital'`), which meant clicking an FT8 spot from the
  // Phone cockpit — and a cluster feed is mostly FT8 spots — commanded the rig into DATA-USB
  // while deliberately leaving the operator on the Phone screen. On an FT-991A that is D-U:
  // rear-panel audio in place of the microphone. It also disarmed the mic, because
  // `work_spot_split` sets operating_mode=Digital and the next context change then fails
  // `halt_tx_for_context_change`'s Phone|Cw|Rtty restore gate and lands a plain `halt_tx`.
  // So the operator sat on the Phone screen with a data-mode rig, a mic that was out of
  // circuit, and a PTT that did nothing (#81, path B; #80 is the same fault from the nav side).
  // "QSY to it and stay put" means stay in YOUR mode — that is the whole difference between
  // this and `handleWorkSpot`, which navigates. Working it on the spot's mode is a deliberate
  // act: go to that cockpit.
  const handleWorkSpotHere = useCallback(
    (cockpit: 'phone' | 'cw') => (s: SpotRow) => {
      const kind = cockpit
      void withErrorToast(
        () => workSpot(kind as 'digital' | 'phone' | 'cw', s.freqMhz, s.band, s.call),
        `Could not work ${s.call} — check CAT`,
      ).then((snap) => {
        if (!snap) return
        setSnap(snap)
        pushToast(`▶ ${s.call} — ${s.band} ${s.freqMhz.toFixed(3)} MHz`, 'success', 3000)
      })
    },
    [],
  )
  const workSpotHerePhone = useMemo(() => handleWorkSpotHere('phone'), [handleWorkSpotHere])
  const workSpotHereCw = useMemo(() => handleWorkSpotHere('cw'), [handleWorkSpotHere])

  const handleWorkSpot = useCallback(
    (s: SpotRow) => {
      handleWorkNeeded({
        call: s.call,
        entity: s.entity,
        band: s.band,
        zone: s.zone,
        tags: [],
        priority: 0,
        headline: '',
        // Forward the SPECIFIC digital submode (FT4/FT8) so handleWorkNeeded's tier-switch
        // guard fires — else clicking an FT4 spot QSYs but leaves the decoder on FT8. The
        // frequency-class `s.mode` ('Digital') never matched that guard, so it was dead.
        mode: s.submode === 'FT4' || s.submode === 'FT8' ? s.submode : s.mode,
        freqMhz: s.freqMhz,
      })
    },
    [handleWorkNeeded],
  )

  // Stop a QSO recording from anywhere (the global REC badge in the TopBar), so an active
  // recording started in the Phone cockpit can be stopped without navigating back.
  const handleStopRecording = useCallback(() => {
    void stopQsoRecording()
      .then((s) => s && setSnap(s))
      .catch(() => {})
  }, [])

  const handleSetMode = useCallback((mode: ModeRequest) => {
    void withErrorToast(() => apiSetMode(mode), 'Could not switch mode').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleQsoResend = useCallback(() => {
    void withErrorToast(() => apiQsoResend(), 'Could not resend').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleQsoFreetext = useCallback((text: string) => {
    void withErrorToast(() => apiQsoFreetext(text), 'Could not send free text').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleLogCurrent = useCallback(() => {
    void withErrorToast(() => apiLogCurrentQso(), 'Could not log QSO').then((s) => {
      if (s) {
        setSnap(s)
        pushToast('Logged QSO', 'success', 2500)
        noteLoggedForDxClear()
        refreshNeeds() // drop the just-worked station from the roster/needs immediately
        // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
        // (every log path, auto-log included); outcomes toast via uploadTick.
      }
    })
  }, [noteLoggedForDxClear, refreshNeeds])

  // Selecting a view from the nav. QSO / Field Day also request the backend mode
  // (defaulting to the "run" / "chat" role); Settings are pure UI
  // screens that leave the operating mode unchanged.
  // Where a Settings deep link asked the panel to land. Cleared on the way OUT of Settings so a
  // later plain visit (the rail gear) opens on Station as it always has, rather than returning to
  // wherever the last pointer sent the operator.
  const [settingsTarget, setSettingsTarget] = useState<string | undefined>(undefined)

  const handleView = useCallback(
    (next: View) => {
      setView(next)
      if (next !== 'settings') setSettingsTarget(undefined)
      // Passive-first: entering QSO / Field Day starts in Search-&-Pounce
      // (listen + answer), never auto-calling CQ. The operator hits "Call CQ" /
      // "Running" in the panel to start transmitting.
      if (next === 'chat') handleSetMode('chat')
      else if (next === 'fieldDay') handleSetMode('fieldday-sp')
    },
    [handleSetMode],
  )

  /**
   * Open Settings AT a specific place. `where` is anything `resolveTarget` understands: a section
   * id (`'audio'`), a tab, a tab label, a legacy tab name, or a prose path
   * (`'Settings ▸ Radio ▸ Audio'`).
   *
   * This is what turns the app's ~228 prose pointers into something that takes the operator
   * there. Passing an unrecognised string is safe — the panel opens on its default tab, which is
   * exactly what happened before this existed.
   */
  const openSettingsAt = useCallback(
    (where: string) => {
      setSettingsTarget(where)
      setView('settings')
    },
    [],
  )

  const handleTier = useCallback((t: Tier) => {
    void withErrorToast(() => apiSetTier(t), 'Could not change tier').then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Pick a Digital sub-mode from the rail. Tempo → the TempoFast/TempoDeep free-text cockpit
  // (reuse the workspace bind). Digital → the weak-signal cockpit, PRESERVING the
  // last FT8/FT4 tier (the top bar's pills own that choice now — the rail button
  // must not yank an FT4 operator back to FT8): bind the dx workspace (tier + QSO
  // mode) THEN re-assert the tier, sequentially, so set_area's default-FT8 can't
  // race past it.
  const handleDigitalMode = useCallback(
    (m: DigitalMode) => {
      if (m === 'tempo') {
        handleWorkspace('msg')
        return
      }
      if (m === 'sstv') {
        // SSTV is RX-first — entry asserts nothing on the rig; just navigate.
        setView(m)
        return
      }
      if (m === 'aprs') {
        // APRS is RX-first — monitoring asserts nothing on the rig (a beacon is an explicit send).
        setView(m)
        return
      }
      if (m === 'rtty') {
        // Navigate; the [view] rig-mode effect asserts the RTTY policy (FSK
        // backend → rig RTTY mode, AFSK → LSB) and re-homes to the band's RTTY
        // watering hole on a genuine mode change — the same path as CW/Phone.
        setView(m)
        return
      }
      // PRESERVE the operator's tier if it is already one this screen owns.
      // This used to read `tierRef.current === 'FT4' ? 'FT4' : 'FT8'`, which
      // silently forced anything else back to FT8 — so selecting Q65 (or any of
      // the receive-only modes) and then re-entering Digital dropped the choice
      // on the floor, with no error and no visible cause. Only the Tempo chat
      // tiers, which this screen does not own, still fall back to FT8.
      const wantTier: Tier = OPERATE_TIERS.includes(tierRef.current)
        ? tierRef.current
        : 'FT8'
      setArea('dx')
      try {
        localStorage.setItem('nexus.workspace', 'dx')
      } catch {
        /* ignore */
      }
      setView('operate')
      // The codec tier (FT8/FT4) is INDEPENDENT of the rig's CAT mode — switching tiers does
      // NOT command the Yaesu into DATA-U. Assert the digital rig mode explicitly here:
      // clicking a Digital sub-mode while already on the Operate screen doesn't change `view`,
      // so the rig-mode view-effect never fires and the rig would stay in whatever (SSB/CW)
      // mode it was last left in. follow_freq=false keeps the current digital frequency.
      lastOpModeRef.current = 'digital'
      void setOperatingMode('digital', false)
        .then((s) => s && setSnap(s))
        .catch(() => {})
      // Bind the dx workspace, THEN set the exact tier — each wrapped so a backend
      // failure surfaces a toast (matching handleTier) instead of failing silently
      // or leaving an unhandled rejection.
      void withErrorToast(() => apiSetArea('dx'), 'Could not switch to Digital')
        .then((s) => {
          if (s) setSnap(s)
          return withErrorToast(() => apiSetTier(wantTier), 'Could not change tier')
        })
        .then((s) => {
          if (s) setSnap(s)
        })
    },
    [handleWorkspace],
  )

  const handleSourceChange = useCallback((k: SourceKind) => {
    // Companion bind can fail (port busy) → withErrorToast surfaces it and the
    // backend stays on the previous source.
    void withErrorToast(() => apiSetSource(k), 'Could not switch signal source').then((s) => {
      if (s) {
        setSnap(s)
        // Confirm the switch even when no decodes follow (e.g. Companion idle).
        pushToast(
          k === 'companion'
            ? `Source: ${s.radio.sourceLabel} — listening for WSJT-X/JTDX/MSHV on :2237`
            : `Source: ${s.radio.sourceLabel}`,
          'success',
          3500,
        )
      }
    })
  }, [])

  const handleSettingsSaved = useCallback(() => {
    getSnapshot().then(setSnap).catch(() => {})
    reloadSettings()
  }, [reloadSettings])

  const handleDismissOnboarding = useCallback(() => {
    localStorage.setItem(ONBOARD_KEY, '1')
    setOnboardDismissed(true)
  }, [])

  // Merge the wizard's station/rig draft into settings with ONE set_settings
  // write (apply_settings is heavyweight — never per-field). No-ops on an
  // empty draft. Shared by Test-CAT (write, then probe) and the final apply.
  const applyWizardDraft = useCallback(async (draft: WizardDraft): Promise<void> => {
    if (Object.keys(draft).length === 0) return
    const current = await getSettings()
    const snap = await apiSetSettings({ ...current, ...draft })
    if (snap) setSnap(snap)
    setSettings({ ...current, ...draft })
  }, [])

  const handleWizardTestCat = useCallback(
    async (draft: WizardDraft): Promise<CatTestResult> => {
      // The radio loop reconfigures from SAVED settings (same rule as the
      // Settings Test button) — persist the draft first, then probe.
      await applyWizardDraft(draft)
      return testCat()
    },
    [applyWizardDraft],
  )

  const handleWizardApply = useCallback(
    (ids: ProfileId[], landing: View, modes: FeatureId[], license: string, draft: WizardDraft) => {
      // SEQUENCED, not concurrent: set_settings writes the WHOLE struct
      // (licenseClass included), so a concurrent set_license_class would be
      // reverted by the stale class riding in the merge (review catch — the
      // wizard's own TX-lockout promise silently undone). Draft first, class
      // second; failures surface instead of a false-success close.
      void applyWizardDraft(draft)
        .then(() => setLicenseClass(license))
        .then((s) => s && setSnap(s))
        .catch((e) =>
          pushToast(
            `Setup didn't fully save: ${e instanceof Error ? e.message : e} — check Settings`,
            'error',
            0,
          ),
        )
      // Goal profiles + the chosen operating modes (CW/Phone) force-enabled on top.
      features.applyProfiles(ids, modes)
      setView(landing)
      markWizardSeen()
      setShowWizard(false)
      setWizardGen((g) => g + 1)
    },
    [features.applyProfiles, applyWizardDraft],
  )

  const handleWizardSkip = useCallback(() => {
    markWizardSeen()
    setShowWizard(false)
    setWizardGen((g) => g + 1)
  }, [])

  // NOTE: everything from here down to the `!snap` early return is snap-INDEPENDENT
  // and — critically for the hooks below — must run on EVERY render (rules of hooks:
  // no hook may live after the early return, or React unmounts the whole app once
  // snap loads and the hook count changes).

  // Field Day visibility is owned by the persisted master switch (settings.fdActive),
  // NOT the standalone feature flag — so the two can never diverge. Fold it into the
  // enabled map that the nav and the redirect guard below both read: master off →
  // Field Day is invisible (nav item hidden, view redirects away); master on → visible.
  const fdActive = settings?.fdActive === true
  const navEnabled: Record<FeatureId, boolean> = { ...features.enabled, fieldDay: fdActive }
  const isViewEnabled = (v: View): boolean => navEnabled[v as FeatureId] !== false

  // Defense in depth: if the current view's feature got disabled (e.g. toggled
  // off in Settings while viewing it), fall back to the profile's landing view.
  // The nav already hides disabled sections; this guards a stale selection. Never
  // land on a disabled view (e.g. a Contest profile whose landing is Field Day while
  // the master is off) → operate.
  const fallbackView: View = isViewEnabled(features.landing) ? features.landing : 'operate'
  const effectiveView: View = isViewEnabled(view) ? view : fallbackView
  // Where the crash panel's escape button goes. The landing view normally — unless the
  // landing view is the one that just crashed (a vhf profile lands on Connect), in which
  // case Operate: it is a core section, so it can never be the disabled one.
  const crashEscape: View = fallbackView === effectiveView ? 'operate' : fallbackView

  // ── Eyes-free operating (a11y Phase A) — hooks BEFORE the `!snap` return ──
  // Per-view window title + a polite "now on X" announcement (navigation is
  // otherwise silent to a screen reader).
  useEffect(() => {
    const label = featureById(effectiveView)?.label ?? 'Settings'
    document.title = `${label} — Nexus`
    announce(label)
  }, [effectiveView])
  // TX state → assertive announce + opt-in earcon. Snap-safe (null pre-connect).
  const prevTxRef = useRef<boolean | null>(null)
  const txNow = snap?.radio.transmitting ?? false
  useEffect(() => {
    if (!snap) return
    if (prevTxRef.current !== null && prevTxRef.current !== txNow) {
      announce(txNow ? 'Transmitting' : 'Receiving', { assertive: true })
      if (settings?.soundTxState) txEarcon(txNow)
    }
    prevTxRef.current = txNow
  }, [snap, txNow, settings?.soundTxState])

  if (!snap) {
    return (
      <div className="app loading">
        <span>Connecting to Nexus…</span>
      </div>
    )
  }

  const macros = settings?.macros ?? DEFAULT_MACROS

  const activeConversation =
    snap.conversations.find((c) => c.peer === activePeer) ?? null

  // displayed tier is the authoritative link tier from the snapshot
  const tier = snap.link.tier

  // First-run nudge: callsign unset / still the placeholder, and not dismissed.
  const needsOnboarding =
    !onboardDismissed &&
    effectiveView !== 'settings' &&
    snap.mycall.trim() === '' // fresh install (the default callsign is empty)

  // The Tempo (chat) roster represents who's on the TEMPO protocol — so it shows only
  // stations last heard on TempoFast, not the FT8/FT4 stations that share the engine's single
  // roster. Every other view (Operate, Field Day) shows the full roster.
  const rosterStations =
    effectiveView === 'chat' ? snap.stations.filter((s) => s.tier === 'TempoFast') : snap.stations
  // Two roster surfaces off one component: the Tempo chat roster keeps the long
  // presence retention (store-and-forward delivery needs it); the FT cockpit's
  // Stations panel flushes after 3 missed decode cycles (the Call Roster rule).
  const stationsPanel = (
    <StationList
      stations={rosterStations}
      myGrid={snap.mygrid}
      currentSlot={snap.radio.slot}
      activePeer={activePeer}
      unreadByPeer={unreadByPeer}
      needByCall={needByCall}
      needAlertsByCall={needAlertsByCall}
      band={snap.radio.band}
      feedMode={tier}
      onSelect={handleSelect}
      onCall={handleWorkStation}
      conversations={snap.conversations}
      onArchive={handleArchive}
      bandActive={activePeer === '*'}
      bandUnread={bandUnread}
      onSelectBand={() => handleSelect('*')}
    />
  )
  const operateStationsPanel = (
    <StationList
      stations={rosterStations}
      myGrid={snap.mygrid}
      currentSlot={snap.radio.slot}
      activePeer={activePeer}
      unreadByPeer={unreadByPeer}
      needByCall={needByCall}
      needAlertsByCall={needAlertsByCall}
      band={snap.radio.band}
      feedMode={tier}
      onSelect={handleSelect}
      onCall={handleWorkStation}
      conversations={snap.conversations}
      onArchive={handleArchive}
      bandActive={activePeer === '*'}
      bandUnread={bandUnread}
      onSelectBand={() => handleSelect('*')}
      dropAfterCycles={3}
    />
  )

  // Pane resize: dragging a splitter writes the rail-width CSS var directly each
  // frame (no React re-render), then commits (clamp + persist) on pointer-up.
  // One Pointer-Events path covers mouse, touch, and pen.
  const startResize =
    (side: 'left' | 'right') => (e: React.PointerEvent<HTMLDivElement>) => {
      const el = layoutRef.current
      if (!el) return
      e.preventDefault()
      ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
      const rect = el.getBoundingClientRect()
      const GAP = 12 // .layout padding; keeps the rail edge under the pointer
      const root = document.documentElement.style
      document.body.classList.add('resizing')
      const widthFor = (clientX: number) =>
        side === 'right' ? rect.right - GAP - clientX : clientX - rect.left - GAP
      const move = (ev: PointerEvent) => {
        const w = widthFor(ev.clientX)
        root.setProperty(side === 'right' ? '--right-rail-w' : '--left-rail-w', `${
          side === 'right' ? clampRight(w) : clampLeft(w)
        }px`)
      }
      const up = (ev: PointerEvent) => {
        if (side === 'right') commitRight(widthFor(ev.clientX))
        else commitLeft(widthFor(ev.clientX))
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        document.body.classList.remove('resizing')
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    }

  const waterfallRail = (
    <aside className="right-rail panel">
      <Waterfall
        transmitting={snap.radio.transmitting}
        rxOffsetHz={snap.radio.rxOffsetHz}
        txOffsetHz={snap.radio.txOffsetHz}
        theme={theme}
        onTune={handleTune}
        paletteScope={FT_PALETTE_SCOPE}
      />
      <OperateDecodes
        decodes={snap.recentDecodes}
        slot={snap.radio.slot}
        rxOffsetHz={snap.radio.rxOffsetHz}
        band={snap.radio.band}
        tier={snap.link.tier}
        harqRescues={snap.harqRescues}
        onCall={handleCall}
        needAlertsByCall={needAlertsByCall}
        myGrid={snap.mygrid}
        compact
        title="Band Activity — heard on the band"
      />
      <LinkPill link={snap.link} radio={snap.radio} />
    </aside>
  )

  // Three-pane workspace: stations | center | waterfall, with drag splitters
  // between each. The waterfall lives in the right rail at every width; the rail
  // only narrows on the sm/xs collapse. (This used to claim a `data-layout`
  // top-strip alternative — there was no writer and no CSS behind it.)
  const threePane = (center: JSX.Element, header?: JSX.Element) => (
    <main
      className={`layout${header ? ' has-tempo-header' : ''}`}
      data-three-pane
      ref={layoutRef}
    >
      {header && <div className="grid-header">{header}</div>}
      <div className="grid-stations">{stationsPanel}</div>
      <div
        className="pane-splitter left"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize stations panel (double-click to reset)"
        onPointerDown={startResize('left')}
        onDoubleClick={resetWidths}
      />
      <div className="grid-center">{center}</div>
      <div
        className="pane-splitter right"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize waterfall pane (double-click to reset)"
        onPointerDown={startResize('right')}
        onDoubleClick={resetWidths}
      />
      <div className="grid-waterfall">{waterfallRail}</div>
    </main>
  )

  let workspace: JSX.Element | null
  switch (effectiveView) {
    case 'fieldDay':
      workspace = threePane(
        <FieldDayView fieldDay={snap.fieldDay} onSetMode={handleSetMode} />,
      )
      break
    case 'logbook':
      workspace = (
        <main className="layout single">
          <Logbook
            defaultBand={snap.radio.band}
            defaultFreqMhz={snap.radio.dialMhz}
            // Seed manual entries from the mode the operator was ACTUALLY running —
            // a hand-logged SSB/CW QSO must not default to the digital codec tier
            // (that silently wrote "FT8" on phone contacts and corrupted awards).
            defaultMode={
              lastOpModeRef.current === 'phone'
                ? settings?.phoneMode?.toLowerCase() === 'fm'
                  ? 'FM'
                  : 'SSB'
                : lastOpModeRef.current === 'cw'
                  ? 'CW'
                  : lastOpModeRef.current === 'rtty'
                    ? 'RTTY'
                    : snap.link.tier
            }
          />
        </main>
      )
      break
    case 'needed':
      workspace = (
        <NeededPanel
          // FULL un-gated list: the board's own per-mode toggles decide what shows, so a
          // disabled CW/Phone *feature* no longer hides those needs here (the operator
          // controls mode visibility in the Needed filter bar instead).
          alerts={needAlerts}
          bandPlan={bandPlan}
          selectedCall={activePeer}
          myGrid={snap.mygrid}
          onQsy={(a) => handleQsy(a.band, a.freqMhz ?? undefined)}
          onSelect={handleSelect}
          onWork={handleWorkNeeded}
          onPoint={
              (settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim()
                ? handlePointAntenna
                : undefined
            }
          onPopOut={() => void openPanelWindow('needed')}
          onOpenSettings={openSettingsAt}
          phoneSource={
            feedHealth
              ? {
                  status: feedHealth.phoneCluster,
                  host: feedHealth.phoneClusterHost,
                  spotsSeen: feedHealth.phoneSpotsSeen,
                }
              : null
          }
        />
      )
      break
    case 'spots':
      workspace = (
        <SpotsPanel
          spots={allSpots}
          bandPlan={bandPlan}
          selectedCall={activePeer}
          myGrid={snap.mygrid}
          onSelect={handleSelect}
          onWork={handleWorkSpot}
        />
      )
      break
    case 'awards':
      // Awards + Journey combined: one section, tabbed (Journey + Official Awards).
      workspace = (
        <AwardsJourney
          showGamification={features.isOn('gamification')}
          onOpenSettings={openSettingsAt}
        />
      )
      break
    case 'stats':
      // Descriptive logbook analytics — the log sliced by band/mode/year/hour/entity.
      workspace = <StatsView />
      break
    case 'cw':
      workspace = (
        <CwCockpit
          pitchHz={settings?.cwPitchHz ?? 600}
          wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
          snap={snap}
          theme={theme}
          pendingWork={pendingWork?.view === 'cw' ? pendingWork : null}
          onConsumeWork={() => setPendingWork(null)}
          onSnap={setSnap}
          fieldDay={snap.fieldDay}
          spots={allSpots}
          needByCall={needByCall}
          typeByCall={typeByCall}
          onWorkSpot={workSpotHereCw}
          onRecallMemory={isViewEnabled('memories') ? recallMemory : undefined}
          onOpenMemories={isViewEnabled('memories') ? () => setView('memories') : undefined}
          onOpenSettings={openSettingsAt}
          panels={cwPanels}
        />
      )
      break
    case 'phone':
      workspace = (
        <PhoneCockpit
          snap={snap}
          panels={phonePanels}
          theme={theme}
          pendingWork={pendingWork?.view === 'phone' ? pendingWork : null}
          onConsumeWork={() => setPendingWork(null)}
          onSnap={setSnap}
          fieldDay={snap.fieldDay}
          phoneMode={settings?.phoneMode}
          wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
          spots={allSpots}
          needByCall={needByCall}
          typeByCall={typeByCall}
          onWorkSpot={workSpotHerePhone}
          onRecallMemory={isViewEnabled('memories') ? recallMemory : undefined}
          onOpenMemories={isViewEnabled('memories') ? () => setView('memories') : undefined}
          onOpenSettings={openSettingsAt}
        />
      )
      break
    case 'pota':
      workspace = (
        <main className="layout single">
          <PotaSotaView
            snap={snap}
            onHunt={handleHuntSpot}
            onSnap={setSnap}
          />
        </main>
      )
      break
    case 'settings':
      workspace = (
        <main className="layout single">
          <SettingsPanel
            key={`sp-wiz${wizardGen}`}
            onSaved={handleSettingsSaved}
            target={settingsTarget}
            radio={snap.radio}
            activeRadioId={snap.activeRadioId}
            onProveTx={handleProveTx}
            scale={scale}
            scaleMode={scaleMode}
            scaleCap={scaleCap}
            onScaleModeChange={setScaleMode}
            onScaleCapChange={setScaleCap}
            density={density}
            onDensityChange={setDensity}
            onResetLayout={resetWidths}
            features={features}
            onRerunWizard={() => setShowWizard(true)}
            theme={theme}
            onThemeChange={setTheme}
          />
        </main>
      )
      break
    case 'operate':
      // The Operate cockpit is NOT rendered here — it stays permanently mounted in
      // a persistent host below (so its waterfall + Band Activity keep accumulating
      // in the background across navigation). This case renders nothing in the slot.
      workspace = null
      break
    case 'rtty':
    case 'sstv':
    case 'aprs':
      // Same keep-alive pattern as Operate: RTTY's decoded stream, SSTV's
      // always-armed VIS receiver, and APRS's decode list must survive
      // navigation, so all three live in persistent hosts below. Nothing in the slot.
      workspace = null
      break
    case 'connect':
      workspace = (
        <ConnectView
          myGrid={settings?.mygrid ?? ''}
          theme={theme}
          stations={snap?.stations ?? []}
          prop={prop}
          selectedCall={activePeer}
          onSelectCall={handleMapSelect}
          needByCall={needByCall}
          onWorkSpot={handleWorkMapSpot}
          needAlerts={visibleAlerts}
          // Rotor is configured EITHER by picking a model (Nexus launches the
          // bundled rotctld) OR by the advanced external host — host-only was
          // the pre-rotctld gate and silently disabled point-at for model users.
          onPoint={
            (settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim()
              ? handlePointAntenna
              : undefined
          }
          onSelectSat={
            features.isOn('sats')
              ? (name) => {
                  setSatFocus(name)
                  setView('sats')
                }
              : undefined
          }
          onPopOut={() => void openPanelWindow('connect')}
        />
      )
      break
    case 'dxped':
      workspace = (
        <main className="layout single">
          <DxpeditionsView
            snap={prop}
            onWorkSpot={handleWorkMapSpot}
            onShowOnMap={(call) => {
              // Hand off to Connect with the expedition selected on the map.
              handleMapSelect(call)
              setView('connect')
            }}
            onPopOut={() => void openPanelWindow('dxped')}
          />
        </main>
      )
      break
    case 'sats':
      workspace = (
        <main className="layout single">
          <SatellitesView
            focusSat={satFocus}
            snap={snap}
            onPopOut={() => void openPanelWindow('sats')}
          />
        </main>
      )
      break
    case 'memories':
      // A manager view — never touches the rig on entry; only an explicit
      // Tune (recallMemory) retunes + switches cockpit.
      workspace = (
        <main className="layout single">
          <MemoriesView
            onPopOut={() => void openPanelWindow('memories')}
            myGrid={settings?.mygrid ?? ''}
            dialMhz={snap.radio.dialMhz}
            dialMode={
              lastOpModeRef.current === 'cw'
                ? 'CW'
                : lastOpModeRef.current === 'phone'
                  ? settings?.phoneMode === 'fm'
                    ? 'FM'
                    : snap.radio.sideband || 'USB'
                  : // The tier's own name, not a guess. This was
                    // `tier === 'FT4' ? 'FT4' : 'FT8'`, which labelled a memory
                    // saved on Q65/WSPR/JT65 as FT8.
                    OPERATE_TIERS.includes(tier)
                    ? tier
                    : 'FT8'
            }
            onRecall={recallMemory}
          />
        </main>
      )
      break
    case 'program':
      // The programming workbench never touches the rig on entry (no rig-mode
      // policy entry needed — global sections leave the VFO alone).
      workspace = (
        <main className="layout single">
          <RadioProgView myGrid={settings?.mygrid ?? ''} catOk={snap.radio.catOk === true} />
        </main>
      )
      break
    case 'chat':
    default:
      workspace = (
        <>
          {threePane(
            <Conversation
              key={activePeer ?? 'launchpad'} // remount per peer: open pinned to the newest message, never inherit the last peer's scroll offset
              conversation={activeConversation}
              peer={activePeer}
              radio={snap.radio}
              mode={snap.mode}
              fieldDay={snap.fieldDay}
              macros={macros}
              onSend={handleSend}
              onResend={handleResend}
              onBroadcast={handleBroadcast}
              onCallCq={handleCallCq}
              beaconOn={snap.radio.beacon ?? false}
              onToggleBeacon={handleToggleBeacon}
              mycall={snap.mycall}
              mygrid={snap.mygrid}
              // Roam (coordinated QSY) lives INSIDE Tempo now: the chip toggles
              // it, the gear opens the full panel (was its own rail section).
              roamEnabled={snap.qsy?.enabled ?? false}
              roamStatus={snap.qsy?.enabled ? (snap.qsy.paused ? 'paused' : (snap.qsy.current ?? 'on')) : undefined}
              onToggleRoam={() =>
                void withErrorToast(
                  () => apiQsySetEnabled(!(snap.qsy?.enabled ?? false)),
                  'Could not toggle Roam',
                ).then((s) => s && setSnap(s))
              }
              onRoamSettings={() => setRoamOpen(true)}
            />,
            <TempoHeader
              snap={snap}
              onSnap={setSnap}
              tier={tier}
              onTierChange={handleTier}
              bandPlan={bandPlan}
              onSetFrequency={handleSetFrequency}
              onSetTxLevel={handleSetTxLevel}
              wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
              onToggleCqRun={handleToggleCqRun}
              onResumeCqRun={handleResumeCqRun}
            />,
          )}
          {roamOpen && (
            <div className="roam-modal" role="dialog" aria-label="Roam settings">
              <div className="roam-modal-body">
                <button
                  type="button"
                  className="roam-modal-close"
                  onClick={() => setRoamOpen(false)}
                  aria-label="Close Roam settings"
                >
                  ✕
                </button>
                <RoamPanel
                  qsy={snap.qsy ?? null}
                  channels={settings?.qsySet ?? []}
                  cadence={settings?.qsyCadence ?? 6}
                  bandPlan={bandPlan}
                  activePeer={activePeer}
                  onSnap={setSnap}
                  onReloadSettings={reloadSettings}
                />
              </div>
            </div>
          )}
        </>
      )
      break
  }

  return (
    <div className="app">
      <TopBar
        mycall={snap.mycall}
        mygrid={snap.mygrid}
        radio={snap.radio}
        radios={snap.radios}
        radioPegged={snap.radioPegged}
        onSetActiveRadio={handleSetActiveRadio}
        onSetPegLock={handleSetPegLock}
        link={snap.link}
        bandPlan={bandPlan}
        onSetFrequency={handleSetFrequency}
        onSetTxEnabled={handleSetTxEnabled}
        onSetTune={handleSetTune}
        onHaltTx={handleHaltTx}
        onSetTxEven={handleSetTxEven}
            onSetTxCycleAuto={handleSetTxCycleAuto}
        onSetHoldTxFreq={handleSetHoldTxFreq}
        onStopRecording={handleStopRecording}
        hideTxControls={effectiveView === 'operate'}
        hideFrequencyControl={
          effectiveView === 'phone' ||
          effectiveView === 'cw' ||
          effectiveView === 'operate' ||
          effectiveView === 'chat' ||
          effectiveView === 'rtty' ||
          effectiveView === 'sstv' ||
          effectiveView === 'aprs' ||
          // Satellites: the bird's own surfaces ARE the frequency authority here
          // (transponder cards, passband strip, the binding line). Operator
          // report: "my dropdowns for frequencies are still showing FT8
          // frequencies under satellite" — the top control offered 14.074 next
          // to a bird on 435 MHz.
          effectiveView === 'sats'
        }
        hideDigitalChrome={
          effectiveView === 'phone' ||
          effectiveView === 'cw' ||
          // RTTY/SSTV/APRS are free-running modes with their OWN band selectors — the
          // top control is fed the DIGITAL (FT8) plan, and the tier tiles / slot
          // clock / DT readout are slot-sync furniture that means nothing here.
          effectiveView === 'rtty' ||
          effectiveView === 'sstv' ||
          effectiveView === 'aprs' ||
          effectiveView === 'sats'
        }
        tier={tier}
        onTierChange={handleTier}
        onOpenGuide={() => setShowGuide(true)}
        field={fieldMode}
        onFieldChange={setFieldMode}
        // Who is at the key (#25). Absent/empty renders nothing — the single-op case.
        operator={settings?.fdOperator ?? ''}
        operatorRoster={opRoster}
        onSetOperator={handleSetOperator}
      />

      <UpdateBanner update={selfUpdate} />

      {/* ⚠️ NO blockReason — deliberate, operator ruling 2026-07-26.
          This used to disable Work and show "In a QSO with <call>" or "Transmitting" in place of
          the button. Both were wrong for what Pounce IS: a race against a pileup that has not
          formed yet. Whether to abandon the current contact and chase a new one is the
          operator's call to make in that moment, not something the app should refuse — and the
          reason text replaced the very control they were reaching for, so the alert became a
          notification you could not act on.
          Safe to allow: `work_spot_split` sets the mode then the dial, and the radio loop already
          defers a retune while a slot is actively transmitting (rigs reject VFO/mode changes
          mid-TX), so a click during our own over is applied at the next safe moment rather than
          keying across it. This removes a UI refusal, not a TX-path guard.
          Still notify-never-act ([[feedback-alerts-notify-never-act]]): Nexus does not move the
          radio on its own here — this is a button the operator presses. */}
      <PounceBanner alert={pounceAlert} onDismiss={dismissPounce} onWork={handlePounceWork} myGrid={snap.mygrid} />

      {needsOnboarding && (
        <OnboardingBanner
          onOpenSettings={() => handleView('settings')}
          onDismiss={handleDismissOnboarding}
        />
      )}

      {reveal.pending && (
        <RevealNudge
          feature={reveal.pending.feature}
          achievement={reveal.pending.achievement}
          onEnable={reveal.enable}
          onDismiss={reveal.dismiss}
        />
      )}

      <NowBar
        snap={snap}
        prop={prop}
        feedHealth={feedHealth}
        connectEnabled={features.isOn('connect')}
        dxpedEnabled={features.isOn('dxped')}
        onNavigate={handleView}
        // Profile-declared chip emphasis (dangling since the profiles landed).
        // A hand-blended feature set is tagged 'custom' (no profile) → default order.
        emphasis={features.profile === 'custom' ? undefined : PROFILES[features.profile].nowBarEmphasis}
      />

      <div className="shell">
        <ModeNav
          view={effectiveView}
          mode={snap.mode}
          enabled={navEnabled}
          onSelect={handleView}
          tier={tier}
          onDigitalMode={handleDigitalMode}
        />
        {/* CRASH CONTAINMENT — inside `.shell` and AFTER the rail, deliberately.
            A render throw in a view used to unmount the ENTIRE root (0.24.6 field
            incident: a rules-of-hooks bug in a Connect pane → a black window, rail
            included, re-armed on every launch because the persisted view was
            restored straight back into it). Scoped here, a crash costs the view and
            nothing else: the rail, top bar, Now Bar and toasts keep rendering, so
            the operator can always navigate out. The keep-alive hosts are INSIDE it
            too — an Operate/RTTY/SSTV/APRS crash is exactly as fatal, and there is
            no useful "black screen but the decodes survived". `resetKey` (not a
            React `key`) clears the fallback on navigation without remounting those
            hosts every section change — see the ErrorBoundary header. */}
        <ErrorBoundary
          label={featureById(effectiveView)?.label ?? 'This section'}
          resetKey={effectiveView}
          action={{
            label: crashEscape === effectiveView ? 'Try again' : `Back to ${featureById(crashEscape)?.label ?? 'Operate'}`,
            onClick: () => handleView(crashEscape),
          }}
        >
          {/* Operate cockpit lives here PERMANENTLY (mounted once, hidden when you're
              on another section) so the waterfall + Band Activity keep decoding and
              accumulating in the background — navigate away and back and your decodes
              are exactly where you left them, plus everything heard while away. The
              host is display:contents when shown (so the inner <main> flexes exactly
              as before) and display:none when hidden. */}
          <div className="operate-host" hidden={effectiveView !== 'operate'}>
            <OperateCockpit
              companionAddr={settings?.companionAddr}
              blockedCalls={settings?.blockedCalls ?? []}
              onToggleBlocked={handleToggleBlocked}
              snap={snap}
              theme={theme}
              tier={tier}
              onTierChange={handleTier}
              bandPlan={bandPlan}
              onSetFrequency={handleSetFrequency}
              onSourceChange={handleSourceChange}
              onTune={handleTune}
              onCall={handleCall}
              onSetTxLevel={handleSetTxLevel}
              onSetMode={handleSetMode}
              onSetTxEven={handleSetTxEven}
              onSetTxCycleAuto={handleSetTxCycleAuto}
              onResend={handleQsoResend}
              onFreetext={handleQsoFreetext}
              onLog={handleLogCurrent}
              onOverrideTx={handleOverrideTx}
              onHaltTx={handleHaltTx}
              onSetTxEnabled={handleSetTxEnabled}
              onSetTune={handleSetTune}
              onSetHoldTxFreq={handleSetHoldTxFreq}
              dxClearTick={dxClearTick}
              onSnap={setSnap}
              onOpenSettings={openSettingsAt}
              wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
              preferRrr={settings?.preferRrr ?? false}
              qsoMacros={macros.qso}
              roster={operateStationsPanel}
              needByCall={needByCall}
              needAlertsByCall={needAlertsByCall}
              selectedCall={activePeer}
              onSelect={handleSelect}
              layoutMode={operateLayout}
              onLayoutMode={handleOperateLayout}
              panels={operatePanels}
              onPopOut={() => void openPanelWindow('operate')}
              active={effectiveView === 'operate'}
            />
          </div>
          {/* RTTY + SSTV keep-alive hosts (same pattern as .operate-host): the decoded
              RTTY stream and the always-armed SSTV VIS receiver keep accumulating in
              the backend while the operator is on another section; `active` gates only
              each view's display poll (the OperateCockpit pattern). Gated on the
              feature toggle so a disabled section mounts nothing. */}
          {isViewEnabled('rtty') && (
            <div className="rtty-host" hidden={effectiveView !== 'rtty'}>
              <RttyCockpit
                snap={snap}
                onSnap={setSnap}
                active={effectiveView === 'rtty'}
                onSetFrequency={handleSetFrequency}
                onSetTxEnabled={handleSetTxEnabled}
                theme={theme}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                panels={rttyPanels}
              />
            </div>
          )}
          {isViewEnabled('sstv') && (
            <div className="sstv-host" hidden={effectiveView !== 'sstv'}>
              <SstvView
                snap={snap}
                theme={theme}
                onSnap={setSnap}
                active={effectiveView === 'sstv'}
                onSetFrequency={handleSstvTune}
                onSetTxEnabled={handleSetTxEnabled}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                txModeDefault={settings?.sstvDefaultTxMode}
                txPowerPct={settings?.sstvTxPowerPct}
                panels={sstvPanels}
                onOpenSettings={openSettingsAt}
              />
            </div>
          )}
          {isViewEnabled('aprs') && (
            <div className="aprs-host" hidden={effectiveView !== 'aprs'}>
              <AprsCockpit
                theme={theme}
                myGrid={snap.mygrid}
                active={effectiveView === 'aprs'}
                onTune={handleAprsTune}
                radio={snap.radio}
                onSetTxEnabled={handleSetTxEnabled}
                onOpenSettings={openSettingsAt}
              />
            </div>
          )}
          {workspace}
        </ErrorBoundary>
      </div>

      <Toasts />
      <Announcer />

      {radioPicker?.showPicker && (
        // First screen for a two-radio setup — pick which radio this window drives. Choosing
        // relaunches the window bound to that radio (no shortcuts/env for the operator).
        <RadioPicker
          info={radioPicker}
          onChoose={(id) => void chooseRadio(id)}
          onSingleRadio={() => void useSingleRadio().finally(() => setRadioPicker(null))}
        />
      )}

      {showWizard && settings && (
        // Gated on settings: the prefills are one-shot useState initializers,
        // and mounting against null would turn the whole real config into
        // "fields the operator cleared" at draft time (review catch).
        <SetupWizard
          settings={settings}
          radio={snap.radio}
          onApply={handleWizardApply}
          onTestCat={handleWizardTestCat}
          onProveTx={handleProveTx}
          onSkip={handleWizardSkip}
          onOpenGuide={() => setShowGuide(true)}
        />
      )}

      {showGuide && <GettingStartedGuide onClose={() => setShowGuide(false)} />}

      {snap.pendingLog && (
        <LogConfirm
          record={snap.pendingLog}
          onConfirm={handleConfirmLog}
          onDiscard={handleDiscardLog}
        />
      )}
    </div>
  )
}

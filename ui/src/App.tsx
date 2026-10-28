import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useEscStop } from './useEscStop'
import { publishBandConditions } from './bandConditions'
import type { AppSnapshot, BandChannel, ExperienceLevel, LoggedQso, ModeRequest, Settings, SourceKind, Tier } from './types'
import { isOnAir } from './types'
import { rigModeTransition, RIG_MODE_BY_VIEW, type RigMode } from './rigModeForView'
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
  getFdRuleset,
  getSettings,
  setLogbookColumns,
  logOperators,
  getSnapshot,
  type FdRulesetDto,
  selectPeer as apiSelectPeer,
  archiveConversation as apiArchiveConversation,
  sendMessage as apiSendMessage,
  broadcast as apiBroadcast,
  setChatCq as apiSetChatCq,
  resumeChatCq as apiResumeChatCq,
  setFrequency as apiSetFrequency,
  sstvTune as apiSstvTune,
  setBlockedCalls as apiSetBlockedCalls,
  js8SetRxBlocklist,
  js8SetGroups,
  js8SetAvoidAllcall,
  tuneChannel as apiTuneChannel,
  aprsTune,
  setMode as apiSetMode,
  setTier as apiSetTier,
  setSource as apiSetSource,
  setTxEnabled as apiSetTxEnabled,
  setTxLevel as apiSetTxLevel,
  setActiveRadio as apiSetActiveRadio,
  switchStationProfile as apiSwitchStationProfile,
  setPegLock as apiSetPegLock,
  setTune as apiSetTune,
  atuBypass as apiAtuBypass,
  atuTune as apiAtuTune,
  haltTx as apiHaltTx,
  setTxEven as apiSetTxEven,
  setTxCycleAuto as apiSetTxCycleAuto,
  setBeacon as apiSetBeacon,
  setRxOffset as apiSetRxOffset,
  setTxOffset as apiSetTxOffset,
  setHoldTxFreq as apiSetHoldTxFreq,
  subscribeSnapshot,
} from './api'
import { withErrorToast, pushToast, dismissToast, setPopupNotifications, setPopupMinSeconds } from './toast'
import { contestStartWarning } from './features/contestLocation'
import { keptFileMessage } from './features/keptFiles'
import { flexVoiceNeedsDax } from './features/flexDaxNote'
import { useActivationNotices } from './useActivationNotices'
import { t } from './i18n'
import { resolveUnits, setUnitsMirror } from './units'
import { doubleBeep, processDecodes, txEarcon, type NeededAlert } from './alerts'
import { openingToastSpec } from './openingAlert'
import { announce } from './announce'
import { Announcer } from './components/Announcer'
import { ErrorBoundary } from './components/ErrorBoundary'
import { loadWatchlist, type WatchFilter } from './watchlist'
import { foldRetiredWantedList } from './features/watchlistFold'
import { useTheme } from './useTheme'
import { useContrastPrefs } from './useFieldMode'
import { usePaletteRoles } from './usePaletteRoles'
import { useSkin } from './useSkin'
import { useNight } from './useNight'
import { useScale } from './useScale'
import { useDpiScaleSeed } from './useDpiSeed'
import { useViewport, useViewportClass } from './useViewport'
import { useDensity } from './useDensity'
import { useTextSize } from './useTextSize'
import { useLocalClock } from './useLocalClock'
import { useMotion } from './useMotion'
import { useBandEdgeTones } from './useBandEdgeTones'
import { useAchievements } from './useAchievements'
import { useJourneyUnlocks } from './useJourneyUnlocks'
import { useFeatures } from './useFeatures'
import { useReveals } from './useReveals'
import { sectionFeatures, featureById, type FeatureId } from './features/registry'
import { resolveBootView, coerceArea } from './features/bootView'
import { visibleNeeds, boardNeeds, workTarget, modeClassOf, topNeedByCall, alertsByCall, activityTypeByCall, ownScreen, workTier, pskSubmode, workHintView, WORK_SECTION, spotNeed, type OwnScreen } from './features/needs'
import { OPERATE_PANELS, CW_PANELS, PHONE_PANELS, PSK_PANELS, JTTY_PANELS, RTTY_PANELS, SSTV_PANELS, JS8_PANELS, boxEntries, ownShown, usePanelLayout, type PanelLayoutApi, type PanelVocabulary } from './features/panelState'
import { surfaceGet, surfaceSet } from './features/windowScope'
import { DXPED_WINDOWS, KP_FORECAST, XRAY_NOW, watchFeed } from './features/connectFeeds'
import { DASH_RAIL_FOLDS, DASH_SLOT_IDS, isDashRailSection, railOnScreen, useDashRailSections, type DashRailSection, type DashSlotId } from './features/dashRail'
import { sharedPaneById, sharedPaneOf } from './features/sharedPanes'
import type { PaneId } from './features/connectConfig'
import { DashRail, dashRailRecords, useDashRail } from './components/DashRail'
import type { BoxSource, RailLink } from './components/panes/CockpitBox'
import { publishDashRailSwitch, type DashRailSwitch } from './components/dashRailSwitch'
import { usePaneWidths, LEFT_MIN, RIGHT_MIN } from './usePaneWidths'
import { PaneSeam } from './components/PaneSeam'
import { WATERFALL_SPLIT_MAX, WATERFALL_SPLIT_MIN } from './features/paneSeam'
import { TopBar } from './components/TopBar'
import { StationList, type StationFilter } from './components/StationList'
import { Conversation } from './components/Conversation'
import { TempoHeader } from './components/TempoHeader'
import { CockpitTxStrip } from './components/CockpitTxStrip'
import { TxAlarmBanner } from './components/TxAlarmBanner'
import { Waterfall } from './components/Waterfall'
import { FT_PALETTE_SCOPE } from './waterfallPalette'
import { markerWidthHz } from './waterfall'
import { LinkPill } from './components/LinkPill'
import { ModeNav, type View, type DigitalMode } from './components/ModeNav'
import { contestName } from './fdEvent'
import { OperateCockpit } from './components/OperateCockpit'
import { NowBar } from './components/NowBar'
import { AwardsJourney } from './components/AwardsJourney'
import { CwCockpit } from './components/CwCockpit'
import { PhoneCockpit } from './components/PhoneCockpit'
import { RttyCockpit } from './components/RttyCockpit'
import { PskCockpit } from './components/PskCockpit'
import { Js8Cockpit, type Js8ListSettings } from './components/Js8Cockpit'
import { JttyCockpit } from './components/JttyCockpit'
import { js8ComposeConfig } from './js8Compose'
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
  setWatchList,
  getAllSpots,
  clickTciSpot,
  getSatSchedule,
  getSatTrackStatus,
  getSatTransponder,
  getIssPass,
  getTleStatus,
  sstvArm,
  sstvAutoDisarm,
  setSettings as apiSetSettings,
  setContestEsm,
  setCwSpeedMatch,
  setFdOperator,
  setSidebandOverride,
  testCat,
  setOperatingMode,
  workSpot,
  getLicensedBandPlan,
  getPskState,
  pskSetMode,
  setHuntTarget,
  setLicenseClass,
  setExperienceLevel as apiSetExperienceLevel,
  markSetupWizardSeen,
  stopQsoRecording,
  pointRotatorAtCall,
  qsySetEnabled as apiQsySetEnabled,
  radioLaunchInfo,
  chooseRadio,
  useSingleRadio,
  resendChat,
  type RadioLaunchInfo,
  focusWindow,
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
import { sameCall } from './callsign'
import { processFlare, effectiveXray } from './flareAlert'
import { processStorm, processStormForecast } from './stormAlert'
import { processDxpedAlerts } from './features/dxpedChase'
import { checkDxpedAlarms } from './features/dxpedAlarm'
import { checkSatAlarms, satAlarmMap } from './features/satAlarm'
import { tickSatPassAlert } from './features/satPassAlert'
import type { EsmSetting } from './features/esmHost'
import { tickIssAutoArm } from './features/issAutoArm'
import { satElementsLane } from './features/satLane'
import { parsecStopLane } from './features/parsecPresence'
import { slotKeyRefusedLane } from './features/slotKeyRefused'
import { slotUnkeyFailedLane } from './features/slotUnkeyFailed'
import { slotAudioLostLane } from './features/slotAudioLost'
import { slotOverDroppedLane } from './features/slotOverDropped'
import { pttRefusedLane } from './features/pttRefused'
import { voiceDroppedLane } from './features/voiceDropped'
import { clockRepairHoldLane } from './features/clockRepairHold'
import { heldLine, txOnHeldLane } from './features/txOnHeld'
import { drivesAnotherStation } from './features/durableStore'
import { audioErrorLane } from './features/audioError'
import { askAgainWhileSaving } from './features/notAnswered'
import { useTciSpots } from './features/tciSpots'
import { dxpedWorkMode } from './components/connect/paneFormat'
import { setStatus } from './status'
import type { PropagationSnapshot, FeedHealth, NeedAlert, SpotRow, DxpedWindow, WorkableCard, CatTestResult, PointedAt } from './types'
import { pointedTo } from './components/rotorPointAt'
import { NeededPanel } from './components/NeededPanel'
import { SpotsPanel } from './components/SpotsPanel'
import { LogConfirm } from './components/LogConfirm'
import { ContestView } from './components/ContestView'
import { CONTEST_SETUP_SAVED_EVENT } from './components/ContestSetupWizard'
import { ContestLargeEntry } from './components/ContestLogger'
import { openContestLogger } from './features/contestLoggerLayout'
import { ContestScreenAway, useContestScreenWindow } from './components/ContestScreenWindow'
import { useWorkInRequests } from './features/contestWorkIn'
import { useContestSwitch } from './contestSwitch'
import { useOperatorBoxAsk } from './features/contestKeys'
import { OperateDecodes } from './components/OperateDecodes'
import { Logbook } from './components/Logbook'
import { RoamPanel } from './components/RoamPanel'
import { SettingsPanel } from './components/SettingsPanel'
import { Toasts } from './components/Toasts'
import { ConfirmHost, confirmDialog } from './confirm'
import { RemoteApprovalPrompt } from './remote-native/RemoteApprovalPrompt'
import { LogbookSaving } from './components/LogbookSaving'
import { OnboardingBanner } from './components/OnboardingBanner'
import { PounceBanner } from './components/PounceBanner'
import { UpdateBanner } from './components/UpdateBanner'
import { ClientBanner } from './components/ClientBanner'
import { useSelfUpdate } from './useSelfUpdate'
import { usePounce, type PounceAlert } from './usePounce'
import { useStationAlerts } from './useStationAlerts'
import { RevealNudge } from './components/RevealNudge'
import { SetupWizard, type WizardDraft } from './components/SetupWizard'
import { GettingStartedGuide } from './components/GettingStartedGuide'
import { FirstContactWalk } from './components/FirstContactWalk'
import { RadioPicker } from './components/RadioPicker'
import { PROFILES, type ProfileId } from './features/profiles'
import { CallCardHost } from './components/CallCard'
import { useRadioSwitchNotice } from './useRadioSwitchNotice'

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). It is the shell: it
// renders no cockpit control of its own, so what moved is its OWN prose — the loading line,
// the two rail resizers, the status lane, the crash escape, and the toasts its handlers
// raise. Every callsign, band, mode, dial reading, tier and source label those toasts carry
// is DATA, interpolated invariantly and never translated (`i18n/index.ts`). The DEFAULT_MACROS
// below are transmitted text — Q-codes and on-air phrases — so they stay in the code, and the
// section names come from the feature registry, which owns those words.

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
  'FT2',
  'Q65',
  'MSK144',
  'FST4',
  'FST4W',
  'JT65',
  'WSPR',
]

/** The screens whose Esc App binds to the top bar's halt while one is on show (see the listener in
 *  App). The other operating screens bind their own Esc; stop-control-wiring.test.tsx presses Esc
 *  on every section in the registry and holds each to what it sends. Connect is here because it draws
 *  no top bar, so no Stop TX of its own (the operator's 2026-10-01 ruling). */
const ESC_HALT_VIEWS: ReadonlySet<View> = new Set<View>(['chat', 'phone', 'jtty', 'sstv', 'aprs', 'sats', 'connect'])
/** The screen of each section the station reports (`radio.operatingMode`), for a computer driving
 *  it to follow. 'keyboard' is PSK's screen, the section's own landing; 'jtty' is the Keyboard
 *  section while the station names JTTY as its modem (`radio.keyboardMode`). */
const SCREEN_BY_SECTION: Partial<Record<string, View>> = {
  digital: 'operate',
  phone: 'phone',
  cw: 'cw',
  rtty: 'rtty',
  keyboard: 'psk',
  jtty: 'jtty',
}
function App() {
  const [theme, setTheme, themeChoice] = useTheme()
  // The built-in themes (Settings ▸ Appearance ▸ Theme): App is the one writer of `data-skin`,
  // and Settings is handed the theme and the setter, the same way it gets the theme.
  const [skin, setSkin] = useSkin(theme)
  // The contrast axis: field mode (outdoor/POTA) and the standing high-contrast preference
  // (#215). Both set data-contrast on <html>; only field mode is handed to useScale, because
  // only it carries the larger auto-fit. Global — facts about the station, like the theme.
  const { fieldMode, setFieldMode, highContrast, setHighContrast } = useContrastPrefs()
  // The colour roles (Settings ▸ Appearance ▸ Colours): App is their one writer, and Settings is
  // handed the picks and the setter, the same way it gets the theme.
  const { palette, setPreset: setPalettePreset } = usePaletteRoles()
  const { scale, mode: scaleMode, cap: scaleCap, setMode: setScaleMode, setCap: setScaleCap } = useScale(fieldMode)
  // First launch on a high-density display: raise the auto-fit ceiling so the UI can grow to
  // the panel. Once only, raise only, and nothing at all on an ordinary 96-dpi monitor or on
  // a platform whose OS already scales for us. See useDpiSeed.ts.
  useDpiScaleSeed(setScaleCap)
  // Publishes the zoom-aware `data-viewport` size class on <html> (live on resize
  // AND on scale change) so the layout adapts to the EFFECTIVE width.
  useViewport(scale)
  // THE DASHBOARD RAIL beside the operating cockpits (components/DashRail): the published class says
  // whether this window can show it (`lg` and up — never a size query), and each section remembers
  // whether it does. Off everywhere until the operator turns it on.
  const viewportClass = useViewportClass()
  const dashRail = useDashRailSections()
  // Its contents, per cockpit (each starting from the rail every cockpit shared before), owned here with the
  // cockpits' own records, so the two can share one reading of what is on screen.
  const dashRailRec = useDashRail()
  const railFits = viewportClass === 'lg' || viewportClass === 'xl'
  // Density (row heights / padding) and text size (#215) — both chosen in Settings ▸ Workspace.
  const [density, setDensity] = useDensity()
  const [textSize, setTextSize] = useTextSize()
  // #253: optional local-time clock beside UTC in the top bar (per machine, off by default).
  const [localClock, setLocalClock] = useLocalClock()
  // Motion (Settings ▸ Appearance ▸ Performance): App is its one writer, like the theme.
  const [motion, setMotion] = useMotion()
  // Modular features (toggles + profiles). Drives nav, view-gating, and the
  // gamification/achievements layer.
  const features = useFeatures()
  useAchievements(features.isOn('gamification'))
  useJourneyUnlocks(features.isOn('gamification'))
  const reveal = useReveals(features)
  // First-run setup wizard (goal-driven). Only on a genuinely fresh install, and never by itself on a
  // computer driving another station: it sets up a station, and there every setting it edits is the
  // station's, while "fresh" is read from this computer's own storage. Settings still offers it.
  // "Fresh" is decided by the settings file as well (`setupWizardSeen`, below): this computer's
  // storage only says the wizard MAY be due; it opens only where there was no settings file and it
  // was never done, so a WebView data reset or a reinstall never shows it to an existing station.
  const [showWizard, setShowWizard] = useState<boolean>(
    () => !drivesAnotherStation() && features.firstRun && storageWritable() && !wizardSeen(),
  )
  // Getting started guide — Help ▸ Getting started, and the wizard's optional
  // walkthrough offer. Pure documentation: it writes nothing and is never
  // shown unasked, so there is no "seen" flag to persist.
  const [showGuide, setShowGuide] = useState(false)
  // The first-run question ("How much have you done?") is asked only when the wizard opens by
  // itself on a fresh install; re-opened from Settings it does not ask again.
  const [wizardAsks, setWizardAsks] = useState(showWizard)
  // The first-contact walk-through (Help ▸ Your first FT8 contact, and the wizard's offer). Like
  // the guide it writes nothing; asked for together with the guide, it opens once the guide closes.
  const [showWalk, setShowWalk] = useState(false)
  // `scale` so the rail clamps re-run on zoom change (ceilings are zoom-relative).
  const { leftW, rightW, leftMax, rightMax, commitLeft, commitRight, resetLeft, resetRight, resetWidths } =
    usePaneWidths(scale)
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  // Night (Settings ▸ Appearance ▸ Theme): App is its one writer. Auto goes by the sun at the
  // station's grid square, which arrives with the snapshot — until then Auto has no grid and stays off.
  const { night, setNight, gridKnown: nightGridKnown } = useNight(snap?.mygrid ?? '')
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
  // call refire the cockpit's prefill effect. Cleared once consumed. `park`: the park or
  // summit a Needed row is an activation of, filled into the log with the call (LogEntry).
  const [pendingWork, setPendingWork] = useState<{
    call: string
    view: 'cw' | 'phone'
    ts: number
    park?: { program: string; reference: string } | null
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
  // Live mirror of the screen in front of the operator, for the app-wide 30 s poll effect (empty
  // deps): the ISS SSTV auto-arm may take the radio only while SSTV or Satellites is that screen.
  const viewRef = useRef(view)
  useEffect(() => {
    viewRef.current = view
  }, [view])
  // Bird handed off from a map satellite click — the Satellites section opens on it.
  const [satFocus, setSatFocus] = useState<string | null>(null)
  // Callsign handed off from a cockpit recall card's previous-contact row (#192) — the Logbook
  // opens with it in the search box. `pendingWork`'s shape, not `satFocus`'s, and the `ts` nonce
  // is the reason: the operator can type over the Logbook's search box, go back to the cockpit
  // and click the same call again, and a bare `string | null` would not refire for that. Cleared
  // once the Logbook has consumed it, so a later trip through the nav opens unfiltered.
  const [logFocus, setLogFocus] = useState<{ call: string; ts: number; editId?: string | null } | null>(null)
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
  // TWO REFS, AND THEY ANSWER DIFFERENT QUESTIONS. Conflating them was #143.
  //
  // `lastOpModeRef` — the last mode ASSERTED. Every operating view advances it, Tempo
  // included, because it is what tells the manual-log form and the memories panel which mode
  // the operator is actually running. Getting that wrong silently wrote "FT8" on phone
  // contacts and corrupted awards, so it must keep tracking every assert.
  //
  // `lastHomedModeRef` — the last mode we RE-HOMED THE DIAL FOR. Only views that own a
  // frequency advance it. Tempo asserts the digital mode but keeps its own band picker's
  // frequency, so under the old single ref it advanced the guard without homing and the FT
  // screen then saw "no change" and left FT8 on the CW dial. `rigModeForView.ts` has the
  // whole reasoning; do not merge these back together.
  const lastOpModeRef = useRef<RigMode>('digital')
  const lastHomedModeRef = useRef<RigMode>('digital')
  // A screen entered without asserting its section, named for the rig-mode effect alone: the screen
  // a computer driving a station follows it to, or goes back to when the station refuses a section
  // change (both below), and the cockpit a Work from afar opened, named by the work-hint follow below. That Work (`workAfar`: a computer driving this station) entered its section WITHOUT
  // arming Enable TX, and the effect's re-assert, from this screen, would arm the latch for Phone,
  // CW, RTTY and PSK: the authority the Work withheld. Read and cleared on every run, so it holds
  // back only the one entry it names.
  const afarEntryRef = useRef<View | null>(null)
  // Guard refs for the rig-mode effect below. `opModeSeeded` is set once, by whichever comes
  // first: the persisted-operating-mode seed (that effect lives further down, where `settings`
  // is in scope) or a genuine view change — an operator's click outranks any seed, because the
  // click IS the intent.
  const opModeSeeded = useRef(false)
  const didMountOpMode = useRef(false)
  // The screen being LEFT, for the one entry that depends on it: Phone from SSTV keeps the dial
  // (`rigModeForView.ts`). Starts as the restored view, so the first click after launch knows it.
  const prevViewRef = useRef(view)

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
    // modes (engine.rs) wherever no stop holds it off. SF ticket #3.
    //
    // This is a MOUNT guard, NOT the same-value guard the comment below rightly forbids —
    // every subsequent view change still re-asserts unconditionally. A re-assert after Stop TX,
    // Esc, TX Off or the watchdog leaves TX off: the engine arms only where nothing stopped it.
    const from = prevViewRef.current
    prevViewRef.current = view
    if (!didMountOpMode.current) {
      didMountOpMode.current = true
      return
    }
    opModeSeeded.current = true // an explicit view change outranks the booted-mode seed
    const fromAfar = afarEntryRef.current === view
    afarEntryRef.current = null
    // The map, the home-on-entry rule and the guard bookkeeping all live in
    // `rigModeForView.ts` — pure, so the SEQUENCE of views can be tested. That matters:
    // #143 was a sequence bug (CW → Tempo → FT left FT8 on the CW dial) and no single
    // transition in it is wrong, so nothing that looked at one transition could see it.
    // Read that module's header before changing any of this; it carries #80's history too.
    //
    // ALWAYS (re)assert the rig mode on entering an operating view. We must NOT skip it with a
    // same-value guard: the guard ref drifts out of sync with the real rig (handleDigitalMode
    // and the Needed click set the mode without going through here), which left the rig stuck
    // in the wrong mode while the VFO read-back kept working. The backend is idempotent and
    // re-arms an immediate retune, so re-asserting is cheap.
    const prevOp = lastOpModeRef.current
    const prevHomed = lastHomedModeRef.current
    const { mode, followFreq, keepDial, nextHomed } = rigModeTransition(view, lastHomedModeRef.current, from)
    lastHomedModeRef.current = nextHomed
    if (!mode) return
    lastOpModeRef.current = mode // every assert advances THIS one, including Tempo
    // A cockpit a Work from afar opened is not re-asserted from here: that Work entered its section
    // and left Enable TX as it was (`afarEntryRef`).
    if (fromAfar) return
    void setOperatingMode(mode, followFreq, keepDial)
      .then((s) => s && setSnap(s))
      .catch((why) => {
        // At the station a refusal stays quiet, as it always has. A computer driving a station
        // says why, and goes back to the screen it came from, which is still the station's
        // section; the way back sends nothing (`afarEntryRef`).
        if (!drivesAnotherStation()) return
        pushToast(String(why), 'error')
        if (viewRef.current !== view) return
        lastOpModeRef.current = prevOp
        lastHomedModeRef.current = prevHomed
        afarEntryRef.current = from
        setView(from)
      })
  }, [view])
  const [prop, setProp] = useState<PropagationSnapshot | null>(null)
  // DXpeditions ON THE AIR NOW that announced SuperFox — a format this version of Nexus has no
  // decoder for, so the operation never reaches the decode list and Hound mode cannot help.
  // The Operate header names them beside the Hound button; the calendar already knew, and an
  // operator who finds out in the middle of the pileup has found out too late. Deduped —
  // `workableNow` carries one card per needed BAND, so a multi-band operation appears several
  // times and would otherwise be listed several times.
  const superFoxCalls = useMemo(
    () =>
      Array.from(
        new Set(
          (prop?.dxpeditions.workableNow ?? [])
            .filter((c) => c.ft8Mode === 'SuperFox')
            .map((c) => c.call),
        ),
      ),
    [prop],
  )
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
  // FT's Stations list's filter chip and search, owned here for the same reason: the list is a pane in
  // the cockpit's region, which remounts its panes when it switches between today's tree and the
  // arranged columns (⊞ Arrange's first move, the dashboard rail's boxes folding in below `lg` and out
  // again). Neither is stored, so a restart still opens on All with no search.
  const [operateStationsFilter, setOperateStationsFilter] = useState<StationFilter>('all')
  const [operateStationsQuery, setOperateStationsQuery] = useState('')
  // Host SSTV's panel record above its keep-alive view so removals + share edits survive the
  // view's remounts (panelState.ts: owned by a host that outlives the view).
  const sstvPanels = usePanelLayout(SSTV_PANELS)
  // Phone has NO removable panels (operator 2026-07-25) — every pane is fixed and framed.
  const phonePanels = usePanelLayout(PHONE_PANELS)
  const cwPanels = usePanelLayout(CW_PANELS)
  const rttyPanels = usePanelLayout(RTTY_PANELS)
  const pskPanels = usePanelLayout(PSK_PANELS)
  const js8Panels = usePanelLayout(JS8_PANELS)
  const jttyPanels = usePanelLayout(JTTY_PANELS)

  // Operate MODE: 'dx' (FT8/FT4 structured cockpit) or 'msg' (Tempo two-way
  // calling). The FT8/FT4 ⇄ Tempo switch binds the radio tier+mode and swaps only
  // the cockpit; Connect/Map/Prop/Logbook/Awards are GLOBAL views selected from the
  // sidebar (they never retune the radio). Default FT8/FT4 (the 80% case).
  // Tempo's full-width waterfall strip (2026-10-26), the box its height divider sizes.
  const tempoWfRef = useRef<HTMLDivElement>(null)
  const [area, setArea] = useState<'dx' | 'msg'>(() => {
    try {
      // coerceArea (test-pinned) also migrates the retired 'connect' area to
      // FT8/FT4 (Connect is now a global view).
      // …and opens a remembered Tempo mode on FT while Tempo is switched off (#402).
      return coerceArea(localStorage.getItem('nexus.workspace'), features.enabled.chat !== false)
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

  // #402: Tempo switched off in Settings ▸ Features while it is the mode takes the station to FT8/FT4
  // (as on the next start), without moving the operator off the screen they are on: its rail item is
  // gone, so a Tempo mode would have no way back. The screen itself falls back by `effectiveView`.
  const tempoOn = features.enabled.chat !== false
  useEffect(() => {
    if (tempoOn || area !== 'msg') return
    setArea('dx')
    try {
      localStorage.setItem('nexus.workspace', 'dx')
    } catch {
      /* ignore */
    }
    void withErrorToast(() => apiSetArea('dx'), t('shell.error.switchMode')).then((s) => {
      if (s) setSnap(s)
    })
  }, [tempoOn, area])

  // Pop the Needed board out into its own window at launch — but honor the
  // operator's persisted choice (the board previously force-opened EVERY start
  // with no opt-out; closing it each session was a standing annoyance). The
  // toggle lives on the Needed panel header; default stays auto-open.
  // #240: NOT while the first-run setup wizard is up. The board is its own OS window and
  // landed on top of the wizard on a fresh install, so a new operator could not reach it.
  // Waiting (rather than skipping) keeps the pop-out: it opens once the wizard is finished
  // or dismissed. Later launches never show the wizard, so they are unchanged.
  const neededPoppedRef = useRef(false)
  useEffect(() => {
    if (neededPoppedRef.current || !snap || showWizard) return
    if (features.enabled.needed === false) return
    neededPoppedRef.current = true
    try {
      if (localStorage.getItem('nexus.needed.autopop') === 'off') return
    } catch {
      /* storage blocked — keep the default behavior */
    }
    void openPanelWindow('needed').catch(() => {})
  }, [snap, features.enabled, showWizard])

  const handleWorkspace = useCallback((w: 'dx' | 'msg') => {
    setArea(w)
    try {
      localStorage.setItem('nexus.workspace', w)
    } catch {
      /* ignore */
    }
    // Switching mode lands on that mode's cockpit (FT8/FT4 → Operate, Tempo → Chat).
    setView(w === 'dx' ? 'operate' : 'chat')
    void withErrorToast(() => apiSetArea(w), t('shell.error.switchMode')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])
  // Surface the station's audio-error line in the persistent status lane — it was only visible
  // deep in Settings ▸ CAT, i.e. effectively invisible. The headline names the kind of problem the
  // station gives it (a PTT the rig did not accept, a sound card that failed, a monitor held off),
  // tiered by what it costs, and the station's sentence is the tooltip (features/audioError.ts).
  useEffect(() => {
    setStatus('audio', audioErrorLane(snap?.radio.audioError, snap?.radio.audioErrorKind))
  }, [snap?.radio.audioError, snap?.radio.audioErrorKind])

  // Surface an RF-scope source that is not delivering. WARNING, not critical, and the wording says
  // what to DO: on an FT-710 the spectrum only exists once SCU-LAN10 and the external display are
  // enabled in the radio's EX menu, and Nexus cannot set either over CAT — so a blank pane with no
  // explanation would send the operator hunting through Nexus for a setting that is on the rig.
  // The waterfall keeps running on sound-card audio throughout, which is why this never blocks.
  useEffect(() => {
    const err = snap?.radio.scopeError
    if (err) {
      setStatus('scope', { tier: 'warning', message: 'NO RF SCOPE', detail: err })
    } else {
      setStatus('scope', null)
    }
  }, [snap?.radio.scopeError])

  // #275: the radio REFUSED a scope span. Its own lane, not `scope` above: the scope is
  // streaming perfectly and one control was rejected, which is a different fact with a different
  // cure. Nexus does not fix it by flipping the rig's scope mode — that is the operator's pick —
  // so the lane's job is to say what happened and what to do, and to go away by itself the moment
  // a span is accepted (the backend writes this field in both directions).
  useEffect(() => {
    const refused = snap?.radio.scopeSpanRefused
    if (refused) {
      setStatus('scopeSpan', {
        tier: 'warning',
        message: t('shell.lane.scopeSpan.message'),
        detail: refused,
      })
    } else {
      setStatus('scopeSpan', null)
    }
  }, [snap?.radio.scopeSpanRefused])

  // Surface a serial COM-port collision (two radios on one port) in the status
  // lane — otherwise it only shows as an unexplained red radio pill.
  useEffect(() => {
    const warn = snap?.radio.radioConfigWarning
    setStatus(
      'radioConfig',
      warn ? { tier: 'warning', message: t('shell.lane.radioConfig.message'), detail: warn } : null,
    )
  }, [snap?.radio.radioConfigWarning])

  // A rig at 0% power KEYS, shows TX, and produces an over that looks entirely normal from the
  // operator's chair — it is silent only to everyone else, which is why the report it came from
  // ("opening cat but not sending audio out") cost an evening to chase. Notify, never act: the
  // lane says so and nothing touches the power.
  useEffect(() => {
    setStatus(
      'txPowerZero',
      snap?.radio.txPowerZero
        ? {
            tier: 'warning',
            message: t('shell.lane.txPowerZero.message'),
            detail: t('shell.lane.txPowerZero.detail'),
          }
        : null,
    )
  }, [snap?.radio.txPowerZero])

  // A per-QSO recording that could not be written. The contact IS logged — only the audio failed —
  // so this is a warning in the lane rather than an error on the log action, and it names the full
  // path because "it did not save" without saying where is exactly the report that prompted it.
  useEffect(() => {
    const warn = snap?.radio.recordingWarning
    setStatus(
      'recording',
      warn ? { tier: 'warning', message: t('shell.lane.recording.message'), detail: warn } : null,
    )
  }, [snap?.radio.recordingWarning])

  // Parsec presence mode stopped a transmission because the operator's Parsec session dropped.
  // The lane, not a toast: the operator was not there to see a toast, so the reason must still be
  // on screen when they reconnect. The station keeps the report until they transmit again.
  useEffect(() => {
    setStatus('parsecStop', parsecStopLane(snap?.parsecPresence))
  }, [snap?.parsecPresence?.stoppedAt, snap?.parsecPresence?.stopped.join(',')])

  // A slot over (FT8, FT4, JS8 …) the radio did not accept PTT for: the station dropped it and
  // halted TX, as WSJT-X halts on a rig failure. The lane says so until TX is turned on again. Not
  // on the Remote page, like the Parsec stop above: that page is frozen.
  useEffect(() => {
    setStatus('slotKeyRefused', slotKeyRefusedLane(snap?.radio.slotKeyRefused))
  }, [snap?.radio.slotKeyRefused?.at, snap?.radio.slotKeyRefused?.why])

  // …and one whose unkey the radio did not accept: the station halted TX the same way and keeps
  // sending the unkey until the radio takes it. The lane says so until TX is turned on again; not
  // on the Remote page either.
  useEffect(() => {
    setStatus('slotUnkeyFailed', slotUnkeyFailedLane(snap?.radio.slotUnkeyFailed))
  }, [snap?.radio.slotUnkeyFailed?.at, snap?.radio.slotUnkeyFailed?.why])

  // …and one the station ended part way through because its audio stopped reaching the radio (the
  // route carrying it, Flex native DAX or TCI, went): TX halted the same way, and the lane says so
  // until TX is turned on again; not on the Remote page either.
  useEffect(() => {
    setStatus('slotAudioLost', slotAudioLostLane(snap?.radio.slotAudioLost))
  }, [snap?.radio.slotAudioLost?.at])

  // …and an FT8 or FT4 over the radio keyed only after its period had ended (a slow CAT link): the
  // station dropped that over and left TX on, however many in a row, as WSJT-X does, so the lane is
  // its own, a warning, until the next over keys in time. Not on the Remote page either.
  useEffect(() => {
    setStatus('slotOverDropped', slotOverDroppedLane(snap?.radio.slotOverDropped))
  }, [snap?.radio.slotOverDropped?.at, snap?.radio.slotOverDropped?.late])

  // The operator's PTT press that Nexus's own Flex client kept off the air, the radio still on the
  // DAX Nexus set and not on its mic: the lane says so, in place of the PTT advice, until a press
  // keys; not on the Remote page either.
  useEffect(() => {
    setStatus('pttRefused', pttRefusedLane(snap?.radio.pttRefused))
  }, [snap?.radio.pttRefused?.at, snap?.radio.pttRefused?.mode])

  // A voice-keyer message Nexus's own Flex client dropped before the radio took DAX for it (Phone at
  // the shack, native audio on): the lane says why, since the keyer's toast answered the press
  // before the drop, until the next message the keyer accepts; not on the Remote page either.
  useEffect(() => {
    setStatus('voiceDropped', voiceDroppedLane(snap?.radio.voiceDropped))
  }, [snap?.radio.voiceDropped?.at, snap?.radio.voiceDropped?.cause])

  // A clock repair holds transmit: from the press of Repair clock until the repair ends, the
  // station starts no transmission, and the lane says so on every screen while it lasts. Not on
  // the Remote page either: that page is frozen.
  useEffect(() => {
    setStatus('clockRepairHold', clockRepairHoldLane(snap?.radio.clockRepairTxHeld))
  }, [snap?.radio.clockRepairTxHeld])

  // A TX On refused because a newly signed-in device that is not yet approved controls the station:
  // the latch stays dark, so the lane says why while the hold stands — where to approve it at the
  // shack, that this device must be approved on a computer driving the station. Rebuilt as the
  // remote PC's words for the hold change too (the minutes left, the owner told), not every second.
  const txHoldLine = heldLine(snap?.radio.txHold)
  useEffect(() => {
    setStatus('txOnHeld', txOnHeldLane(snap?.radio.txOnHeld, drivesAnotherStation(), snap?.radio.txHold))
  }, [snap?.radio.txOnHeld, txHoldLine])

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

  // The logbook database could not be opened at launch, so this session keeps the log in
  // log.adi. Said ONCE a session, sticky until the operator dismisses it, and never in the way
  // (a toast, not a dialog); the ref, not the snapshot, is what stops every poll re-raising it.
  const logStoreNoticeShown = useRef(false)
  useEffect(() => {
    const problem = snap?.logStoreProblem
    if (!problem || logStoreNoticeShown.current) return
    logStoreNoticeShown.current = true
    pushToast(
      problem.networkFolder
        ? t('shell.logStore.network')
        : t('shell.logStore.failed', { reason: problem.reason }),
      'error',
      0,
    )
  }, [snap?.logStoreProblem])

  // A file the station could not read and kept rather than save over (a torn journal, or one a
  // newer Nexus wrote): each said ONCE a session, sticky until dismissed, as the notice above
  // is.
  const keptFilesShown = useRef(new Set<string>())
  useEffect(() => {
    for (const file of snap?.keptFiles ?? []) {
      const said = `${file.store}\n${file.path}`
      if (keptFilesShown.current.has(said)) continue
      keptFilesShown.current.add(said)
      pushToast(keptFileMessage(file), 'error', 0)
    }
  }, [snap?.keptFiles])

  // The activation said where the operator is, not on the POTA/SOTA view alone: one taken up at
  // launch, and the 0000Z rollover.
  useActivationNotices(snap?.activation)

  // The logbook database refused a change (C10b). The station keeps it in memory — sending it
  // again while the refusal can pass, holding it for the quit when it cannot — and the screen
  // says so while it lasts (operator, 2026-09-23: "Never silently lose a contact"): one sticky
  // toast when the trouble starts, one more if a change is refused for good, and when every
  // change is saved again both go, with a word that it is. The ref, not the snapshot, stops each
  // poll re-raising them.
  const logSaveToasts = useRef<{ retrying?: number; held?: number }>({})
  useEffect(() => {
    const trouble = snap?.logSaveTrouble
    const shown = logSaveToasts.current
    if (trouble) {
      if (trouble.retrying > 0 && shown.retrying === undefined) {
        shown.retrying = pushToast(t('shell.logSave.retrying', { reason: trouble.reason }), 'error', 0)
      }
      if (trouble.held > 0 && shown.held === undefined) {
        shown.held = pushToast(t('shell.logSave.held', { reason: trouble.reason }), 'error', 0)
      }
      return
    }
    if (shown.retrying === undefined && shown.held === undefined) return
    if (shown.retrying !== undefined) dismissToast(shown.retrying)
    if (shown.held !== undefined) dismissToast(shown.held)
    logSaveToasts.current = {}
    pushToast(t('shell.logSave.saved'), 'success', 8000)
  }, [snap?.logSaveTrouble])


  // Per-(band,mode) last-alert time so a band coming alive toasts once, not every
  // poll (defence in depth — the backend tracker already flags `isNew` once).
  const openingAlertRef = useRef<Map<string, number>>(new Map())
  // Latest MEASURED Kp, so the forecast watcher below can tell a heads-up from a
  // storm already in progress.
  const kpNowRef = useRef<number | null>(null)
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
          // The band dropdowns' opening dots read this same snapshot (bandConditions.ts).
          publishBandConditions(p)
          // Solar-flare heads-up (edge-triggered; flareAlert.ts owns the dedup).
          processFlare(effectiveXray(xrayFastRef.current, p.spaceWx.xrayLong))
          // Geomagnetic storm heads-up, same edge-triggered shape over the MEASURED
          // Kp already in this snapshot — no extra fetch. Until this existed a storm
          // reached the operator only if they had the Space Weather pane in front of
          // them, which is backwards: a flare is minutes, a storm is days.
          processStorm(p.spaceWx.kp)
          kpNowRef.current = p.spaceWx.kp
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
          // dial that really is theirs. The screen goes with every tick, read when the tick
          // runs: on any screen but SSTV or Satellites the radio is in use, and the pass is
          // only announced (issAutoArm.ts).
          const issDeps = {
            tuneChannel: handleTuneChannel,
            setFrequency: handleSetFrequency,
            sstvArm,
            sstvAutoDisarm,
          }
          // The radio in use is the station's only enabled radio: with its probed coverage,
          // that is how the tick knows 145.800 cannot be received here at all. Read when the
          // tick runs, as the screen is.
          const soleRadio = () =>
            !(settingsRef.current?.radios ?? []).some(
              (r) => r.enabled && r.id !== snapRef.current?.activeRadioId,
            )
          if (settingsRef.current?.issSstvAutoArm === true) {
            // A satellite pass holding the dial (a transponder held) has the radio as another
            // screen has it: the tick only announces the ISS. A failed read is no hold, as before.
            Promise.all([getIssPass(), getSatTransponder().catch(() => null)])
              .then(([issPass, held]) =>
                tickIssAutoArm(
                  issPass,
                  snapRef.current?.radio,
                  true,
                  issDeps,
                  Date.now() / 1000,
                  viewRef.current,
                  soleRadio(),
                  held != null,
                ),
              )
              // Rejection is a ROUTINE state (elements past 30 d make
              // get_iss_pass refuse) — it must still tick with no pass, or an
              // arm in flight when the refusal begins never unwinds and the
              // rig strands on 145.800 with SSTV armed (review catch).
              .catch(() =>
                tickIssAutoArm(
                  null,
                  snapRef.current?.radio,
                  true,
                  issDeps,
                  Date.now() / 1000,
                  viewRef.current,
                  soleRadio(),
                ),
              )
          } else {
            tickIssAutoArm(
              null,
              snapRef.current?.radio,
              false,
              issDeps,
              Date.now() / 1000,
              viewRef.current,
            )
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
            const spec = openingToastSpec(o, resolveUnits(settingsRef.current?.units))
            if (spec.beepHz != null) doubleBeep(spec.beepHz)
            pushToast(spec.message, spec.kind, spec.ttlMs, spec.prominent ? { alert: true, prominent: true } : { alert: true })
          }
          // Honest-state: surface non-live propagation in the Now-Bar lane.
          if (p.source === 'offline') {
            setStatus('prop', {
              tier: 'warning',
              message: t('shell.lane.prop.offline.message'),
              detail: t('shell.lane.prop.offline.detail'),
            })
          } else if (p.source === 'cached') {
            const ageMin = Math.max(0, Math.round((Date.now() / 1000 - p.asOf) / 60))
            setStatus('prop', {
              tier: 'warning',
              message: t('shell.lane.prop.cached.message', { minutes: ageMin }),
              detail: t('shell.lane.prop.cached.detail'),
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
  // These three alerts WATCH the window's one poll of their feeds (features/connectFeeds): the Kp
  // outlook box, Connect's map and its chase boxes read the same feeds, so a box on screen adds no
  // request of its own (each used to be polled twice while one was open). Each watcher runs for the
  // whole session, so its feed is asked for on arrival and every cycle, as before.
  //
  // Storm FORECAST heads-up, app-wide: an alert that only fired while the Kp box is open would not
  // be an alert. stormAlert.ts dedups by the predicted onset time, so this announces once per
  // forecast event however often it is asked.
  useEffect(() => watchFeed(KP_FORECAST, (f) => processStormForecast(f, kpNowRef.current)), [])
  // X-ray fast lane (60 s): flare ONSET reaches the operator in ~1 min instead of
  // the 5-min prop-snapshot cadence. Best-effort — a failed fetch just leaves the
  // snapshot's slower value driving the watcher.
  useEffect(
    () =>
      watchFeed(XRAY_NOW, (x) => {
        xrayFastRef.current = x.flux
        processFlare(effectiveXray(x.flux, null))
      }),
    [],
  )
  // DXpedition best-shot windows for the chase alerts (server-cached climatology;
  // 10 min is generous). Best-effort — without it the loud spotted-alert still
  // works from the snapshot's cards; only the quiet modelled-only toast needs it.
  useEffect(
    () =>
      watchFeed(DXPED_WINDOWS, (list) => {
        dxpedWindowsRef.current = new Map(list.map((w) => [w.call.toUpperCase(), w]))
      }),
    [],
  )
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
  // On a slow disk the engine refuses the read until that QSO is saved; the refusal is
  // asked again a second later (`askAgainWhileSaving`), not left to the poll.
  const refreshNeeds = useCallback(() => {
    askAgainWhileSaving(getNeedAlerts)
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
  // The spots programs on the station's TCI server sent, for the cockpits' band strips: asked when
  // the station says they changed. A click on one only tunes (`handleTciSpot`).
  const tciSpots = useTciSpots(snap?.radio.tciServer?.spots)
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
  // A Needed row heard in JS8, JTTY or a PSK mode opens that screen only while it is on (`ownScreen`).
  const pskEnabled = features.isOn('psk')
  const js8Enabled = features.isOn('js8')
  const jttyEnabled = features.isOn('jtty')
  // Work-a-spot navigation: whenever a spot is worked — from THIS window's
  // boards or a pop-out (which can't navigate the main window itself) — follow
  // to the matching cockpit (the operator's report: "if I click a contact, it
  // should bring me into the right section"). Baselined on the first snapshot
  // so a webview reload never replays the engine's last work action.
  const workNavRef = useRef<number | null>(null)
  useEffect(() => {
    const tick = snap?.workTick ?? 0
    // Native pop-outs share this hint. Independent browser drafts/navigation
    // follow only their own confirmed Work action, never a station broadcast.
    if (workNavRef.current === null) {
      workNavRef.current = tick
      return
    }
    if (tick === workNavRef.current) return
    workNavRef.current = tick
    // The hint is the section the Work told `work_spot` (WORK_SECTION): 'keyboard' opens PSK and
    // 'js8' JS8, the screens a Needed row of those modes works on.
    const target: View = workHintView(snap?.workView)
    // Never navigate into a feature-disabled (hidden) cockpit — same gate as
    // handleWorkNeeded; the rig already switched, the view just stays put.
    if (
      (target === 'cw' && !cwEnabled) ||
      (target === 'phone' && !phoneEnabled) ||
      (target === 'rtty' && !rttyEnabled) ||
      (target === 'psk' && !pskEnabled) ||
      (target === 'jtty' && !jttyEnabled) ||
      (target === 'js8' && !js8Enabled)
    )
      return
    // Sync the rig-mode guard BEFORE navigating (same as handleWorkNeeded) —
    // otherwise the [view] effect sees a mode change and re-homes the dial to
    // the segment start, yanking the rig OFF the exact spot frequency the
    // workSpot click just tuned (review-caught on the pop-out path). The guard
    // holds the screen's RIG mode: PSK's is 'keyboard', JS8's is Digital's.
    lastOpModeRef.current = RIG_MODE_BY_VIEW[target] ?? 'digital'
    lastHomedModeRef.current = lastOpModeRef.current
    // A Work from afar entered its section without arming Enable TX: open its cockpit, and let the
    // rig-mode effect leave the section as that Work left it (`afarEntryRef`).
    afarEntryRef.current = snap?.workAfar === true ? target : null
    setView(target)
    // Prefill the log Call from a work action fired in ANOTHER window (e.g. the pop-out band
    // map), matching the prefill an in-window click gets via handleWorkNeeded. Digital
    // auto-sequences on a decode double-click, so it takes no prefill; RTTY's cockpit takes
    // no prefill either (only the CW/Phone log forms consume pendingWork). A Needed row's park
    // comes with its call (`workPark`): the log line fills both, and no hunt is set.
    const wc = snap?.workCall
    if (wc && (target === 'cw' || target === 'phone')) {
      setPendingWork({ call: wc, view: target, ts: Date.now(), park: snap?.workPark ?? null })
    }
  }, [snap?.workTick, snap?.workView, cwEnabled, phoneEnabled, rttyEnabled, pskEnabled, jttyEnabled, js8Enabled])
  // A COMPUTER DRIVING A STATION SHOWS THE STATION'S SECTION. The screen restored at launch is this
  // computer's last, and the station may be in another section: Phone drawn over a station in FT8
  // sent its band picks as phone bands to the digital section, and the rig came up in DATA. So on
  // an operating screen the view follows the section the station reports, READ-ONLY: nothing is
  // sent, on connect or after, and a station running FT8 is never moved by a computer connecting.
  // Keyed on the station's value alone, so the operator's own pick (sent by the rig-mode effect)
  // is never pulled back by a snapshot taken before the station answered it. A screen with no
  // section of its own (Map, Logbook…) stays put, as does a section whose screen is switched off.
  // PSK and JTTY are one section, so in it the screen is the modem the station names on the air
  // (`keyboardMode`). A station too old to name it reports the section alone, where either keyboard
  // screen is the station's and a screen from elsewhere lands on PSK's, as before.
  const stationSection = snap?.radio.operatingMode
  const stationModem = stationSection === 'keyboard' ? snap?.radio.keyboardMode : undefined
  const stationScreen = stationModem === 'JTTY' ? 'jtty' : stationSection
  useEffect(() => {
    if (!drivesAnotherStation() || !stationScreen) return
    const own = RIG_MODE_BY_VIEW[viewRef.current]
    if (!own) return
    if (own === stationScreen || (!stationModem && own === 'jtty' && stationScreen === 'keyboard')) return
    const target = SCREEN_BY_SECTION[stationScreen]
    if (!target || !features.isOn(target)) return
    lastOpModeRef.current = RIG_MODE_BY_VIEW[target] ?? 'digital'
    lastHomedModeRef.current = lastOpModeRef.current
    afarEntryRef.current = target
    setView(target)
    // The station's section alone: a re-run on the screen changing would pull the operator's pick back.
  }, [stationScreen])
  // Declared here (rather than beside bandPlan below) because the need gate reads it: the
  // band scopes live in settings and everything derived from `needAlerts` sits right below.
  const [settings, setSettings] = useState<Settings | null>(null)
  // #391: the corner pop-ups follow the setting. A settings file without it keeps them on, as they
  // always were.
  useEffect(() => {
    setPopupNotifications(settings?.popupNotifications !== false)
  }, [settings?.popupNotifications])
  // #321: and how long they stay, at least. Absent is 0, each pop-up's own time.
  useEffect(() => {
    setPopupMinSeconds(settings?.popupMinSecs ?? 0)
  }, [settings?.popupMinSecs])
  // The active FD event's ruleset FACTS (banned modes + assistance policy) for the
  // warn-only advisories. get_fd_ruleset reads settings.fd_event itself (and works with
  // the master switch off), so the fetch just re-runs when the configured event changes —
  // and after every save made here (`settingsSaves`), because the same answer carries
  // whether the picked contest can start (`problem`), and a class or a county filled in
  // changes that without changing the event.
  const [fdRuleset, setFdRuleset] = useState<FdRulesetDto | null>(null)
  const [settingsSaves, setSettingsSaves] = useState(0)
  useEffect(() => {
    let live = true
    getFdRuleset()
      .then((r) => live && setFdRuleset(r))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [settings?.fdEvent, settingsSaves])
  // The operator's per-type alert BAND SCOPES (Settings ▸ Spots & Alerts). They gate the
  // need ICONS as well as the sound/toast — "I selected grids, vhf/uhf 6m and up ... and its
  // still showing the grid icons in ft8 in both roster and classic mode when on hf bands"
  // (operator, twice). Undefined until settings load, which is permissive by design.
  const needScopes = useMemo(
    () => ({
      dxcc: settings?.alertDxccBands,
      grid: settings?.alertGridBands,
      rareGrid: settings?.alertRareGridBands,
    }),
    [settings?.alertDxccBands, settings?.alertGridBands, settings?.alertRareGridBands],
  )
  // The operator's GEOGRAPHIC alert scope (#174), resolved to cty.dat entity names. Null until
  // they tick something, and the decode alerter treats null as "alert on everything".
  // The Needed board's feed — band scopes honoured, mode-feature gate neutral (see boardNeeds).
  const boardAlerts = useMemo(() => boardNeeds(needAlerts, needScopes), [needAlerts, needScopes])
  const visibleAlerts = useMemo(
    () => visibleNeeds(needAlerts, { cw: cwEnabled, phone: phoneEnabled }, needScopes),
    [needAlerts, cwEnabled, phoneEnabled, needScopes],
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
  const [bandPlan, setBandPlan] = useState<BandChannel[]>([])
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
      lastHomedModeRef.current = booted // the rig is where the operator left it
      opModeSeeded.current = true
    }
  }, [settings?.operatingMode])
  // Ding/dong when the dial crosses your TX privileges (default on).
  useBandEdgeTones(snap?.radio.txAllowed, settings?.bandEdgeTones ?? true)
  // User watch list (localStorage) — sent to the station, whose alerts and Needed board read it.
  // Re-synced when the manager edits it (it dispatches `nexus:watchlist-changed`), so the station
  // picks up changes live.
  const [watchlist, setWatchlist] = useState<WatchFilter[]>(() => loadWatchlist())
  useEffect(() => {
    const resync = () => setWatchlist(loadWatchlist())
    window.addEventListener('nexus:watchlist-changed', resync)
    return () => window.removeEventListener('nexus:watchlist-changed', resync)
  }, [])
  // The retired "Wanted watch list" setting joins the watch list, once (operator, 2026-09-24:
  // "One list"; features/watchlistFold).
  useEffect(() => {
    void foldRetiredWantedList()
  }, [])
  // The station puts the watch list's stations first on the Needed board (operator, 2026-09-24:
  // "watched counts as needed") and raises its alerts, gates and label with them — so it is sent
  // the list on launch and after every edit, and the board is read again. One send at a time, in
  // order: the station keeps whichever list reaches it last, so an earlier send finishing late
  // would restore an entry the operator has just removed.
  const watchSent = useRef<Promise<unknown>>(Promise.resolve())
  useEffect(() => {
    const entries = watchlist.map(({ id, kind, value, cqOnly, minSnr, label }) => ({ id, kind, value, cqOnly, minSnr, label }))
    watchSent.current = watchSent.current.then(() => setWatchList(entries)).then(refreshNeeds, () => {})
  }, [watchlist, refreshNeeds])
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

  // The station profile changed (the picker, Settings, or an activation picking its park's): the
  // settings copy every screen reads takes the profile's call, grid, state and power.
  const activeStationProfile = snap?.activeStationProfile
  useEffect(() => {
    if (activeStationProfile !== undefined) reloadSettings()
  }, [activeStationProfile, reloadSettings])

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

  // The unread badges are a function of the snapshot, the active peer and the read cursor, and
  // they follow the first two. They need no clock: the cursor moves only in the effect above,
  // after a render that a new snapshot or a new active peer caused, and only for the ACTIVE peer
  // (which never shows a badge) and for threads that are gone (which have none), so a memo can
  // never hold a stale count between snapshots. (A 400 ms ticker used to re-run these, and with
  // them the whole App, 2.5 times a second while nothing changed. It had outlived the mock
  // typing indicator it was written for.)
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
  }, [snap, activePeer])

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
  }, [snap, activePeer])

  const handleSelect = useCallback((call: string) => {
    void withErrorToast(() => apiSelectPeer(call), t('shell.error.selectStation')).then(
      (s) => s && setSnap(s),
    )
  }, [])

  // Confirm here rather than in StationList so every host (cockpit, detached panel) gets
  // the same guard. Unconditional is right: the recents list only renders threads that
  // have messages, so there is no empty-thread case to skip. The copy names the
  // non-obvious consequence — deleting also cancels still-queued outbound traffic.
  const handleArchive = useCallback(async (peer: string) => {
    if (
      !(await confirmDialog({
        title: t('shell.conversation.delete.title', { peer }),
        body: t('shell.conversation.delete.body'),
        confirmLabel: t('shell.conversation.delete.action'),
        danger: true,
      }))
    )
      return
    void withErrorToast(
      () => apiArchiveConversation(peer),
      t('shell.conversation.delete.failed'),
    ).then((s) => s && setSnap(s))
  }, [])

  // The Map and the roster share ONE selection: the active peer. Clicking a map
  // dot selects (or, if already selected, clears) that station — and the roster
  // highlights it too, since StationList already keys its highlight off activePeer.
  const handleMapSelect = useCallback((call: string | null) => {
    void withErrorToast(() => apiSelectPeer(call), t('shell.error.selectStation')).then(
      (s) => s && setSnap(s),
    )
  }, [])

  // Pounce — the app's one PUSH channel. The earcon has already played inside the hook by the
  // time `pounceAlert` is set; this is the visual half plus the one-click work.
  const { alert: pounceAlert, dismiss: dismissPounce } = usePounce()
  // Signed self-update: downloads quietly, installs only on an explicit press that the engine
  // refuses while the radio is busy (see useSelfUpdate / update_install_block). It is also the
  // one owner of the launch's update prompt: the old "update available" notice speaks only
  // where the self-updater cannot (features/updateCheck.ts).
  const selfUpdate = useSelfUpdate(!!settings?.betaUpdates)
  const handlePounceWork = useCallback(
    (a: PounceAlert) => {
      // Route through the SAME path the needed board uses, so QSY, rig mode and pileup-split
      // handling behave identically to working any other spot. A separate path here would drift.
      const kind =
        a.mode === 'CW' ? 'cw' : a.mode === 'Phone' ? 'phone' : ('digital' as const)
      if (a.freqMhz) {
        workSpot(kind as 'digital' | 'phone' | 'cw', a.freqMhz, a.band, a.call)
          .then((snap) => setSnap(snap))
          .catch(() => pushToast(t('shell.pounce.qsy.failed', { call: a.call }), 'error'))
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
      if (me && sameCall(call, me)) {
        pushToast(t('shell.ownCall', { call }), 'info', 2500)
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
        void withErrorToast(() => apiSelectPeer(call), t('shell.tempo.open.failed', { call })).then((s) => {
          if (s) setSnap(s)
        })
        pushToast(t('shell.tempo.opened', { call }), 'success', 3000)
        return
      }
      void withErrorToast(
        () => apiCallStation(call, grid, message, snr, freq),
        t('shell.work.failed', { call }),
      ).then((s) => {
        if (s) {
          setSnap(s)
          // Work the station on the single-screen Operate cockpit — the QSO
          // sequences inline there while the waterfall + decodes stay visible.
          // (Never bounce to the chat-style 'qso' view and lose the band.)
          setView('operate')
          // Immediate confirmation the action took (and TX is now armed for it).
          pushToast(t('shell.work.started', { call }), 'success', 4000)
        }
      })
    },
    [handleWorkspace],
  )

  // The Stations cards take `handleCall` DIRECTLY — there is deliberately no adapter here any
  // more. A card hands over the callsign, the station's GRID, its audio offset and its
  // protocol tier, in the shared handler's own argument order, so nothing in between can drop
  // one. The offset is the RX/TX move, not sequencing data: passing it puts the marks on the
  // station the same way Band Activity's double-click and the Roster table already do
  // (WSJT-X: RX always follows, TX follows unless Hold Tx Freq is on).
  //
  // What stood here was an adapter that re-typed the callback as (call, tier) and passed
  // undefined for grid/message/snr/freq, on the stated grounds that "a card knows only the
  // callsign + its protocol tier". That was wrong — a card is handed the whole Station — and
  // it is why Work started the QSO but left the markers behind while the other two panes
  // moved them: one gesture, three behaviours in one cockpit (#183, against 1.7.0–1.9.0).
  // An adapter is what made the drop possible, so the fix is to not have one.
  // The station's decode alerts (someone calling you, a new DXCC/grid, a watch-list station, a CQ,
  // a park on the air): the station decides them and raises each once; this window shows them. The
  // toast's button works the station — what you almost always want next — exactly as double-
  // clicking its decode row does. Placed AFTER handleCall so it's in scope.
  useStationAlerts(
    useCallback(
      (a: NeededAlert) => {
        // Pass the decode's TIER — without it an FT1/Tempo decode alert ran the FT8
        // call sequence (the "Work on FT1 wrongly switches to FT8" bug); handleCall
        // routes chat tiers to the Tempo conversation instead.
        if (a.call && a.work) handleCall(a.call, undefined, a.work.message, a.work.snr, a.work.freqHz, a.work.tier)
      },
      [handleCall],
    ),
  )
  // The eyes-free cues of each new look at the decode window (the decode tick, the spoken batch
  // summary), and the refs the rest of the app reads off the snapshot.
  useEffect(() => {
    if (!snap || !settings) return
    // Keep the chase-alert suppression in sync (the prop poller reads the ref).
    qsoPartnerRef.current = snap.fieldDay?.dxcall ?? snap.qso?.dxcall ?? null
    // Latest tier for the rail's Digital button (preserve FT4 across nav).
    tierRef.current = snap.link.tier
    processDecodes(snap.recentDecodes, settings)
  }, [snap, settings])

  // Bumps when a QSO is logged AND "Clear DX call after logging" is on — the
  // cockpit watches it and wipes its DX Call/Grid fields (stock WSJT-X option).
  // Driven off the engine's loggedTick so EVERY log path fires it, including a
  // backend auto-log the frontend never initiated (#210) — the old approach
  // intercepted only the frontend's own log actions and so missed the auto-log.
  //
  // The needs refresh rides the same tick for the same reason (#350): only the Log button
  // and the confirm popup asked for one, so a contact the sequencer logged after RR73 kept
  // its pre-QSO need tags — and "Needed only" / "Hide worked" kept the station just worked —
  // until the 30 s poll came round.
  const [dxClearTick, setDxClearTick] = useState(0)
  const prevLoggedTick = useRef<number | null>(null)
  useEffect(() => {
    const tick = snap?.loggedTick
    if (tick == null) return
    if (prevLoggedTick.current == null) {
      prevLoggedTick.current = tick // adopt the first value — a fresh mount is not a log event
      return
    }
    if (tick !== prevLoggedTick.current) {
      prevLoggedTick.current = tick
      if (settings?.clearDxAfterLog) setDxClearTick((t) => t + 1)
      refreshNeeds()
    }
  }, [snap?.loggedTick, settings?.clearDxAfterLog, refreshNeeds])

  // The station refused the answer (its `expectedKey` is not the head's any more) or the call
  // failed: RE-READ rather than leave the operator looking at a hold that has moved on. Nothing
  // is logged or discarded on this path — the engine already refused it.
  const reloadAfterRefusal = useCallback(() => {
    getSnapshot().then(setSnap).catch(() => {})
  }, [])

  // The engine refuses an answer that names a hold which is no longer the head with a CODE, not
  // a sentence — and the wording is OURS. `withErrorToast` appends the backend's text raw after
  // the translated fallback, so a sentence written in Rust would reach a German or Japanese
  // operator in English, and would exist in two places at once. Same shape as the Remote
  // refusals (`remoteBusy`, `staleContext`). Anything else keeps the generic toast, with
  // whatever the backend said, because an unknown failure must not be dressed up as a known one.
  const toastPendingLogError = useCallback((err: unknown, fallback: string) => {
    const code = err instanceof Error ? err.message : typeof err === 'string' ? err : ''
    if (code === 'pendingLogMovedOn') pushToast(t('shell.log.movedOn'), 'error')
    else pushToast(code ? `${fallback}: ${code}` : fallback, 'error')
  }, [])

  const handleConfirmLog = useCallback(
    (record: LoggedQso) => {
      void apiConfirmPendingLog(record, snap?.pendingQsoLogKey)
        .then((s) => {
          setSnap(s)
          refreshNeeds() // drop the just-worked station from the roster/needs immediately
          // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
          // (every log path, auto-log included); outcomes toast via uploadTick.
        })
        .catch((err: unknown) => {
          toastPendingLogError(err, t('shell.log.failed'))
          reloadAfterRefusal()
        })
    },
    [refreshNeeds, reloadAfterRefusal, toastPendingLogError, snap?.pendingQsoLogKey],
  )

  const handleDiscardLog = useCallback(() => {
    void apiDiscardPendingLog(snap?.pendingQsoLogKey)
      .then(setSnap)
      .catch((err: unknown) => {
        toastPendingLogError(err, t('shell.log.discard.failed'))
        reloadAfterRefusal()
      })
  }, [reloadAfterRefusal, toastPendingLogError, snap?.pendingQsoLogKey])

  const handleSend = useCallback(
    (text: string) => {
      if (!activePeer) return
      void withErrorToast(
        () => apiSendMessage(activePeer, text),
        t('shell.message.failed'),
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
    void withErrorToast(() => apiSelectPeer('*'), t('shell.bandFeed.failed')).then(
      (s2) => s2 && setSnap(s2),
    )
  }, [])

  // Tap-to-resend a terminal (no-ack / abandoned) bubble: same text, fresh cycle budget,
  // same bubble (the backend re-points it at the new queue entry).
  const handleResend = useCallback((m: import('./types').ChatMessage) => {
    const peer = m.to
    if (!peer) return
    void withErrorToast(() => resendChat(peer, m.ackId ?? null), t('shell.resend.failed', { peer })).then(
      (s) => {
        if (s) {
          setSnap(s)
          pushToast(t('shell.resend.sending', { peer }), 'success', 3000)
        }
      },
    )
  }, [])

  const handleBroadcast = useCallback(
    (text: string) => {
      void withErrorToast(() => apiBroadcast(text), t('shell.broadcast.failed')).then(surfaceBandFeed)
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
    void withErrorToast(() => apiSetChatCq(true), t('shell.cq.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleToggleCqRun = useCallback(() => {
    const next = (snap?.chatCq ?? 'off') === 'off'
    void withErrorToast(() => apiSetChatCq(next), t('shell.cqRun.toggle.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [snap?.chatCq])

  const handleResumeCqRun = useCallback(() => {
    void withErrorToast(() => apiResumeChatCq(), t('shell.cqRun.resume.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleToggleBeacon = useCallback(() => {
    const next = !(snap?.radio.beacon ?? false)
    void withErrorToast(() => apiSetBeacon(next), t('shell.beacon.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [snap?.radio.beacon])

  const handleSetFrequency = useCallback(
    (dialMhz: number, band: string, mode: string) => {
      void withErrorToast(
        () => apiSetFrequency(dialMhz, band, mode),
        t('shell.frequency.failed'),
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
      void withErrorToast(() => apiSetBlockedCalls(next), t('shell.blocklist.failed')).then(
        (s2) => {
          if (s2) setSnap(s2)
          setSettings((prev) => (prev ? { ...prev, blockedCalls: next } : prev))
        },
      )
    },
    [],
  )

  // THE JS8 LISTS' SETTINGS (Settings ▸ JS8) for the cockpit, and its call list's three writes: each
  // one field through its own verb, never the settings form, and the mirror patched with what was
  // saved. The hosted page writes neither.
  const js8Lists: Js8ListSettings | undefined = settings
    ? {
        rxBlocklist: settings.js8RxBlocklist ?? [],
        highlightPrimary: settings.js8HighlightPrimary ?? [],
        highlightSecondary: settings.js8HighlightSecondary ?? [],
        eot: settings.js8Eot ?? '\u2662',
        mfi: settings.js8Mfi ?? '\u2026\u2026',
        sounds: settings.js8Sounds ?? [],
        groups: settings.js8Groups ?? [],
        avoidAllcall: settings.js8AvoidAllcall ?? false,
        setRxBlocklist: async (calls: string[]) => {
          const saved = await withErrorToast(() => js8SetRxBlocklist(calls), t('js8.lists.saveFailed'))
          if (saved) setSettings((prev) => (prev ? { ...prev, js8RxBlocklist: saved } : prev))
        },
        setGroups: async (groups: string[]) => {
          const saved = await withErrorToast(() => js8SetGroups(groups), t('js8.lists.saveFailed'))
          if (saved) setSettings((prev) => (prev ? { ...prev, js8Groups: saved } : prev))
        },
        setAvoidAllcall: async (avoid: boolean) => {
          const saved = await withErrorToast(() => js8SetAvoidAllcall(avoid), t('js8.lists.saveFailed'))
          if (saved !== null) setSettings((prev) => (prev ? { ...prev, js8AvoidAllcall: saved } : prev))
        },
      }
    : undefined

  const handleSstvTune = useCallback(
    (dialMhz: number, band: string, mode: string) => {
      void withErrorToast(
        () => apiSstvTune(dialMhz, band, mode),
        t('shell.frequency.failed'),
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
      t('shell.frequency.failed'),
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // APRS Tune: QSY to the APRS frequency on 2 m FM simplex, auto-routing to the 2 m-capable radio.
  // A dedicated verb (not handleSetFrequency) because APRS isn't an operating section, so the rig
  // must be put in FM explicitly — otherwise it keeps the prior section's DATA/USB mode and the
  // 2 m packet never demodulates. Clearing the mode override is handled backend-side on the next QSY.
  const handleAprsTune = useCallback((dialMhz: number) => {
    void withErrorToast(() => aprsTune(dialMhz), t('shell.aprs.tune.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxEnabled = useCallback((enabled: boolean) => {
    void withErrorToast(
      () => apiSetTxEnabled(enabled),
      enabled ? t('shell.tx.enable.failed') : t('shell.tx.mute.failed'),
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxLevel = useCallback((level: number) => {
    void withErrorToast(() => apiSetTxLevel(level), t('shell.tx.level.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTune = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetTune(on), t('shell.tune.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // The rig's own ATU, from the TX strips App draws the handlers for (Tempo, APRS). A control that
  // keys the transmitter never fails silently: the backend returns every refusal (TX off, busy,
  // lockout, no tuner) WITH ITS REASON, and the toast shows it.
  const handleAtuTune = useCallback(() => {
    void apiAtuTune()
      .then((s) => setSnap(s))
      .catch((e) => pushToast(String(e), 'error'))
  }, [])
  // The click on ATU ON (#344): the tuner out of line. Keys nothing; refused on the air.
  const handleAtuBypass = useCallback(() => {
    void apiAtuBypass()
      .then((s) => setSnap(s))
      .catch((e) => pushToast(String(e), 'error'))
  }, [])

  // Prove-TX-path: key the tune carrier for ~2.5 s then auto-drop — a BOUNDED transmit that lets the
  // operator verify CAT→PTT→RF (forward power registers). Always invoked from behind a confirm
  // dialog (operator's TX-approval condition); the TX watchdog backs up the auto-unkey.
  const handleProveTx = useCallback(() => {
    void withErrorToast(() => apiSetTune(true), t('shell.proveTx.failed')).then((s) => {
      if (s) setSnap(s)
      window.setTimeout(() => {
        void apiSetTune(false)
          .then((s2) => s2 && setSnap(s2))
          .catch(() => {})
      }, 2500)
    })
  }, [])

  const handleHaltTx = useCallback(() => {
    void withErrorToast(() => apiHaltTx(), t('shell.halt.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Dual-radio: switch the active radio (rig loop swaps rigs, carrier dropped first) and toggle
  // peg-lock. The returned snapshot echoes the new active radio + tune instantly.
  const handleSetActiveRadio = useCallback((id: number) => {
    void withErrorToast(() => apiSetActiveRadio(id), t('shell.radio.switch.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // The top bar's "My station" picker. The snapshot answers with the new call; the settings copy
  // follows below, from the active id the snapshot carries.
  const handleSwitchStationProfile = useCallback((id: string) => {
    void withErrorToast(() => apiSwitchStationProfile(id), t('shell.stationProfile.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetPegLock = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetPegLock(on), t('shell.pegLock.failed')).then((s) => {
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
    if (me && sameCall(call, me)) {
      pushToast(t('shell.ownCall', { call }), 'info', 2500)
      return
    }
    void withErrorToast(
      () => apiOverrideNextTx(call, grid, text),
      t('shell.overrideTx.failed', { call }),
    ).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxEven = useCallback((even: boolean) => {
    void withErrorToast(() => apiSetTxEven(even), t('shell.txPeriod.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleSetTxCycleAuto = useCallback((auto: boolean) => {
    void withErrorToast(() => apiSetTxCycleAuto(auto), t('shell.cycleMode.failed')).then(
      (s) => {
        if (s) setSnap(s)
      },
    )
  }, [])

  const handleSetHoldTxFreq = useCallback((on: boolean) => {
    void withErrorToast(() => apiSetHoldTxFreq(on), t('shell.holdTx.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  // Waterfall click: left-click sets RX only; shift/right-click sets TX;
  // Ctrl/Command sets both. Hold Tx belongs to decode/QSO selection, not this click.
  const handleTune = useCallback((hz: number, target: 'tx' | 'rx' | 'both') => {
    // Stock WSJT-X gestures (Waterfall dispatches): 'rx' = click, 'tx' = Shift, 'both' = Ctrl.
    const call =
      target === 'rx'
        ? () => apiSetRxOffset(hz)
        : target === 'tx'
          ? () => apiSetTxOffset(hz)
          : () => apiSetTxOffset(hz).then(() => apiSetRxOffset(hz))
    void withErrorToast(call, t('shell.offset.failed')).then((s) => {
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
        pushToast(t('shell.qsy.noChannel', { band }), 'error', 3000)
        return
      }
      const dial = freqMhz ?? ch.dialMhz
      void withErrorToast(
        () => apiSetFrequency(dial, ch.band, ch.mode),
        t('shell.qsy.failed', { band }),
      ).then((s) => {
        if (s) {
          setSnap(s)
          pushToast(t('shell.qsy.done', { dial: dial.toFixed(3) }), 'success', 2500)
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
          t('shell.recall.sectionOff', { section: target === 'cw' ? 'CW' : 'Phone' }),
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
            pushToast(t('shell.recall.failed'), 'error', 3000)
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
          lastHomedModeRef.current = opMode // recalled to an EXACT dial — do not re-home
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
              ? t('shell.recall.done.setMode', {
                  name: m.name,
                  freq: plan.freqMhz.toFixed(3),
                  mode: plan.mode,
                })
              : t('shell.recall.done', {
                  name: m.name,
                  freq: plan.freqMhz.toFixed(3),
                  mode: plan.mode,
                }),
            'success',
            2500,
          )
        } catch (e) {
          pushToast(t('shell.recall.error', { error: `${e instanceof Error ? e.message : e}` }), 'error', 4000)
        }
      })()
    },
    [cwEnabled, phoneEnabled],
  )

  // Global quick-recall hotkeys: Ctrl+1..9 (or ⌘+1..9 — the native chord on macOS, where
  // Ctrl+digit is Mission Control's) recall the 1st..9th ★ favorite from ANY
  // section (the same action as clicking its MEM-strip chip — recallMemory tunes +
  // switches to the right cockpit). Rides a ref so the listener binds once and always
  // sees the latest recallMemory + enabled flag; favorites are read fresh at press time.
  // Ctrl/⌘+Digit is clear of the cockpits' Alt+Digit (FT8 Tx) and F-key macros.
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
          t('shell.net.reminder', {
            until: untilPhrase(r.startMs, now),
            name: r.memory.name,
            freq: r.memory.rxMhz.toFixed(3),
            mode: r.memory.mode,
          }),
          'info',
          300_000,
          { prominent: true, action: () => recallMemory(r.memory), actionLabel: t('shell.net.tune') },
        )
      }
    }
    check()
    const id = window.setInterval(check, 30_000)
    return () => window.clearInterval(id)
  }, [recallMemory])

  // Click-to-work ANY needed spot (CW / Phone / Digital): a single click that
  // changes the band, mode, AND frequency to exactly the spot's — in ONE atomic backend call
  // (`workSpot`) so the rig can never end up in the new mode at the old dial (no wrong-mode
  // flash) and the UI never sees a half-applied mode/freq state. Then open the matching
  // cockpit and — for CW/Phone — hand it the callsign to prefill the log. A need with no
  // Point the antenna rotator at a needed call (great-circle bearing from your grid).
  const handlePointAntenna = useCallback(async (call: string, longPath = false) => {
    try {
      // A browser gets nothing back: the station resolves it.
      const pointed: PointedAt | null | undefined = await pointRotatorAtCall(call, longPath)
      // ⚠️ THE TOAST NAMES THE PATH. A heading with no path is half an answer — the same
      // reason `azimuthTitle` says "short path" out loud on every bearing Nexus displays.
      //
      // ⚠️ BOTH KEYS WRITTEN OUT, never `t(computedKey, …)`. A key built in a variable is
      // invisible to the catalog scanners: `hardcoded-strings` reports both entries as
      // orphans nobody references, and `placeholders` counts another unreadable call site
      // against a ratchet that only goes down. Two literal `t(…)` calls cost one line.
      // …and the PARAMS are object literals at each call site for the same reason: the
      // guard counts a call site unreadable when the key OR the params come from a
      // variable, so hoisting them into `where` traded one unreadable site for another.
      // …and what the bearing was taken to: the station's grid, or only its country's centre.
      pushToast(
        pointed == null
          ? t('remote.b1.rotatorPointing', { call })
          : longPath
            ? t('shell.rotator.pointedLong', { bearing: Math.round(pointed.bearing), call, to: pointedTo(pointed) })
            : t('shell.rotator.pointed', { bearing: Math.round(pointed.bearing), call, to: pointedTo(pointed) }),
        'success',
        3000,
      )
    } catch (e) {
      pushToast(typeof e === 'string' ? e : t('shell.rotator.failed', { call }), 'error', 4000)
    }
  }, [])

  // resolvable frequency at all falls back to a plain band QSY.
  const handleWorkNeeded = useCallback(
    async (alert: NeedAlert) => {
      // A row heard in JS8, JTTY or a PSK mode works on that mode's own screen (`ownScreen`) while it is
      // switched on, at the row's own frequency or that screen's channel for the band.
      const screen = ownScreen(alert)
      let own: OwnScreen | null = null
      if (screen && (screen === 'js8' ? js8Enabled : screen === 'jtty' ? jttyEnabled : pskEnabled)) {
        const plan = await withErrorToast(
          () => getLicensedBandPlan(screen),
          t('shell.work.failed.cat', { call: alert.call }),
        )
        if (!plan) return
        own = { view: screen, plan }
      }
      // `target`, not `t` — `t` is the translator in this file.
      const target = workTarget(alert, bandPlan, own)
      if (!target) {
        handleQsy(alert.band)
        return
      }
      // A POTA/SOTA row names the activation it IS (`park`), and the contact this leads to is
      // logged with the park. Without it a contact worked from the Needed board was an ordinary
      // QSO: no hunter credit, no SIG/SIG_INFO, and nothing for the POTA board's Hide worked to see.
      //
      // ⚠️ WHERE A LOG LINE TAKES THE PARK, IT IS NOT A HUNT (operator, 2026-10-10: "It should
      // instead simply populate the call data (and the park number) without setting it as
      // 'Hunted'. Hunted should only be used if you chase a park from the POTA tab."). A row that
      // opens the Phone or CW cockpit hands its park to that cockpit's log line with the call
      // (`pendingWork.park`), so nothing waits at the top of the POTA/SOTA view and the line's
      // Clear really clears it. A hunt set here outlived a station the operator never heard by four
      // hours (HUNT_TTL_SECS), and only that view could end it. A row with no log line to fill
      // (FT8/FT4 logs from the engine, RTTY's cockpit takes no handoff, a switched-off cockpit is
      // never opened) still tags the hunt before the QSY, the map's setHuntTarget-then-QSY order:
      // that is the only way its park reaches the contact. Native only: a browser's Work never
      // carried a park.
      //
      // AFTER that bail-out, never before it. `workTarget` is null only when the row has no
      // frequency of its own, no mode default AND no band-plan channel — and `handleQsy` reads
      // that same missing channel, so it toasts "no channel" and the rig does not move. Tagging
      // there armed a four-hour pend (HUNT_TTL_SECS) for a QSY that provably did not happen,
      // waiting to stamp that park on the next contact with the call, wherever it was made.
      //
      // AWAITED, and its failure said out loud: `set_hunt_target` rejects whenever the feed's
      // spelling of the reference will not normalize, and the swallowed rejection left the
      // operator watching the QSY land, working the park, and logging it with no reference at
      // all. The work still proceeds on a failed tag — the station is real and the operator can
      // add the reference by hand; refusing the QSY would be the bigger surprise.
      const park = alert.park ?? null
      const fillsLogLine = (target.view === 'cw' && cwEnabled) || (target.view === 'phone' && phoneEnabled)
      if (park && !fillsLogLine) {
        await withErrorToast(
          () => setHuntTarget(alert.call, park.program, park.reference),
          t('ota.hunt.setFailed', { call: alert.call }),
        )
      }
      // The Needed board now lists ALL modes (W1), but the CW/Phone cockpits are opt-in
      // features. If the target cockpit is disabled, don't navigate into a hidden view
      // (that dumped the operator on the landing page) — just QSY the rig to the spot.
      if ((target.view === 'cw' && !cwEnabled) || (target.view === 'phone' && !phoneEnabled)) {
        handleQsy(alert.band)
        return
      }
      // RTTY is a Digital submode with an opt-in cockpit. When it's disabled, don't strand
      // the operator on a hidden view — QSY to the spot's EXACT frequency (RTTY spots always
      // carry one) so they can still work it from wherever they are.
      if (target.view === 'rtty' && !rttyEnabled) {
        handleQsy(alert.band, target.freqMhz)
        return
      }
      // The section `work_spot` is told (WORK_SECTION): 'operate' is the digital cockpit, so
      // 'digital'; RTTY passes through as 'rtty' and PSK is 'keyboard', whose policies the backend
      // + [view] rig-mode effect apply; JS8 is 'js8', Digital under its own name so the hint every
      // window follows says JS8. The guards below hold the screen's rig mode.
      const section = WORK_SECTION[target.view]
      const rigMode: RigMode = RIG_MODE_BY_VIEW[target.view] ?? 'digital'
      void (async () => {
        // A digital spot carries its protocol in its own mode (`workTier`) — the FT8/FT4 tier
        // rides the SAME atomic workSpot call, else clicking an FT4 spot QSYs but leaves the
        // decoder on FT8. It used to be a separate set_tier call first, and in the gap between
        // the two the radio loop could command the tier's DEFAULT dial (FT8 14.074 → FT4
        // 14.080) to the rig before the spot's exact frequency — the "hits a default
        // first, then switches" double-retune, whose stale read-back could race the
        // correction on slow serial and leave the wrong dial standing (operator report,
        // 2026-08-09). tierRef syncs from the returned snapshot (setSnap → link.tier).
        // JS8's screen rides the same call as the JS8 tier.
        let tier: 'FT8' | 'FT4' | 'JS8' | undefined
        const named = target.view === 'js8' ? 'JS8' : target.view === 'operate' ? workTier(alert) : null
        if (named && tierRef.current !== named) tier = named
        // The park rides the work too, so the work hint below says the same as this click, as it
        // does for a pop-out board's: the snapshot's echo of a park row must not hand the log line
        // the call alone.
        const s = await withErrorToast(
          () =>
            park && fillsLogLine
              ? workSpot(section, target.freqMhz, target.band, target.call, tier, park)
              : workSpot(section, target.freqMhz, target.band, target.call, tier),
          t('shell.work.failed.cat', { call: target.call }),
        )
        // On failure DON'T navigate or poison the guard ref — the backend made no change
        // (atomic), so the view-effect can still apply the mode on a later nav.
        if (!s) return
        setSnap(s)
        // Keep the rig-mode effect's guards in sync so it doesn't re-fire on the nav — and
        // the HOMED guard too, because this landed the dial on purpose (#143).
        lastOpModeRef.current = rigMode
        lastHomedModeRef.current = rigMode
        // The PSK screen opens on the row's own sub-mode where it has it (`pskSubmode`), its Rev
        // setting kept. The engine refuses the switch while PSK is transmitting, and says why.
        const slug = target.view === 'psk' ? pskSubmode(alert) : null
        if (slug) {
          const psk = await withErrorToast(() => getPskState(), t('psk.mode.failed'))
          if (psk && psk.mode !== slug) {
            await withErrorToast(() => pskSetMode(slug, psk.reverse === true), t('psk.mode.failed'))
          }
        }
        // CW/Phone log forms consume a prefill; the digital + RTTY cockpits don't (digital
        // auto-sequences on a decode double-click, RTTY has its own net picker) — just the QSY.
        if (target.view === 'cw' || target.view === 'phone') {
          setPendingWork({ call: target.call, view: target.view, ts: Date.now(), park })
        }
        setView(target.view)
        pushToast(
          t('shell.work.ready', { call: target.call, mode: alert.mode, band: target.band }),
          'success',
          4000,
        )
      })()
    },
    [bandPlan, handleQsy, cwEnabled, phoneEnabled, rttyEnabled, pskEnabled, jttyEnabled, js8Enabled],
  )

  // Work a spot double-clicked on the MAP — the same atomic path as the Needed
  // board (workSpot → rig jumps band+mode+freq, cockpit opens). The source-reported
  // mode routes the cockpit: CW→CW, SSB/FM→Phone, FT8/unknown→Digital.
  const handleWorkMapSpot = useCallback(
    // `spot`, not `t` — `t` is the translator in this file.
    async (spot: { call: string; band: string; mode: string | null; freqMhz: number | null; program?: string; reference?: string }) => {
      const alert: NeedAlert = {
        call: spot.call,
        entity: '',
        band: spot.band,
        zone: 0,
        tags: [],
        priority: 0,
        headline: '',
        // Keep FT4/FT8 specific so the tier-switch guard fires (same as handleWorkSpot);
        // everything else collapses to its class for cockpit routing.
        mode: spot.mode === 'FT4' || spot.mode === 'FT8' ? spot.mode : modeClassOf(spot.mode),
        freqMhz: spot.freqMhz,
      }
      // Tag the hunt target BEFORE the QSY — same order as the POTA/SOTA board's own
      // setHuntTarget-then-QSY split (handleHuntSpot below) — so a park worked from the
      // map credits the activator too, not just the QSY.
      //
      // ONLY WHEN THE QSY WILL HAPPEN. `handleWorkNeeded` bails out on a row with no workable
      // frequency, and `handleQsy` reads that same missing band-plan channel, so the rig does not
      // move; a tag set regardless armed a four-hour pend (HUNT_TTL_SECS) for a QSY that provably
      // did not happen, waiting to stamp that park on the next contact with the call. Awaited and
      // reported for the reasons `handleWorkNeeded` spells out — `set_hunt_target` rejects on a
      // reference it cannot normalize, and that used to be swallowed.
      if (spot.program && spot.reference && workTarget(alert, bandPlan)) {
        const { program, reference } = spot
        await withErrorToast(
          () => setHuntTarget(spot.call, program, reference),
          t('ota.hunt.setFailed', { call: spot.call }),
        )
      }
      void handleWorkNeeded(alert)
    },
    [handleWorkNeeded, bandPlan],
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
        t('shell.work.failed.cat', { call: s.call }),
      ).then((snap) => {
        if (!snap) return
        setSnap(snap)
        pushToast(
          t('shell.work.here', { call: s.call, band: s.band, freq: s.freqMhz.toFixed(3) }),
          'success',
          3000,
        )
      })
    },
    [],
  )
  const workSpotHerePhone = useMemo(() => handleWorkSpotHere('phone'), [handleWorkSpotHere])
  // ⛔ A click on a program's spot (Share by TCI) in a cockpit's band strip: the station tunes there
  // and tells every program, and that is all. Never the Work above, whose section entry arms.
  const handleTciSpot = useCallback((s: SpotRow) => {
    void withErrorToast(
      () => clickTciSpot(s.call, s.freqMhz),
      t('shell.pounce.qsy.failed', { call: s.call }),
    ).then((next) => {
      if (!next) return
      setSnap(next)
      pushToast(t('shell.work.here', { call: s.call, band: s.band, freq: s.freqMhz.toFixed(3) }), 'success', 3000)
    })
  }, [])
  const workSpotHereCw = useMemo(() => handleWorkSpotHere('cw'), [handleWorkSpotHere])

  const handleWorkSpot = useCallback(
    (s: SpotRow) => {
      // `spotNeed` forwards the SPECIFIC digital submode (FT4/FT8) so handleWorkNeeded's
      // tier-switch guard fires — else clicking an FT4 spot QSYs but leaves the decoder on FT8.
      handleWorkNeeded(spotNeed(s))
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
    return withErrorToast(() => apiSetMode(mode), t('shell.error.switchMode')).then((s) => {
      if (s) setSnap(s)
      // A contest that starts about to send the DX exchange from a US or Canadian call says so,
      // loudly and once, as it starts (the strip keeps saying it). Never a refusal.
      const warning = contestStartWarning(mode, s)
      if (warning) pushToast(warning, 'info', 15_000, { prominent: true })
    })
  }, [])

  const handleQsoResend = useCallback(() => {
    void withErrorToast(() => apiQsoResend(), t('shell.resend.qso.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleQsoFreetext = useCallback((text: string) => {
    void withErrorToast(() => apiQsoFreetext(text), t('shell.freetext.failed')).then((s) => {
      if (s) setSnap(s)
    })
  }, [])

  const handleLogCurrent = useCallback(() => {
    void withErrorToast(() => apiLogCurrentQso(), t('shell.log.failed')).then((r) => {
      if (r) {
        setSnap(r.snapshot)
        // The engine's verdict, not the call returning (#100): every false is a deliberate
        // logbook-integrity refusal (already logged, no active QSO, no report exchanged),
        // and a green "Logged QSO" over one claimed a write that never happened.
        if (r.logged && r.alreadyWaiting && r.call) {
          // Nothing new was held: that station's offer already waits in the popup.
          pushToast(t('shell.toast.logWaiting', { call: r.call }), 'info', 4000)
        } else if (r.logged) {
          pushToast(r.call ? t('shell.toast.loggedCall', { call: r.call }) : t('shell.toast.logged'), 'success', 2500)
          refreshNeeds() // drop the just-worked station from the roster/needs immediately
          // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
          // (every log path, auto-log included); outcomes toast via uploadTick.
        } else if (!r.pending) {
          pushToast(t('shell.toast.nothingToLog'), 'info', 4000)
        }
      }
    })
  }, [refreshNeeds])

  // Selecting a view from the nav. QSO / Field Day also request the backend mode
  // (defaulting to the "run" / "chat" role); Settings are pure UI
  // screens that leave the operating mode unchanged.
  // Where a Settings deep link asked the panel to land. Cleared on the way OUT of Settings so a
  // later plain visit (the rail gear) opens on Station as it always has, rather than returning to
  // wherever the last pointer sent the operator. `seq` counts the requests, so asking again for
  // the place Settings already holds still moves the panel: the operator may have changed tab
  // since, and the Getting started guide opens over Settings itself.
  const [settingsTarget, setSettingsTarget] = useState<{ where: string; seq: number } | undefined>(
    undefined,
  )

  // The contest re-entry an opened screen asked for, until the engine answers. Until then the snapshot
  // still shows the station out of the contest, and the CW and Phone log lines must not call it stopped.
  const [contestReentering, setContestReentering] = useState(false)

  const handleView = useCallback(
    (next: View) => {
      setView(next)
      if (next !== 'settings') setSettingsTarget(undefined)
      // Passive-first: entering QSO / Field Day starts in Search-&-Pounce
      // (listen + answer), never auto-calling CQ. The operator hits "Call CQ" /
      // "Running" in the panel to start transmitting.
      if (next === 'chat') handleSetMode('chat')
      // The contest only while its mode is on. With the mode off this is the screen that turns
      // it on, and a session entered behind the master switch is one the snapshot hides: a
      // Running started from it would call CQ unseen.
      // And only from another mode: `set_mode` replaces the mode wholesale, so re-entering S&P
      // from inside the contest stopped a run's CQs and dropped the contact being worked —
      // every time the operator looked at the score. A station already in the contest keeps
      // its role.
      // The CW and Phone screens too: their log line IS the contest's while the session runs, and
      // with the mode on and the session dropped (a trip to Chat, a tier switch) it fell back to the
      // everyday form, with no exchange boxes and no dupe check.
      else if (
        (next === 'fieldDay' || next === 'cw' || next === 'phone') &&
        settings?.fdActive === true &&
        snap?.mode !== 'fieldDay'
      ) {
        setContestReentering(true)
        void handleSetMode('fieldday-sp').finally(() => setContestReentering(false))
      }
    },
    [handleSetMode, settings?.fdActive, snap?.mode],
  )

  // ⭐ "WORK THIS CONTEST IN CW / IN PHONE" FROM A CONTEST WINDOW (features/contestWorkIn): the same switch
  // as the rail's, so the contest's S&P is entered where the session dropped and a station already in
  // the contest keeps its role, and then this window comes to the front, where the operator asked to be.
  // The Contest screen here passes `handleView` itself.
  useWorkInRequests((next) => {
    handleView(next)
    void focusWindow().catch(() => {})
  })

  // ⭐ CTRL+O FROM A CONTEST STRIP (`features/contestKeys.ts`): the Contest screen, with an ask its
  // operator box answers by taking the caret. Only the view changes. A key in a log line leaves the
  // station's mode as it is, so the FT8/FT4 role `handleView` enters on the way in is not touched.
  const [operatorAsk, setOperatorAsk] = useState(0)
  useOperatorBoxAsk(() => {
    setView('fieldDay')
    setOperatorAsk((n) => n + 1)
  })

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
      setSettingsTarget((prev) => ({ where, seq: (prev?.seq ?? 0) + 1 }))
      setView('settings')
    },
    [],
  )

  // When Nexus switches radio by itself and the station asked to be told: one line with Undo
  // (the operator's own radio pick, back to the radio it left) and Why? (the routing rules).
  useRadioSwitchNotice({
    snap,
    policy: settings?.radioSwitchPolicy,
    selectRadio: (id) =>
      apiSetActiveRadio(id).then((s) => {
        if (s) setSnap(s)
        return s
      }),
    openSettingsAt,
  })

  const handleTier = useCallback((next: Tier) => {
    void withErrorToast(() => apiSetTier(next), t('shell.tier.failed')).then((s) => {
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
      if (m === 'psk') {
        // Navigate; the [view] rig-mode effect asserts the Keyboard policy
        // (always-USB PKTUSB) and re-homes to the band's PSK31 watering hole on
        // a genuine mode change — the same path as CW/Phone/RTTY. The cockpit
        // still auto-arms its RX decoder on entry.
        setView(m)
        return
      }
      if (m === 'jtty') {
        // Navigate; the [view] rig-mode effect asserts the Keyboard section with JTTY as its modem
        // ('jtty', rigModeForView) and re-homes to the band's JTTY frequency on a genuine change —
        // PSK ⇄ JTTY included. The cockpit auto-arms its RX decoder on entry. Nothing keys.
        setView(m)
        return
      }
      if (m === 'js8') {
        // Navigate; the [view] rig-mode effect asserts DATA and re-homes (rigModeForView), and
        // the cockpit's rising `active` edge calls `js8_enter` — set_tier(JS8) + the JS8
        // watering hole for the band. RX only: nothing here can key.
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
      void withErrorToast(() => apiSetArea('dx'), t('shell.digital.failed'))
        .then((s) => {
          if (s) setSnap(s)
          return withErrorToast(() => apiSetTier(wantTier), t('shell.tier.failed'))
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
    void withErrorToast(() => apiSetSource(k), t('shell.source.switchFailed')).then((s) => {
      if (s) {
        setSnap(s)
        // Confirm the switch even when no decodes follow (e.g. Companion idle).
        pushToast(
          k === 'companion'
            ? t('shell.source.companion', { source: s.radio.sourceLabel })
            : t('shell.source.set', { source: s.radio.sourceLabel }),
          'success',
          3500,
        )
      }
    })
  }, [])

  const handleSettingsSaved = useCallback(() => {
    getSnapshot().then(setSnap).catch(() => {})
    reloadSettings()
    setSettingsSaves((n) => n + 1)
  }, [reloadSettings])

  // The contest setup saves from wherever it is opened (the Contests pane has no line to App of its
  // own), and says so on this window: read the settings and the snapshot again, as after a Save.
  useEffect(() => {
    window.addEventListener(CONTEST_SETUP_SAVED_EVENT, handleSettingsSaved)
    return () => window.removeEventListener(CONTEST_SETUP_SAVED_EVENT, handleSettingsSaved)
  }, [handleSettingsSaved])

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

  // The durable half of "the wizard was done": its one writer, in the settings file. Failure is
  // not toasted: the wizard is closed either way, and the browser copy above still holds for
  // this computer.
  const recordWizardSeen = useCallback(() => {
    void markSetupWizardSeen()
      .then(() => setSettings((prev) => (prev ? { ...prev, setupWizardSeen: true } : prev)))
      .catch(() => {})
  }, [])
  // A wizard that would open by itself waits for the settings file's word: an existing station
  // (no `setupWizardSeen: false`) is never shown it, nor asked the question.
  useEffect(() => {
    if (!settings || !wizardAsks || settings.setupWizardSeen === false) return
    setShowWizard(false)
    setWizardAsks(false)
  }, [settings, wizardAsks])

  const handleWizardApply = useCallback(
    (
      ids: ProfileId[],
      landing: View,
      modes: FeatureId[],
      license: string,
      draft: WizardDraft,
      experience: ExperienceLevel | null,
    ) => {
      // SEQUENCED, not concurrent: set_settings writes the WHOLE struct
      // (licenseClass included), so a concurrent set_license_class would be
      // reverted by the stale class riding in the merge (review catch — the
      // wizard's own TX-lockout promise silently undone). Draft first, class
      // second; failures surface instead of a false-success close.
      void applyWizardDraft(draft)
        .then(() => setLicenseClass(license))
        .then((s) => s && setSnap(s))
        // The experience answer has its own writer; a later form save keeps it. Only an answer
        // given in this run is written: none leaves the station's (absent = never asked).
        .then(() => (experience ? apiSetExperienceLevel(experience) : null))
        .then((s) => {
          if (!s || !experience) return
          setSnap(s)
          setSettings((prev) => (prev ? { ...prev, experienceLevel: experience } : prev))
        })
        .catch((e) =>
          pushToast(
            t('shell.wizard.saveFailed', { error: `${e instanceof Error ? e.message : e}` }),
            'error',
            0,
          ),
        )
      // Goal profiles + the chosen operating modes (CW/Phone) force-enabled on top, on the
      // first run only. Re-run from Settings, the wizard edits station, rig and log and keeps the
      // screens the operator has on now: it never changes the feature set.
      if (wizardAsks) features.applyProfiles(ids, modes)
      setView(landing)
      markWizardSeen()
      recordWizardSeen()
      setShowWizard(false)
      setWizardGen((g) => g + 1)
    },
    [features.applyProfiles, applyWizardDraft, wizardAsks, recordWizardSeen],
  )

  const handleWizardSkip = useCallback(() => {
    markWizardSeen()
    recordWizardSeen()
    setShowWizard(false)
    setWizardGen((g) => g + 1)
  }, [recordWizardSeen])

  // NOTE: everything from here down to the `!snap` early return is snap-INDEPENDENT
  // and — critically for the hooks below — must run on EVERY render (rules of hooks:
  // no hook may live after the early return, or React unmounts the whole app once
  // snap loads and the hook count changes).

  // ⭐ THE CONTEST SCREEN IS ON THE RAIL WITH FIELD DAY MODE OFF, because the mode switch is at
  // the top of it: hidden while the mode is off, the screen could only ever turn the mode off.
  // What the mode reveals still follows the persisted master switch (settings.fdActive) and
  // nothing else: the Class/Section exchange in every cockpit, the Club Board button, and
  // entering the contest when the screen opens (`handleView`).
  const fdActive = settings?.fdActive === true
  const navEnabled: Record<FeatureId, boolean> = { ...features.enabled, fieldDay: true }
  const isViewEnabled = (v: View): boolean => navEnabled[v as FeatureId] !== false

  // Recall card → Logbook, filtered to the call (#192, kr4fqg: "click a previous contact and
  // land in the log"). Same shape as the `onOpenMemories` handoffs below — `undefined` when the
  // section is switched off in this build, which is what keeps the row from advertising a
  // navigation that would go nowhere. Every cockpit that mounts a recall card gets it. Given a
  // contact's id (a recall row, a recent contact, an Awards row) the Logbook also opens its editor on
  // that contact: the one editor, and the one write path.
  const openLogbookFor = isViewEnabled('logbook')
    ? (call: string, id?: string | null) => {
        setLogFocus({ call, ts: Date.now(), editId: id ?? null })
        setView('logbook')
      }
    : undefined

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
  // The section the dashboard rail stands beside, when the view on screen is an operating cockpit.
  const { isOn: railIsOn, setOn: setRailOn } = dashRail
  const railSection: DashRailSection | null = isDashRailSection(effectiveView) ? effectiveView : null
  const railShown = railSection != null && railFits && railIsOn(railSection)
  // ON A WINDOW TOO SMALL FOR IT (the operator's "They move into the columns"): beside FT, Phone, CW and
  // JS8 the rail's boxes stand in the cockpit's columns instead, until the window is wide enough again.
  const railFoldsHere = railSection != null && DASH_RAIL_FOLDS.includes(railSection)
  const railFolds = railFoldsHere && !railFits && railIsOn(railSection!)
  // Its switch for the cockpit's ⊞ Panels menu, published before paint (components/dashRailSwitch) and
  // withdrawn when App goes: null where the rail does not stand.
  const railSwitch = useMemo<DashRailSwitch | null>(
    () =>
      railSection
        ? { on: railIsOn(railSection), fits: railFits, folds: railFoldsHere, set: (on: boolean) => setRailOn(railSection, on) }
        : null,
    [railSection, railIsOn, setRailOn, railFits, railFoldsHere],
  )
  useLayoutEffect(() => publishDashRailSwitch(railSwitch), [railSwitch])
  useLayoutEffect(() => () => publishDashRailSwitch(null), [])
  // THE CALL IN EACH COCKPIT'S LOG ENTRY, the call its Log action would write, for the Rotor box
  // beside it (the bearing to it, and Point). Phone, CW, RTTY, PSK and JS8 report their log strip's
  // (LogEntry `onEntryCall`, settled, so this renders once per call rather than per keystroke); FT's
  // is the QSO's, the one its Log QSO logs (`entryCallOf`, off the snapshot); SSTV and APRS have no
  // log entry. Kept per cockpit, since every cockpit stays mounted behind the one on screen.
  const [entryCalls, setEntryCalls] = useState<Partial<Record<DashRailSection, string>>>({})
  const reportEntry = useMemo(() => {
    const report = (section: DashRailSection) => (call: string) =>
      setEntryCalls((m) => (m[section] === call ? m : { ...m, [section]: call }))
    return { phone: report('phone'), cw: report('cw'), rtty: report('rtty'), psk: report('psk'), js8: report('js8') }
  }, [])

  // ── Eyes-free operating (a11y Phase A) — hooks BEFORE the `!snap` return ──
  // Per-view window title + a polite "now on X" announcement (navigation is
  // otherwise silent to a screen reader).
  useEffect(() => {
    const label = featureById(effectiveView)?.label ?? t('features.settings.label')
    // Conditions' title still says "(formerly Connect)"; its label, which Settings, this
    // announcement and the website's feature list read, is the plain name (operator, 2026-10-02).
    const section = effectiveView === 'connect' ? t('features.connect.windowTitle') : label
    document.title = t('shell.windowTitle', { section })
    announce(label)
  }, [effectiveView])
  // TX state → assertive announce + opt-in earcon. Snap-safe (null pre-connect). The arbiter the
  // ON AIR sign shows, never the FT slot flag alone: that kept a voice over, CW and a tune silent
  // for an operator who hears the TX state rather than sees it (operator, 2026-09-27).
  const prevTxRef = useRef<boolean | null>(null)
  const txNow = snap ? isOnAir(snap.radio) : false
  useEffect(() => {
    if (!snap) return
    if (prevTxRef.current !== null && prevTxRef.current !== txNow) {
      announce(txNow ? t('shell.tx.announce.on') : t('shell.tx.announce.off'), { assertive: true })
      if (settings?.soundTxState) txEarcon(txNow)
    }
    prevTxRef.current = txNow
  }, [snap, txNow, settings?.soundTxState])

  // ESC STOPS TRANSMIT ON TEMPO, PHONE, SSTV, APRS AND SATELLITES (2026-10-01). FT, CW, RTTY,
  // PSK and JS8 each bind their own Esc while shown, to the function their Stop TX calls. These five
  // bound none: Esc did nothing on Tempo, SSTV and APRS, stopped only the voice keyer on Phone, and
  // Satellites had no stop at all. On all five the stop is halt_tx — Tempo's Stop TX is the top
  // bar's, which is this handleHaltTx; Phone's and SSTV's TX-strip Stop TX call haltTx alone; APRS and
  // Satellites draw no stop control (the top bar's is hidden there) — so App sends that halt while
  // one of them is on show, on the shared listener the cockpits' own Esc rides too (useEscStop:
  // capture phase, never preventDefault, stop authority only). So on the same press a menu or dialog
  // still closes, Satellites still closes the bird detail, and the voice keyer still stops itself.
  // JTTY TOO: its TX strip's Stop TX is the plain halt, which also drops JTTY's queue and cuts what
  // is on the air, so App binds its Esc here, as Phone's and SSTV's.
  // CONNECT TOO (the operator's ruling 2026-10-01: "remove all radio control from connect, reclaim that space"): the
  // top bar is not drawn on Connect, so its Stop TX went with it; without this nothing there could cut an over that Operate's
  // background sequence is still keying. 'connect' is in ESC_HALT_VIEWS, so it is this same call, stop authority included.
  // THE CONTEST SCREEN TOO, WHILE ITS ENTRY SHOWS (2026-10-11). With a contest running the screen
  // takes contacts in the logger window's large entry (`ContestLargeEntry`, in the workspace switch
  // below), a contest strip in this window, so its Esc is this window's: the halt, as in the Phone
  // LOG pane, and the entry keeps what it holds (the logger window's own Esc halts and clears:
  // DetachedPanel binds the halt in every contest window). With no contest running there is no entry,
  // and Esc there is as it was.
  const contestEntryShown = effectiveView === 'fieldDay' && snap?.fieldDay != null
  useEscStop(ESC_HALT_VIEWS.has(effectiveView) || contestEntryShown, handleHaltTx)
  // …AND ITS F-KEYS DO NOTHING, as in the logger window: nothing on that entry sends a message, and
  // the webview's own F-key actions are held too, so an F5 reached for out of habit cannot reload the
  // window in the middle of a contact. With Alt, Ctrl or Cmd held a key is the system's (Alt+F4
  // still closes the window), as the voice keyer leaves it.
  useEffect(() => {
    if (!contestEntryShown) return
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key) && !e.altKey && !e.ctrlKey && !e.metaKey) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [contestEntryShown])

  // ⭐ A contest switched by a saved pick, for the Contest screen to say so (`contestSwitch.ts`):
  // kept here, because the pick is saved in Settings, where that screen is not mounted.
  const contestSwitch = useContestSwitch(snap?.fieldDay)
  // ⭐ …and whether the Contest screen below its log line is out in a window of its own: kept here too,
  // so the window's close brings it back whichever screen the operator is on (ContestScreenWindow.tsx).
  const contestScreen = useContestScreenWindow(handleSettingsSaved)

  if (!snap) {
    return (
      <div className="app loading">
        {/* On a computer driving another station, what it waits for: connecting, control held
            elsewhere, or why the station did not take it. Nothing on a station's own screen. */}
        <ClientBanner />
        <span>{t('shell.loading')}</span>
      </div>
    )
  }

  const macros = settings?.macros ?? DEFAULT_MACROS

  const activeConversation =
    snap.conversations.find((c) => c.peer === activePeer) ?? null

  // displayed tier is the authoritative link tier from the snapshot
  const tier = snap.link.tier
  // How wide the FT waterfall draws its RX/TX markers on this tier: WSJT-X's bracket, or null for
  // the single line. The submodes are settings; until they load, Q65 and JT65 keep the line.
  const ftMarkerWidthHz = markerWidthHz(tier, {
    periodS: snap.link.periodSecs,
    q65Submode: settings?.q65Submode,
    jt65Submode: settings?.jt65Submode,
  })

  // First-run nudge: callsign unset / still the placeholder, and not dismissed.
  const needsOnboarding =
    !onboardDismissed &&
    effectiveView !== 'settings' &&
    snap.mycall.trim() === '' // fresh install (the default callsign is empty)

  // The Tempo (chat) roster represents who's on the TEMPO protocol — so it shows only
  // stations last heard on a Tempo tier, not the FT8/FT4 stations that share the engine's
  // single roster. Every other view (Operate, Field Day) shows the full roster.
  //
  // ⚠️ BOTH CHAT TIERS, not just TempoFast. Tempo has two — TempoFast and TempoDeep — and the
  // backend has said so all along (`Tier::is_chat` is `TempoFast | TempoDeep`, and its comment
  // spells out that the whole chat cadence runs on both). This filter knew only the first, so
  // on TempoDeep the roster was STRUCTURALLY always empty: every station heard there was
  // filtered out of the one view that exists to list them. Found while validating an
  // unrelated "Tempo not decoding" report (#160) — nobody had reported the roster itself,
  // which is what an always-empty list gets you: it reads as a quiet band.
  const rosterStations =
    effectiveView === 'chat'
      ? snap.stations.filter((s) => s.tier === 'TempoFast' || s.tier === 'TempoDeep')
      : snap.stations
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
      onCall={handleCall}
      conversations={snap.conversations}
      onArchive={handleArchive}
      bandActive={activePeer === '*'}
      bandUnread={bandUnread}
      onSelectBand={() => handleSelect('*')}
    />
  )
  const operateStationsPanel = (
    <StationList
      // Operate's Classic rail hosts this list as the ⊞ `stations` pane, and this is that
      // entry's tick — the same setPanelState, the same record. The node is built here
      // rather than in the cockpit (it is passed down as `roster`), so the wiring is here
      // too; nothing else renders a StationList with a ⊞ entry, so nothing else passes it.
      onRemove={() => operatePanels.setPanelState('stations', 'removed')}
      paneTitle={t('operate.panel.stations')}
      // Its head is the grip FT's ⊞ Arrange drags it by (panes/PaneDrag).
      grip="stations"
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
      onCall={handleCall}
      conversations={snap.conversations}
      onArchive={handleArchive}
      bandActive={activePeer === '*'}
      bandUnread={bandUnread}
      onSelectBand={() => handleSelect('*')}
      dropAfterCycles={3}
      filter={operateStationsFilter}
      onFilter={setOperateStationsFilter}
      query={operateStationsQuery}
      onQuery={setOperateStationsQuery}
    />
  )

  // The rail dividers (PaneSeam): a drag paints the rail-width CSS var on <html> directly each
  // frame (no React re-render), then commits (clamp + persist) once, on release; the arrows,
  // Home/End and Backspace do the same from the keyboard.
  const paintRail = (name: '--left-rail-w' | '--right-rail-w') => (px: number) =>
    document.documentElement.style.setProperty(name, `${px}px`)

  const waterfallRail = (
    <aside className="right-rail panel">
      <OperateDecodes
        decodes={snap.recentDecodes}
        late={snap.lateDecodes}
        slot={snap.radio.slot}
        rxOffsetHz={snap.radio.rxOffsetHz}
        band={snap.radio.band}
        tier={snap.link.tier}
        periodSecs={snap.link.periodSecs}
        harqRescues={snap.harqRescues}
        onCall={handleCall}
        needAlertsByCall={needAlertsByCall}
        needScopes={needScopes}
        myGrid={snap.mygrid}
        compact
        title={t('shell.bandActivity.title')}
      />
      <LinkPill link={snap.link} radio={snap.radio} />
    </aside>
  )

  // Three-pane workspace: a full-width waterfall strip under the header, then stations | center |
  // the rail (Band Activity and the link pill), with drag splitters between each. THE WATERFALL
  // STRIP (2026-10-26, "width and height to be consistent"): every mode's waterfall starts at the
  // same full width and 25 % of the shell, so Tempo's left the right rail (where it was 299 px wide
  // at 1366×768) for a strip across the top, sized by its own divider as FT's is. The strip is
  // the workspace grid's own row (styles.css `.layout[data-three-pane]`, `--tempo-wf-h`) and stays
  // in the same parent at every width, so it never remounts; on the sm/xs collapse it stacks first.
  const threePane = (center: JSX.Element, header?: JSX.Element) => (
    <main className={`layout${header ? ' has-tempo-header' : ''}`} data-three-pane>
      {header && <div className="grid-header">{header}</div>}
      <Waterfall
        stripRef={tempoWfRef}
        transmitting={snap.radio.transmitting}
        rxOffsetHz={snap.radio.rxOffsetHz}
        txOffsetHz={snap.radio.txOffsetHz}
        theme={theme}
        onTune={handleTune}
        paletteScope={FT_PALETTE_SCOPE}
        markerWidthHz={ftMarkerWidthHz}
        txBlanks // FT surface — a 13 s over may go dark; see the prop's doc.
      />
      <PaneSeam
        axis="y"
        varName="--tempo-wf-h"
        strip={tempoWfRef}
        className="tempo-wf-seam"
        storageKey="nexus.split.tempo.waterfall"
        min={WATERFALL_SPLIT_MIN}
        max={WATERFALL_SPLIT_MAX}
        defaultPct={25}
        label={t('shell.waterfall.splitter.label')}
      />
      <div className="grid-stations">{stationsPanel}</div>
      <PaneSeam
        axis="x"
        className="left"
        label={t('shell.rail.stations.label')}
        value={leftW}
        min={LEFT_MIN}
        max={leftMax}
        grows={1}
        onPaint={paintRail('--left-rail-w')}
        onCommit={commitLeft}
        onReset={resetLeft}
      />
      <div className="grid-center">{center}</div>
      {/* This divider is the right rail's LEFT edge: moving it left widens the rail. */}
      <PaneSeam
        axis="x"
        className="right"
        label={t('shell.rail.waterfall.label')}
        value={rightW}
        min={RIGHT_MIN}
        max={rightMax}
        grows={-1}
        onPaint={paintRail('--right-rail-w')}
        onCommit={commitRight}
        onReset={resetRight}
      />
      <div className="grid-waterfall">{waterfallRail}</div>
    </main>
  )

  // THE SPOTS AND NEEDED BOARDS' WIRING, one object each, shared by the two views below, by the
  // Phone and CW cockpits' Spots and Needed panes (#345) and by the Spots and Needed boxes on Connect
  // and in the rail — so a pane or a box can never be wired differently from its view, and working a
  // row from one is the view's own act.
  const spotsBoard = {
    bandPlan,
    selectedCall: activePeer,
    myGrid: snap.mygrid,
    onSelect: handleSelect,
    onWork: handleWorkSpot,
    // The BOARD's feed (band scopes honoured, mode-feature neutral), because the panel asks
    // it exactly what the Needed board would say: is this station still needed on the band
    // and mode it is spotted on? If so, Hide worked leaves it alone.
    needAlerts: boardAlerts,
    // The Spot me in the board's head (SpotsSpotMe): the station's mode, which its comment names.
    spotMe: { section: snap.radio.operatingMode, keyboardMode: snap.radio.keyboardMode, sideband: snap.radio.sideband, tier },
  }
  const neededBoard = {
    // Un-gated by MODE FEATURE — the board's own per-mode toggles decide what shows,
    // so a disabled CW/Phone feature no longer hides those needs here — but STILL
    // scoped by band: `boardNeeds` is that exact half, and it is a named function so
    // the scopes cannot be dropped again by swapping this prop.
    alerts: boardAlerts,
    bandPlan,
    selectedCall: activePeer,
    myGrid: snap.mygrid,
    onQsy: (a: NeedAlert) => handleQsy(a.band, a.freqMhz ?? undefined),
    onSelect: handleSelect,
    onWork: handleWorkNeeded,
    onPoint:
      ((settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim())
        ? handlePointAntenna
        : undefined,
    onOpenSettings: openSettingsAt,
    phoneSource: feedHealth
      ? {
          status: feedHealth.phoneCluster,
          host: feedHealth.phoneClusterHost,
          spotsSeen: feedHealth.phoneSpotsSeen,
        }
      : null,
    // During a contest the needs-gate hides everyone the operator wants to work, so the
    // board offers one click to the un-gated Spots view. `fieldDay.running` is the one
    // contest-active signal (the same one the contest spot marks and ESM plate read).
    contestActive: snap.fieldDay?.running ?? false,
    onShowAllSpots: () => setView('spots'),
  }
  // WHAT THE BOXES ARE LENT, once: the dashboard rail beside the cockpits, and the boxes inside FT,
  // Phone, CW and JS8 (any pane in any area, 2026-10-07), get this one object, so a box can never be wired
  // differently in one place from the other. The amplifier and the band ride the snapshot App already
  // polls, as on Connect; the Spots, POTA/SOTA and Needed boxes are the boards themselves, lent as they
  // are to Connect. Neither the rail nor a box stands on the hosted Remote page (the desktop first), so
  // this wiring is always the native one.
  const boxSource: BoxSource = {
    myGrid: settings?.mygrid ?? '',
    theme,
    stations: snap.stations ?? [],
    prop,
    needByCall,
    needAlerts: visibleAlerts,
    amp: snap.radio.amp ?? null,
    rigBand: snap.radio.band ?? null,
    fieldDay: snap.fieldDay ?? null,
    activation: { act: snap.activation ?? null, logTick: snap.logTick },
    onWorkSpot: handleWorkMapSpot,
    onPoint: (settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim() ? handlePointAntenna : undefined,
    spotsFeed: { rows: allSpots, board: spotsBoard },
    otaBoard: { snap, onHunt: handleHuntSpot, onSnap: setSnap },
    neededBoard,
  }
  const cockpitBoxes = boxSource
  // ONCE PER SCREEN, ACROSS THE COCKPIT AND THE RAIL BESIDE IT (the operator's "Once per screen"): one reading
  // of what the two show, made here, where both records live. The cockpit's own Spots and Needed panes come
  // first, then the rail's slots, then the cockpit's boxes: a slot whose board the cockpit shows as a pane of
  // its own shows another (features/dashRail `railOnScreen`), and a box whose entry the rail shows shows
  // another (features/panelState `boxEntries`). Neither record is rewritten by that, so each gets its own back
  // when the other lets it go. A pick on either side moves the entry there and hands the other side what it
  // showed, as two boxes swap. Only the cockpit on screen has the rail beside it.
  const boxCockpits: Partial<Record<DashRailSection, { spec: PanelVocabulary<string>; panels: PanelLayoutApi<string> }>> = {
    operate: { spec: OPERATE_PANELS as PanelVocabulary<string>, panels: operatePanels as unknown as PanelLayoutApi<string> },
    phone: { spec: PHONE_PANELS as PanelVocabulary<string>, panels: phonePanels as unknown as PanelLayoutApi<string> },
    cw: { spec: CW_PANELS as PanelVocabulary<string>, panels: cwPanels as unknown as PanelLayoutApi<string> },
    js8: { spec: JS8_PANELS as PanelVocabulary<string>, panels: js8Panels as unknown as PanelLayoutApi<string> },
  }
  const railRecords = railSection ? dashRailRecords(dashRailRec, railSection) : null
  const railHost = railSection && cockpitBoxes ? boxCockpits[railSection] : undefined
  const hostOwn = railHost ? ownShown(railHost.spec, railHost.panels.layout, railHost.panels.stateOf) : []
  const railBoxes =
    railRecords && (railShown || (railFolds && railHost))
      ? railOnScreen(railRecords.slots, DASH_SLOT_IDS.filter((s) => railRecords.panels.stateOf(s) !== 'removed'), hostOwn)
      : []
  const railShows = railBoxes.map((b) => b.entry)
  const hostEntries = railHost ? boxEntries(railHost.spec, railHost.panels.layout, railHost.panels.stateOf, railShows) : {}
  // The rail's pick: the box in its slot, and what the cockpit showed of it moves: a box of the cockpit that
  // showed it takes what the slot showed, and the cockpit's own pane that is that board is hidden (a hide that
  // ends nothing).
  const railPick = (slot: DashSlotId, pane: PaneId) => {
    if (!railRecords) return
    const was = railBoxes.find((b) => b.slot === slot)?.entry ?? sharedPaneOf(railRecords.slots[slot])?.id
    railRecords.assignPane(slot, pane)
    const entry = sharedPaneOf(pane)?.id
    if (!railHost || entry == null) return
    const box = Object.keys(hostEntries).find((b) => hostEntries[b] === entry)
    if (box != null && was != null) railHost.panels.setBox?.(box, was)
    for (const [own, e] of Object.entries(railHost.spec.sharedAs ?? {})) {
      if (e === entry && railHost.panels.stateOf(own) !== 'removed') railHost.panels.setPanelState(own, 'removed')
    }
  }
  const railLink: RailLink | undefined =
    railRecords && railBoxes.length > 0
      ? {
          shows: railShows,
          // A box took one of the rail's: the slot that showed it takes what the box showed.
          take: (entry, give) => {
            const slot = railBoxes.find((b) => b.entry === entry)?.slot
            if (slot == null) return
            const pane = give != null ? sharedPaneById(give)?.pane : undefined
            if (pane) railRecords.assignPane(slot, pane)
            else railRecords.panels.setPanelState(slot, 'removed')
          },
          // Below lg, the rail's boxes themselves, for the cockpit's columns.
          folded: railFolds
            ? {
                boxes: railBoxes,
                pick: (slot, entry) => {
                  const pane = sharedPaneById(entry)?.pane
                  if (pane) railPick(slot, pane)
                },
              }
            : undefined,
        }
      : undefined
  /** The call in a cockpit's log entry (`entryCalls`), or null where there is none. */
  const entryCallOf = (section: DashRailSection): string | null =>
    (section === 'operate' ? snap.qso?.dxcall : entryCalls[section]) || null
  /** What a cockpit's boxes are lent: the window's source with the cockpit's own log entry call, and the
   *  rail beside it while it is the one on screen. */
  const boxesFor = (section: DashRailSection): BoxSource | undefined => {
    const boxes = cockpitBoxes && { ...cockpitBoxes, entryCall: entryCallOf(section) }
    return boxes && railLink && section === railSection ? { ...boxes, rail: railLink } : boxes
  }
  /** ENTER SENDS MESSAGE's switch for one cockpit, as Settings holds it, and the TX dock switch's
   *  save: that one field (`set_contest_esm`), never a settings-form save, which would clear the
   *  transmit queues mid-contest. Shown at once and put back if the save fails. */
  const esmSettingFor = (cockpit: 'cw' | 'rtty' | 'phone'): EsmSetting | undefined => {
    const field = cockpit === 'cw' ? 'contestEsmCw' : cockpit === 'rtty' ? 'contestEsmRtty' : 'contestEsmPhone'
    return {
      on: settings?.[field] === true,
      callOnce: settings?.contestEsmCallOnce === true,
      voiceRoles: cockpit === 'phone' ? settings?.voiceEsmRoles : undefined,
      onSwitch: (on) => {
        setSettings((prev) => (prev ? { ...prev, [field]: on } : prev))
        setContestEsm(cockpit, on).catch(() => {
          setSettings((prev) => (prev ? { ...prev, [field]: !on } : prev))
          pushToast(t('contest.esm.switch.failed'), 'error')
        })
      },
    }
  }
  // CW speed match's switch in the CW contest strip: saved alone, as ESM's is.
  const cwSpeedMatch = {
    on: settings?.cwSpeedMatch === true,
    onSwitch: (on: boolean) => {
      setSettings((prev) => (prev ? { ...prev, cwSpeedMatch: on } : prev))
      setCwSpeedMatch(on).catch(() => {
        setSettings((prev) => (prev ? { ...prev, cwSpeedMatch: !on } : prev))
        pushToast(t('cw.speedMatch.failed'), 'error')
      })
    },
  }
  // CONTEST MODE ON, THE CONTEST NOT RUNNING: the CW and Phone log lines say so, with Resume, which enters
  // the contest's S&P (never a CQ). Opening either screen re-enters on its own (`handleView`); this is for
  // a session that did not come back, or dropped while the screen was open. Not while that re-entry is
  // still on its way: the line would flash for the one snapshot before it lands.
  const resumeContest =
    settings?.fdActive === true && snap.mode !== 'fieldDay' && !contestReentering
      ? () => void handleSetMode('fieldday-sp')
      : undefined
  const cwWorkspace = (
    <CwCockpit
      esmSetting={esmSettingFor('cw')}
      speedMatch={cwSpeedMatch}
      turnSound={settings?.soundCwTurn === true}
      announceVerbosity={settings?.announceVerbosity ?? 'needed'}
      onResumeContest={resumeContest}
      onOpenLogbook={openLogbookFor}
      onEntryCall={reportEntry.cw}
      pitchHz={settings?.cwPitchHz ?? 600}
      onPitch={(cwPitchHz) => setSettings((prev) => (prev ? { ...prev, cwPitchHz } : prev))}
      wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
      snap={snap}
      theme={theme}
      pendingWork={pendingWork?.view === 'cw' ? pendingWork : null}
      onConsumeWork={() => setPendingWork(null)}
      onSnap={setSnap}
      fieldDay={snap.fieldDay}
      spots={allSpots}
      tciSpots={tciSpots}
      onTciSpot={handleTciSpot}
      needByCall={needByCall}
      typeByCall={typeByCall}
      onWorkSpot={workSpotHereCw}
      onRecallMemory={isViewEnabled('memories') ? recallMemory : undefined}
      onOpenMemories={isViewEnabled('memories') ? () => setView('memories') : undefined}
      onOpenSettings={openSettingsAt}
      panels={cwPanels}
      spotsBoard={spotsBoard}
      neededBoard={neededBoard}
      boxes={boxesFor('cw')}
    />
  )
  const phoneWorkspace = (
    <PhoneCockpit
      esmSetting={esmSettingFor('phone')}
      flexDaxNote={settings ? flexVoiceNeedsDax(settings) : false}
      onResumeContest={resumeContest}
      onOpenLogbook={openLogbookFor}
      onEntryCall={reportEntry.phone}
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
      tciSpots={tciSpots}
      onTciSpot={handleTciSpot}
      needByCall={needByCall}
      typeByCall={typeByCall}
      onWorkSpot={workSpotHerePhone}
      onRecallMemory={isViewEnabled('memories') ? recallMemory : undefined}
      onOpenMemories={isViewEnabled('memories') ? () => setView('memories') : undefined}
      onOpenSettings={openSettingsAt}
      spotsBoard={spotsBoard}
      neededBoard={neededBoard}
      boxes={boxesFor('phone')}
    />
  )

  let workspace: JSX.Element | null
  switch (effectiveView) {
    case 'fieldDay':
      // THE FIELD DAY SECTION IS SETUP, SCORE, SECTIONS, BONUSES, LOG AND THE CLUB BOARD — and,
      // while a contest runs, a place to TYPE the contacts (the operator's pick, 2026-10-11,
      // rehearsing the Illinois QSO Party: "The Logger window's large Call / RST / County boxes,
      // dupe warning and last contacts at the top of the Contest screen. Same log line, so a
      // contact logs once; it never transmits."). That entry is the logger window's own
      // (`ContestLargeEntry`), mounted here as the main window's contest strip: the same
      // `LogEntry`, the same shared contact as the cockpits' LOG panes, and no transmitter behind
      // it — no PTT, no messages, no Enter Sends Message, no Tune — while Esc stays this window's
      // stop (`useEscStop`, above). It is still NOT a cockpit, and there is deliberately no second
      // one: Field Day is a MODE the whole app enters (`Mode::FieldDay`), the radio is worked in
      // the primary sections — CW, Phone, RTTY, PSK and the digital Conversation view all take
      // `fieldDay` and turn their log strips into FD entries with class/section and shared dupe
      // checking. A parallel Field-Day-only cockpit shipped beside them for one build and was
      // withdrawn: it was a weaker copy of surfaces that already exist (its digital pane was a
      // read-only monitor), and it took the operator away from the mode's own features to get the
      // exchange boxes those sections already draw.
      //
      // It does NOT go through `threePane`, and that helper is the reason: it is not a layout,
      // it is the digital OPERATING workspace, and it hardcodes its furniture. Until
      // 2026-08-30 this view was drawn inside it — stations/chat rail on the left, waterfall +
      // band activity + the mesh link pill on the right. It reads and writes none of that.
      // Operator, mid-event: "what screen is this supposed to represent? Why are theyere
      // waterfalls in here when its not the primary working area?" The rails also cost it the
      // width its header strip needs (class/section, RUN vs S&P and four exports), which is
      // the second half of the same report.
      //
      // Nothing is lost by leaving: the waterfall, band activity and the link pill all belong
      // to the FT surface and are on screen in Operate, which is where digital Field Day
      // contacts are actually made; the stations rail is the Tempo mesh roster and
      // conversation picker, which this view never consulted.
      //
      // ONE WRAPPER, `.contest-screen`: a column holding the entry over the screen, there whether
      // or not the entry is, so a contest starting or ending never re-parents (and so never
      // remounts) the screen; with no entry it lays the screen out exactly as before. The
      // screen's root IS a `.panel`, so `.layout.single > .panel` still gives it the definite
      // height, the deficit valve and the measure it needs (styles.css; computed in
      // layout-single-deficit.test.tsx and fdDashboardShell.test.tsx).
      //
      // THE OPERATOR BOX goes with the entry, onto its title line beside Sending and I moved (the
      // operator, 2026-10-12: "I want to move the field to change operators up near the top"), and
      // the screen below draws none while it is there. It saves through `handleSetOperator`, the
      // same narrow write the top bar's operator chip uses, so the chip follows what is typed.
      //
      // THE SCREEN BELOW THE ENTRY POPS OUT to a window of its own (the operator, 2026-10-12: "that
      // whole window needs to pop out"; the pick: "While it's open, the main Contest screen shows just
      // the big entry and last contacts; closing the window brings the section back"). Its Pop out is
      // on its banner, here and nowhere else (not the logger window's copy, nor the Remote page's);
      // while the window is open, one line stands where the screen was, with Bring it back, and the
      // entry, its operator box and Ctrl+O stay this window's. ContestScreenWindow.tsx has the rest.
      workspace = (
        <div className="contest-screen">
          {snap.fieldDay && (
            <ContestLargeEntry
              snap={snap}
              fieldDay={snap.fieldDay}
              fdActive={settings?.fdActive ?? false}
              fdRuleset={fdRuleset}
              title={t('contestScreen.entry.title')}
              operator={settings ? (settings.fdOperator ?? '') : null}
              onSaveOperator={handleSetOperator}
              operatorAsk={operatorAsk}
              onOperatorFocused={() => setOperatorAsk(0)}
              barEnd={
                // ⭐ THE ENTRY ALONE IN A WINDOW (the operator: the screen is cluttered): the contest
                // logger window in its entry-only layout, sharing this entry as that window always does.
                <button
                  type="button"
                  className="contest-logger-toggle"
                  title={t('contestScreen.entry.popOut.title')}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => void openContestLogger('entry')}
                >
                  {t('contestScreen.entry.popOut.label')}
                </button>
              }
            />
          )}
          <main className="layout single">
            {contestScreen.out ? (
              <ContestScreenAway onBringBack={contestScreen.bringBack} />
            ) : (
              <ContestView
                fieldDay={snap.fieldDay}
                clubLink={snap.clubLink}
                onSetMode={handleSetMode}
                fdActive={settings?.fdActive ?? false}
                fdRuleset={fdRuleset}
                tier={tier}
                section={snap.radio.operatingMode}
                keyboardMode={snap.radio.keyboardMode}
                onOpenSettings={openSettingsAt}
                onSettingsSaved={handleSettingsSaved}
                contestSwitch={contestSwitch}
                operatorAsk={operatorAsk}
                onOperatorFocused={() => setOperatorAsk(0)}
                operatorBoxAbove={snap.fieldDay != null}
                entryAbove={snap.fieldDay != null}
                onPopOut={contestScreen.popOut}
                onWorkIn={handleView}
              />
            )}
          </main>
        </div>
      )
      break
    case 'logbook':
      workspace = (
        <main className="layout single">
          <Logbook
            focusCall={logFocus}
            onConsumeFocusCall={() => setLogFocus(null)}
            logTick={snap.logTick}
            activation={snap.activation ?? null}
            // The column choice lives in settings.json, saved through its one writer: a settings
            // save from an older copy keeps the live one (`set_logbook_columns`).
            columns={settings?.logbookColumns}
            onColumnsChange={(logbookColumns) => {
              setSettings((prev) => (prev ? { ...prev, logbookColumns } : prev))
              void withErrorToast(() => setLogbookColumns(logbookColumns), t('logbook.columns.saveFailed'))
            }}
            defaultBand={snap.radio.band}
            defaultFreqMhz={snap.radio.dialMhz}
            // Seed manual entries from the mode the operator was ACTUALLY running —
            // a hand-logged SSB/CW QSO must not default to the digital codec tier
            // (that silently wrote "FT8" on phone contacts and corrupted awards).
            //
            // 'keyboard' is the PSK cockpit's rig mode (rigModeForView.ts) and it was
            // missing here, so it fell through the same hole phone once did: a PSK31
            // contact hand-logged after the cockpit visit came up pre-filled FT8 (#159,
            // the third of that report's three defects). PSK31 is the sub-mode the cockpit
            // opens on; a QPSK31 contact is one pick away in the Mode field, which is the
            // same accuracy the phone branch offers between SSB and FM.
            defaultMode={
              lastOpModeRef.current === 'phone'
                ? settings?.phoneMode?.toLowerCase() === 'fm'
                  ? 'FM'
                  : 'SSB'
                : lastOpModeRef.current === 'cw'
                  ? 'CW'
                  : lastOpModeRef.current === 'rtty'
                    ? 'RTTY'
                    : lastOpModeRef.current === 'keyboard'
                      ? 'PSK31'
                      : lastOpModeRef.current === 'jtty'
                        ? 'JTTY'
                        : snap.link.tier
            }
          />
        </main>
      )
      break
    case 'needed':
      workspace = (
        <NeededPanel {...neededBoard} onPopOut={() => void openPanelWindow('needed')} />
      )
      break
    case 'spots':
      workspace = (
        <SpotsPanel spots={allSpots} {...spotsBoard} onPopOut={() => void openPanelWindow('spots')} />
      )
      break
    case 'awards':
      // Awards + Journey combined: one section, tabbed (Journey + Official Awards).
      workspace = (
        <AwardsJourney
          showGamification={features.isOn('gamification')}
          onOpenSettings={openSettingsAt}
          onOpenLogbook={openLogbookFor}
        />
      )
      break
    case 'stats':
      // Descriptive logbook analytics — the log sliced by band/mode/year/hour/entity.
      workspace = <StatsView />
      break
    case 'cw':
      workspace = cwWorkspace
      break
    case 'phone':
      workspace = phoneWorkspace
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
            fieldDay={snap.fieldDay}
            clubLink={snap.clubLink}
            key={`sp-wiz${wizardGen}`}
            onSaved={handleSettingsSaved}
            target={settingsTarget?.where}
            targetSeq={settingsTarget?.seq}
            radio={snap.radio}
            parsecPresence={snap.parsecPresence}
            activeRadioId={snap.activeRadioId}
            onProveTx={handleProveTx}
            scale={scale}
            scaleMode={scaleMode}
            scaleCap={scaleCap}
            onScaleModeChange={setScaleMode}
            onScaleCapChange={setScaleCap}
            density={density}
            onDensityChange={setDensity}
            textSize={textSize}
            onTextSizeChange={setTextSize}
            localClock={localClock}
            onLocalClockChange={setLocalClock}
            onResetLayout={resetWidths}
            features={features}
            onRerunWizard={() => {
              setWizardAsks(false)
              setShowWizard(true)
            }}
            theme={themeChoice}
            onThemeChange={setTheme}
            skin={skin}
            onSkinChange={setSkin}
            paintedTheme={theme}
            fieldMode={fieldMode}
            onFieldModeChange={setFieldMode}
            highContrast={highContrast}
            onHighContrastChange={setHighContrast}
            night={night}
            onNightChange={setNight}
            nightGridKnown={nightGridKnown}
            palette={palette}
            onPaletteChange={setPalettePreset}
            motion={motion}
            onMotionChange={setMotion}
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
    case 'psk':
    case 'jtty':
    case 'sstv':
    case 'aprs':
    case 'js8':
      // Same keep-alive pattern as Operate: RTTY's, PSK's and JTTY's decoded streams,
      // SSTV's always-armed VIS receiver, APRS's decode list and JS8's four-speed
      // activity stream must survive navigation, so all five live in persistent hosts
      // below. Nothing in the slot.
      //
      // ⚠️ A KEEP-ALIVE COCKPIT WITHOUT A CASE HERE FALLS THROUGH TO `default:` AND DRAWS
      // THE TEMPO WORKSPACE UNDERNEATH ITSELF. 'js8' was missing and did exactly that —
      // TempoHeader's TempoFast/TempoDeep chips, the Tempo roster + conversation, and the
      // right rail's FT waterfall beside the cockpit's own, all on the JS8 screen (operator,
      // 2026-09: "why is there tempo fast and tempo slow listed?", "also bringing in tempo
      // chats", and a "split waterfall"). It is the 0.4–0.21 two-mains-in-the-shell class
      // reached from the SWITCH rather than from the CSS that host-hidden.test.ts guards.
      // Pinned by App.js8workspace.test.tsx, which mounts App and counts what is on screen.
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
          // The amplifier rides the snapshot App already polls at 300 ms — no fourth poller,
          // no new command. Absent when none is configured, and the pane then renders nothing.
          amp={snap?.radio.amp ?? null}
          rigBand={snap?.radio.band ?? null}
          fieldDay={snap?.fieldDay ?? null}
          activation={snap ? { act: snap.activation ?? null, logTick: snap.logTick } : undefined}
          // The Spots, POTA/SOTA and Needed boxes are the boards themselves: the Spots view's own
          // `spotsBoard` and feed, the POTA/SOTA view's own hunt wiring and the Needed view's own
          // `neededBoard`, handed over whole so a Work or a HUNT from a box is the view's act. A
          // browser's box gets no hunt wiring (its one line); and the Needed box is the desktop's
          // for now (2026-10-07).
          spotsFeed={{ rows: allSpots, board: spotsBoard }}
          otaBoard={{ snap, onHunt: handleHuntSpot, onSnap: setSnap }}
          neededBoard={neededBoard}
          // Rotor is configured EITHER by picking a model (Nexus launches the
          // bundled rotctld) OR by the advanced external host — host-only was
          // the pre-rotctld gate and silently disabled point-at for model users.
          onPoint={
            ((settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim())
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
          // The top bar is not drawn on Connect, so its clock rides in Connect's header, with the local
          // clock beside it when Settings ▸ Workspace asks for one, as the top bar's did.
          showLocalClock={localClock}
          // Frame + bar's bar shows the station as the dashboard window's bar does: the snapshot's.
          station={{ call: snap?.mycall ?? '', grid: snap?.mygrid ?? '' }}
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
            onOpenLogbook={openLogbookFor}
            focusSat={satFocus}
            snap={snap}
            onPopOut={() => void openPanelWindow('sats')}
            onSnap={setSnap}
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
                  : lastOpModeRef.current === 'rtty'
                    ? // Found while fixing its identical sibling below and fixed with it: this
                      // ladder never had an 'rtty' branch either, so a memory saved from the
                      // RTTY cockpit was stamped FT8 too. Same consumption path, re-checked
                      // rather than assumed by analogy: RTTY is already in `DIGITAL_MODES`, so
                      // `planRecall` routes it digital, and `plan.mode` commands nothing.
                      'RTTY'
                    : lastOpModeRef.current === 'keyboard'
                    ? // The same hole as the Logbook seed above, in the surface that
                      // WRITES A RECORD THE OPERATOR KEEPS: 'keyboard' is the PSK
                      // cockpit's rig mode and it was not on this ladder, so a memory
                      // saved from the PSK watering hole was stamped FT8 — a wrong mode
                      // on a stored frequency, re-read every time it is recalled.
                      //
                      // Safe as a MEMORY mode, checked rather than assumed: `planRecall`
                      // already knows PSK31 (it is in DIGITAL_MODES), and `plan.mode`
                      // never reaches CAT — the rig mode on recall comes from the
                      // 'digital' operating policy, and `plan.mode` is used only for the
                      // toast and the phone USB/LSB override branch. So this labels the
                      // memory honestly and commands nothing new.
                      'PSK31'
                    : lastOpModeRef.current === 'jtty'
                    ? // The JTTY cockpit's rig mode, labelled as the mode it is. A memory planRecall
                      // routes digital (DIGITAL_MODES), as it does PSK31's: it commands nothing new.
                      'JTTY'
                    : // The tier's own name, not a guess. This was
                      // `tier === 'FT4' ? 'FT4' : 'FT8'`, which labelled a memory
                      // saved on Q65/WSPR/JT65 as FT8.
                      OPERATE_TIERS.includes(tier)
                      ? tier
                      : // ⚠️ NOT 'FT8' — ASK THE RADIO. This ladder reads the last OPERATING
                        // section, and Memories is not one: entering it leaves the ref alone,
                        // and on a fresh launch it starts at 'digital'. So a memory saved from
                        // the Memories view itself — the surface whose whole job is saving
                        // memories — was stamped FT8 no matter what the rig was doing, which is
                        // the "new memories default to digital modes" report. The rig's own
                        // reported mode is the honest answer here and is already on the
                        // snapshot; the tier ladder above still wins wherever an operating
                        // section really is active, because there the operator IS in that mode.
                        snap.radio.rigMode?.trim() || snap.radio.sideband || 'USB'
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
          <RadioProgView myGrid={settings?.mygrid ?? ''} catOk={snap.radio.catOk === true} onOpenSettings={openSettingsAt} />
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
                  t('shell.roam.toggle.failed'),
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
              onSetTxEnabled={handleSetTxEnabled}
              onSetTune={handleSetTune}
              onAtuTune={handleAtuTune}
              onAtuBypass={handleAtuBypass}
              onHaltTx={handleHaltTx}
              onSetHoldTxFreq={handleSetHoldTxFreq}
            />,
          )}
          {roamOpen && (
            <div className="roam-modal" role="dialog" aria-label={t('shell.roam.aria')}>
              <div className="roam-modal-body">
                <button
                  type="button"
                  className="roam-modal-close"
                  onClick={() => setRoamOpen(false)}
                  aria-label={t('shell.roam.close.aria')}
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
      <ClientBanner />
      {/* The window's one station card: a call clicked anywhere on screen opens it (CallCard.tsx). */}
      <CallCardHost snap={snap} spots={allSpots} needAlertsByCall={needAlertsByCall} />
      {/* ⚠️ NOT ON CONNECT, by the operator's ruling (2026-10-01): "at very top, its the frequency, the
          band dropdown, tx off, tune, stop tx.  That bar doenst need to live in connect/conditions",
          then "remove all radio control from connect, reclaim that space". So the whole bar goes on
          Connect, and Connect's content takes its height; every other screen keeps it exactly as
          before. Its Stop TX goes with it — the stop line's one ruled exception, which the sweeps
          name: transmit on Connect is stopped by Esc (the listener above) or by leaving the screen. */}
      {effectiveView !== 'connect' && <TopBar
        mycall={snap.mycall}
        mygrid={snap.mygrid}
        radio={snap.radio}
        radios={snap.radios}
        radioPegged={snap.radioPegged}
        onSetActiveRadio={handleSetActiveRadio}
        stationProfiles={snap.stationProfiles}
        activeStationProfile={snap.activeStationProfile}
        onSwitchStationProfile={handleSwitchStationProfile}
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
        // Tempo's cluster moved into its TX strip under the Tempo header (2026-10-01), so
        // the top bar hides it there exactly as on Operate.
        hideTxControls={effectiveView === 'operate' || effectiveView === 'chat'}
        hideFrequencyControl={
          effectiveView === 'phone' ||
          effectiveView === 'cw' ||
          effectiveView === 'operate' ||
          effectiveView === 'chat' ||
          effectiveView === 'rtty' ||
          effectiveView === 'psk' ||
          effectiveView === 'jtty' ||
          effectiveView === 'sstv' ||
          effectiveView === 'aprs' ||
          effectiveView === 'js8' ||
          // Satellites: the bird's own surfaces ARE the frequency authority here
          // (transponder cards, passband strip, the binding line). Operator
          // report: "my dropdowns for frequencies are still showing FT8
          // frequencies under satellite" — the top control offered 14.074 next
          // to a bird on 435 MHz.
          effectiveView === 'sats'
        }
        hideDigitalChrome={
          // ⚠️ 'fieldDay' IS DELIBERATELY NOT ON THIS LIST. It is not a cockpit at all — it is
          // the setup / score / log screen, and it draws no header of its own with a Stop TX on
          // it. Field Day contacts are made in the cockpits below, which are on the list because
          // each one's own header carries the stop line. Hiding the top bar's TX cluster here
          // would take the only stop on the screen away from an operator who is parked on the
          // scoreboard while the rig is keyed from somewhere else.
          effectiveView === 'phone' ||
          effectiveView === 'cw' ||
          // RTTY/PSK/SSTV/APRS/JS8 are free-running modes with their OWN band selectors — the
          // top control is fed the DIGITAL (FT8) plan, and the tier tiles / slot
          // clock / DT readout are slot-sync furniture that means nothing here (JS8 is
          // slot-synced, but its header carries its own band select, speed chips and Stop
          // TX, so the TopBar's cluster is hidden here exactly as for the others).
          effectiveView === 'rtty' ||
          effectiveView === 'psk' ||
          effectiveView === 'jtty' ||
          effectiveView === 'sstv' ||
          effectiveView === 'aprs' ||
          effectiveView === 'js8' ||
          effectiveView === 'sats'
        }
        // The FT screens. At the small size every other screen drops the FT-only items from the
        // bar, which takes it from four lines to two, and these two fold their mode pills into one
        // button (operator, 2026-10-01). Not a widening of the list above: that flag also hides the
        // TX cluster, the only Stop TX on Field Day and the other screens without a cockpit.
        ftScreen={effectiveView === 'operate' || effectiveView === 'chat'}
        tier={tier}
        onTierChange={handleTier}
        onOpenGuide={() => setShowGuide(true)}
        onOpenWalk={() => setShowWalk(true)}
        field={fieldMode}
        onFieldChange={setFieldMode}
        // Who is at the key (#25). Absent/empty renders nothing — the single-op case —
        // UNLESS Field Day is on, where the seat swap has to be reachable from every mode
        // before anyone has claimed the first seat (operator, 2026-08-30).
        operator={settings?.fdOperator ?? ''}
        operatorRoster={opRoster}
        onSetOperator={handleSetOperator}
        fdActive={settings?.fdActive ?? false}
        showLocalClock={localClock}
      />}

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
          Still notify-never-act: Nexus does not move the
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
        needsAvailable
        onNavigate={handleView}
        // Profile-declared chip emphasis (dangling since the profiles landed).
        // A hand-blended feature set is tagged 'custom' (no profile) → default order.
        emphasis={features.profile === 'custom' ? undefined : PROFILES[features.profile].nowBarEmphasis}
        // The dashboard rail's switch: beside an operating cockpit, and only where the window can
        // show the rail, so the bar never offers a press that changes nothing.
        rail={
          // Where the switch changes what is on screen: the rail beside the cockpit, or its boxes in the columns.
          railSection && (railFits || railFoldsHere)
            ? { on: railShown || railFolds, onToggle: () => setRailOn(railSection, !railIsOn(railSection)) }
            : undefined
        }
      />

      {/* `data-dash-rail` while the dashboard rail takes width beside the cockpit: Operate's QSO strip
          keeps its two-row arrangement then (styles.css `.cq-break`). */}
      <div className="shell" data-dash-rail={railShown ? 'on' : undefined}>
        <ModeNav
          view={effectiveView}
          mode={snap.mode}
          enabled={navEnabled}
          onSelect={handleView}
          tier={tier}
          onDigitalMode={handleDigitalMode}
          // The club band board is a WINDOW, not a section: the rail button opens the
          // `fdclub` pop-out straight onto a second monitor. It rides the Field Day
          // master switch (`clubBoard`) and NOT club sync — the board used to be
          // reachable only from inside ContestView once sync was already on, which is
          // exactly why nobody found it.
          onClubBoard={() => void openPanelWindow('fdclub')}
          clubBoard={fdActive}
          // The Contest item's tooltip names the picked contest; a blank pick is ARRL Field
          // Day, as it is to the engine.
          contest={settings ? contestName(settings.fdEvent?.trim() || 'arrlfd') : undefined}
          // The mode badge names the contest the engine runs: the session's, never the pick, which
          // the engine runs as ARRL Field Day when the loaded rules do not carry it.
          runningContest={snap.fieldDay?.event}
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
          label={featureById(effectiveView)?.label ?? t('shell.crash.section')}
          resetKey={effectiveView}
          action={{
            label:
              crashEscape === effectiveView
                ? t('shell.crash.retry')
                : t('shell.crash.back', {
                    section: featureById(crashEscape)?.label ?? t('features.operate.label'),
                  }),
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
              onOpenLogbook={openLogbookFor}
              companionAddr={settings?.companionAddr}
              fdActive={settings?.fdActive ?? false}
              fdRuleset={fdRuleset}
              superFoxCalls={superFoxCalls}
              blockedCalls={settings?.blockedCalls ?? []}
              onToggleBlocked={handleToggleBlocked}
              snap={snap}
              theme={theme}
              tier={tier}
              markerWidthHz={ftMarkerWidthHz}
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
              needScopes={needScopes}
              selectedCall={activePeer}
              onSelect={handleSelect}
              // #204: F4 / Clear has to reach the callsign card, and half of what the card
              // reads is BACKEND state. `handleMapSelect` already round-trips a null through
              // `select_peer` — reuse it rather than adding a second deselect path.
              onClearSelection={() => handleMapSelect(null)}
              layoutMode={operateLayout}
              onLayoutMode={handleOperateLayout}
              panels={operatePanels}
              onPopOut={() => void openPanelWindow('operate')}
              active={effectiveView === 'operate'}
              boxes={boxesFor('operate')}
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
                esmSetting={esmSettingFor('rtty')}
                onOpenLogbook={openLogbookFor}
                onEntryCall={reportEntry.rtty}
                snap={snap}
                onSnap={setSnap}
                active={effectiveView === 'rtty'}
                onSetFrequency={handleSetFrequency}
                onSetTxEnabled={handleSetTxEnabled}
                theme={theme}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                panels={rttyPanels}
                macros={settings?.macros}
                onMacrosSaved={(macros) => setSettings((prev) => (prev ? { ...prev, macros } : prev))}
                onOpenSettings={openSettingsAt}
              />
            </div>
          )}
          {isViewEnabled('psk') && (
            <div className="psk-host" hidden={effectiveView !== 'psk'}>
              <PskCockpit
                onOpenLogbook={openLogbookFor}
                onEntryCall={reportEntry.psk}
                snap={snap}
                onSnap={setSnap}
                active={effectiveView === 'psk'}
                onSetFrequency={handleSetFrequency}
                onSetTxEnabled={handleSetTxEnabled}
                theme={theme}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                panels={pskPanels}
                macros={settings?.macros}
                onMacrosSaved={(macros) => setSettings((prev) => (prev ? { ...prev, macros } : prev))}
                onOpenSettings={openSettingsAt}
              />
            </div>
          )}
          {/* JTTY keep-alive host — the .psk-host contract: the engine keeps decoding while the
              operator is elsewhere, `active` gates the display poll and fires the auto-arm on the
              rising edge. */}
          {isViewEnabled('jtty') && (
            <div className="jtty-host" hidden={effectiveView !== 'jtty'}>
              <JttyCockpit
                onOpenLogbook={openLogbookFor}
                snap={snap}
                onSnap={setSnap}
                active={effectiveView === 'jtty'}
                onSetFrequency={handleSetFrequency}
                onSetTxEnabled={handleSetTxEnabled}
                theme={theme}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                panels={jttyPanels}
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
                txFskId={settings?.sstvTxFskId}
                txPowerPct={settings?.sstvTxPowerPct}
                panels={sstvPanels}
                onOpenSettings={openSettingsAt}
                // #191: the flat mirror, so this is the ACTIVE radio's answer. The switch is
                // per radio (it is a property of how one rig is cabled), and the view is about
                // the radio being operated.
                holdDataSubmode={settings?.sstvHoldDataSubmode ?? false}
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
                // FT's TX cluster (2026-10-01): APRS's TX On/Off moved into it, and it gained
                // Stop TX (halt_tx, the stop its Esc sends). Tune and ATU only when the rig reports a
                // tuner — there is nothing to tune a 2 m FM antenna against otherwise.
                txStrip={
                  <CockpitTxStrip
                    radio={snap.radio}
                    onSetTxEnabled={handleSetTxEnabled}
                    onTune={snap.radio.atu != null ? handleSetTune : undefined}
                    onAtuTune={handleAtuTune}
                    onAtuBypass={handleAtuBypass}
                    onStopTx={handleHaltTx}
                  />
                }
                txAlarm={<TxAlarmBanner alarms={snap.txAlarms} onSnap={setSnap} />}
                onOpenSettings={openSettingsAt}
              />
            </div>
          )}
          {/* JS8 keep-alive host — same contract as .rtty-host/.psk-host: the engine keeps
              decoding all four speeds while the operator is on another section; `active`
              gates the display poll and fires js8_enter on the rising edge. Gated on the
              feature toggle (JS8 ships ON, so this mounts unless the operator turned it off).
              `onSetTxEnabled` is the header pill — the only TX latch in this view. */}
          {isViewEnabled('js8') && (
            <div className="js8-host" hidden={effectiveView !== 'js8'}>
              <Js8Cockpit
                onOpenLogbook={openLogbookFor}
                onEntryCall={reportEntry.js8}
                snap={snap}
                onSnap={setSnap}
                active={effectiveView === 'js8'}
                onSetFrequency={handleSetFrequency}
                onSetTxEnabled={handleSetTxEnabled}
                theme={theme}
                wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}
                callsignAgingMin={settings?.js8CallsignAgingMin ?? 0}
                activityAgingMin={settings?.js8ActivityAgingMin ?? 0}
                txOffsetWhileSending={settings?.js8TxOffsetWhileSending === true}
                panels={js8Panels}
                onOpenSettings={openSettingsAt}
                boxes={boxesFor('js8')}
                compose={js8ComposeConfig(settings)}
                onSavedMessages={(js8SavedMessages) => setSettings((prev) => (prev ? { ...prev, js8SavedMessages } : prev))}
                lists={js8Lists}
              />
            </div>
          )}
          {workspace}
        </ErrorBoundary>
        {/* THE DASHBOARD RAIL: the third child of the shell, after the cockpit — never inside a cockpit
            shell (components/DashRail). Only where the window can show it and the operator turned it
            on for this section; its switches are in the cockpit's ⊞ Panels and on the NOW bar. */}
        {railShown && railSection && (
          <DashRail
            section={railSection}
            // Everything its boxes are lent, exactly as the cockpits' boxes are (`boxSource`, above).
            {...boxSource}
            entryCall={entryCallOf(railSection)}
            rail={{
              ...dashRailRecords(dashRailRec, railSection),
              // What this screen shows in each slot (once per screen), and the pick that moves an entry here.
              slots: { ...railRecords!.slots, ...Object.fromEntries(railBoxes.map((b) => [b.slot, b.pane])) },
              stored: railRecords!.slots,
              assignPane: railPick,
              marked: new Set(
                [...hostOwn, ...Object.values(hostEntries)].flatMap((e) => (e != null ? (sharedPaneById(e)?.pane ?? []) : [])),
              ),
            }}
            onHide={() => setRailOn(railSection, false)}
            scale={scale}
          />
        )}
      </div>

      <Toasts />
      {/* Destructive actions confirm through this, not window.confirm — which is inert in the
          macOS webview and silently answered "no" to every one of them. See src/confirm.tsx. */}
      <ConfirmHost />
      {/* Quitting while the logbook is still saving: the station holds this window and says so
          here (src-tauri quit.rs). */}
      <LogbookSaving />
      {/* A browser asking to stream this station: Nexus asks here, with the browser's key, wherever
          the operator is in the app (src/remote-native/RemoteApprovalPrompt.tsx). The shack's alone. */}
      <RemoteApprovalPrompt />
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

      {showWizard && settings && (!wizardAsks || settings.setupWizardSeen === false) && (
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
          onOpenWalk={() => setShowWalk(true)}
          askExperience={wizardAsks}
          rerun={!wizardAsks}
        />
      )}

      {showGuide && (
        <GettingStartedGuide
          onClose={() => setShowGuide(false)}
          onOpenSettings={openSettingsAt}
        />
      )}

      {showWalk && !showGuide && (
        <FirstContactWalk
          onClose={() => setShowWalk(false)}
          doubleClickSetsTx={settings?.doubleClickSetsTx !== false}
        />
      )}

      {snap.pendingLog && (
        // KEYED ON THE HOLD, on every transport (operator ruling 2026-09-19: completed contacts
        // queue behind the popup). The fields are the popup's own state, seeded from the record
        // it mounted with, so without this a promoted contact wore the previous one's call —
        // and Log filed that call onto the wrong station.
        <LogConfirm
          key={snap.pendingQsoLogKey ?? undefined}
          record={snap.pendingLog}
          waiting={snap.pendingLogsWaiting ?? 0}
          autoLogged={snap.pendingLogsAutoLogged ?? 0}
          onConfirm={handleConfirmLog}
          onDiscard={handleDiscardLog}
        />
      )}
    </div>
  )
}

// Memoised: App takes no props, so whatever re-renders above it never reconciles the whole
// workspace; everything App reads arrives through context, which memo never blocks.
export default memo(App)

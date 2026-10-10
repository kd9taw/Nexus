// Standalone-window renderer: when the app is loaded at `?panel=<name>` (a torn-off
// window created by open_panel_window), render JUST that panel — chrome-less, with its
// own polling — against the same shared engine the main window uses. Multi-monitor
// tear-off: pop Connect / DXpeditions / the Operate cockpit / the Needed board onto
// separate displays so the operator stops toggling. Each detached window is its own
// independent client of the one shared Rust engine (snapshot at 300 ms; the Waterfall
// self-fetches its spectrum), and its action callbacks drive the same engine, so state
// stays consistent across every window.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). It is a router: the
// panels it mounts own their own prose. What is here is the states the router itself can be
// in — connecting, Field Day off, club sync off, and a panel name it does not know — plus the
// conversation-delete guard, which it deliberately raises in the SAME words as the main
// window (the two mirrors drifting apart is what put the guard here). The club-sync-off copy
// is the router's because it is about a panel that ISN'T mounted: it names the Settings route
// that would fill the board, and the board component never renders in that state.
import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from 'react'
import { t } from './i18n'
import { publishBandConditions } from './bandConditions'
import { confirmDialog, ConfirmHost } from './confirm'
import { setPopupNotifications, withErrorToast } from './toast'
import { pollSingleFlight } from './singleFlight'
import { WSPR_WATERFALL_WINDOW, markerWidthHz } from './waterfall'
import type {
  AppSnapshot,
  BandChannel,
  Conversation as Conv,
  ModeRequest,
  NeedAlert,
  PropagationSnapshot,
  Settings,
  SourceKind,
  SpotRow,
  Tier,
} from './types'
import {
  getBandPlan,
  getNeedAlerts,
  getPropagation,
  getSettings,
  getAllSpots,
  selectPeer,
  archiveConversation,
  setFrequency,
  workSpot,
  setHuntTarget,
  subscribeSnapshot,
  callStation,
  setTier,
  setSource,
  setTxLevel,
  setMode,
  setTxEven,
  setTxCycleAuto,
  qsoResend,
  qsoFreetext,
  logCurrentQso,
  overrideNextTx,
  haltTx,
  setRxOffset,
  setTxOffset,
  pointRotatorAtCall,
  setTxEnabled,
  setTune,
  setHoldTxFreq,
  dockBandmapWindow,
  setSettings as persistSettings,
  setFdOperator,
  setSidebandOverride,
  getFdRuleset,
  type FdRulesetDto,
} from './api'
import { markRecalled, memoriesStore, planRecall, type Memory } from './features/memories'
import { failureToShow, useLatestLogAnswer, useLogStatus } from './features/logSource'
import { bandLabelForMhz } from './band'
import { sameCall } from './callsign'
import { MemoriesView } from './components/MemoriesView'
import { NeededPanel } from './components/NeededPanel'
import { PotaSotaView, type OtaSpotClickArg } from './components/PotaSotaView'
import { BandMap } from './components/BandMap'
import { ConnectView } from './components/ConnectView'
import { DashboardBar, StayBehindToggle } from './components/DashboardBar'
import { MapView } from './components/MapView'
import { DxpeditionsView } from './components/DxpeditionsView'
import { SatellitesView } from './components/SatellitesView'
import { SstvViewer } from './components/SstvViewer'
import { Toasts } from './components/Toasts'
import { OperateCockpit } from './components/OperateCockpit'
import { FdClubSection, FieldDayScoreboard, FdBandOccupancy } from './components/ContestView'
import { clubSyncRefusal, clubSyncRefusalText, contestName, isFieldDay } from './fdEvent'
import { Waterfall } from './components/Waterfall'
import { FT_PALETTE_SCOPE } from './waterfallPalette'
import { StationList } from './components/StationList'
import { visibleNeeds, boardNeeds, modeClassOf, workTarget, alertsByCall, activityTypeByCall, topNeedByCall } from './features/needs'
import { spotNeed } from './remote-web/remote-work'
import { OPERATE_PANELS, usePanelLayout } from './features/panelState'
import { surfaceGet, surfaceSet } from './features/windowScope'
import { readEnabledModes } from './useFeatures'
import { useTheme } from './useTheme'
import { useContrastPrefs } from './useFieldMode'
import { usePaletteRoles } from './usePaletteRoles'
import { useSkin } from './useSkin'
import { useNight } from './useNight'
import { useScale } from './useScale'
import { useViewport } from './useViewport'
import { useDensity } from './useDensity'
import { textPx, useTextSize } from './useTextSize'
import { useMotion } from './useMotion'

// `program`/`reference` carry a park identity (POTA/SOTA) when the spot is one — see
// `onWorkSpot` below, which is the PRIMARY surface for tagging the hunt target from a
// torn-off window (the 'connect' branch wires this straight into MapView).
type SpotTarget = {
  call: string
  band: string
  mode: string | null
  freqMhz: number | null
  program?: string
  reference?: string
}
type OperateLayout = 'classic' | 'roster'

// The club board's SYNC-OFF panel. Inline off the shared tokens (the ContestView
// idiom) rather than a styles.css section: four elements in one branch. Set larger
// than body copy for the same reason the board itself is — this window is read from
// the operating position, and the route is something the operator retypes elsewhere.
const FDCLUB_OFF_WRAP: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 12,
  margin: 'auto',
  maxWidth: 620,
  padding: '0 32px',
}
const FDCLUB_OFF_HEAD: CSSProperties = {
  margin: 0,
  fontSize: textPx(22),
  fontWeight: 700,
  color: 'var(--text)',
}
const FDCLUB_OFF_BODY: CSSProperties = {
  margin: 0,
  fontSize: textPx(16),
  lineHeight: 1.5,
  color: 'var(--text-dim)',
}
const FDCLUB_OFF_ROUTE: CSSProperties = {
  margin: 0,
  padding: '12px 14px',
  borderRadius: 'var(--radius)',
  border: '1px solid var(--border)',
  background: 'var(--bg-elev-2)',
  fontSize: textPx(16),
  lineHeight: 1.5,
  color: 'var(--text)',
}
const FDCLUB_OFF_WAIT: CSSProperties = {
  margin: 0,
  fontSize: textPx(14),
  color: 'var(--text-faint)',
}

// PER-SURFACE, like App's 'nexus.operateLayout'. NB these are two differently-spelled keys
// for one concept and already disagreed before this change — deliberately left as-is here,
// because merging them would alter what the main window reads off disk.
function loadOperateLayout(): OperateLayout {
  // Roster is the default; only an explicit 'classic' choice keeps Classic.
  return surfaceGet('nexus.operate.layout') === 'classic' ? 'classic' : 'roster'
}

/** A torn-off window is a SEPARATE JS REALM, not another branch of the main window's tree, so
 *  it needs its own confirm host: `confirmDialog` resolves through a module-level global, and in
 *  this document that global is `null` until something mounts one here. Without it the guarded
 *  action fails closed — it logs and answers "no" — which is how the ✕ on a conversation in this
 *  panel did nothing at all.
 *
 *  Mounted ONCE around the whole panel rather than beside each branch, because the body below
 *  returns from fifteen of them. Per-branch would reproduce the very bug this fixes: a host
 *  present in one tree and absent in another, failing silently in whichever branch someone
 *  forgets. The dialog portals (`RD.Portal`), so where this sits in the tree has no bearing on
 *  layout — only on whether it exists at all.
 *
 *  ⚠️ `<Toasts/>` CANNOT BE HOISTED TO SIT BESIDE IT, AND THAT IS NOT AN OVERSIGHT. `zoom` lives
 *  on `.app` (styles.css) and the toast viewport sizes itself against `--vh-eff`, so a host
 *  outside the branch's `.app` renders at the wrong scale in a window the operator has zoomed.
 *  It has to be INSIDE that tree — which is why no branch writes its own
 *  `<div className="app detached">` any more and every one returns [`DetachedShell`], which
 *  carries the host. A new branch then gets one by construction and cannot omit it.
 *
 *  It WAS per-branch, and the Memories pop-out — one of the seven that never mounted one —
 *  turned that into data loss: its bulk delete asks "Delete 40 memories?", promises in the
 *  confirm body that "the toast that follows can undo it", deletes, and then the Undo it just
 *  promised does not exist, because `pushToast` resolves through a module-level bus and that
 *  document rendered nothing to receive it. */
export function DetachedPanel({ panel }: { panel: string }) {
  return (
    <>
      <DetachedPanelBody panel={panel} />
      <ConfirmHost />
    </>
  )
}

/** The root of every pop-out branch: the zoomed `.app` tree plus this window's toast host.
 *  Use this, never a bare `<div className="app detached">` — see the warning above. */
function DetachedShell({ className, children }: { className?: string; children?: ReactNode }) {
  return (
    <div className={className ? `app detached ${className}` : 'app detached'}>
      {children}
      <Toasts />
    </div>
  )
}

function DetachedPanelBody({ panel }: { panel: string }) {
  const [theme] = useTheme()
  // The built-in theme rides on the theme: this document's own writer of `data-skin`.
  useSkin(theme)
  // Pop-outs follow the contrast axis: a separate document re-applies the attribute itself,
  // the same way it mirrors the theme — outdoors, and an operator's eyes, are facts about the
  // station, not about a window. Only `fieldMode` reaches useScale; high contrast on its own
  // must leave this window's zoom exactly where the operator left it.
  const { fieldMode } = useContrastPrefs()
  // And the colour roles: this document's own writer of the data-<role> attributes, as useTheme
  // is of data-theme (the preseed already painted them; this keeps them owned).
  usePaletteRoles()
  // A torn-off window is its OWN document — it must publish the same layout/responsive
  // state the main app does, or the CSS falls back to the broken narrow/stacked layout
  // (vertical rails go horizontal, the map collapses to zero height). Mirror App.tsx.
  const { scale } = useScale(fieldMode)
  useViewport(scale)
  useDensity()
  useTextSize()
  useMotion()
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  // Night follows the station into a pop-out: this document's own writer of data-night, going by
  // the same grid square and the same sun as the main window.
  useNight(snap?.mygrid ?? '')
  const [settings, setSettings] = useState<Settings | null>(null)
  // The club board's window only: the station's preview of the picked contest, which says
  // whether the rules it loaded carry it (`rulesCarry`) — so a refused club says why here too.
  const [fdRuleset, setFdRuleset] = useState<FdRulesetDto | null>(null)
  useEffect(() => {
    if (panel !== 'fdclub') return
    let live = true
    getFdRuleset()
      .then((r) => live && setFdRuleset(r))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [panel, settings?.fdEvent])
  // Waterfall pop-out ⇄ dock: while this torn-off waterfall window lives, the main cockpit hides
  // its docked copy so the decode lists + roster get the room. On close (or unmount) we clear the
  // flag; the main window's `storage` listener then re-docks automatically. The main cockpit also
  // has an always-visible manual "re-dock" as the fallback if this never fires.
  useEffect(() => {
    if (panel !== 'waterfall') return
    const KEY = 'nexus.waterfall.detached'
    localStorage.setItem(KEY, '1')
    const clear = () => localStorage.setItem(KEY, '0')
    window.addEventListener('beforeunload', clear)
    return () => {
      clear()
      window.removeEventListener('beforeunload', clear)
    }
  }, [panel])
  const [prop, setProp] = useState<PropagationSnapshot | null>(null)
  const [needAlerts, setNeedAlerts] = useState<NeedAlert[]>([])
  const [bandPlan, setBandPlan] = useState<BandChannel[]>([])
  const [operateLayout, setOperateLayout] = useState<OperateLayout>(loadOperateLayout)
  // This window is its OWN surface (instance `w1` by default), so its ⊞ Panels choices
  // are independent of the docked cockpit's — that is the whole point of keying the
  // record per surface instead of one app-global flag.
  const operatePanels = usePanelLayout(OPERATE_PANELS)
  // Band-map pop-out only: the live spot feed + which calls are in the log (worked).
  const isBandMap = panel === 'bandmapPhone' || panel === 'bandmapCw'
  const [allSpots, setAllSpots] = useState<SpotRow[]>([])
  // Selection mirrors the shared engine (snap.activePeer), so a station picked in the main
  // window — or in this one — highlights consistently across every window.
  const selected = snap?.activePeer ?? null

  // Live snapshot (decodes, stations, radio) — same 300 ms cadence as the main window.
  useEffect(() => subscribeSnapshot(setSnap), [])

  // Band-map pop-out: which of the calls ON THE MAP are in the log — asked of `LogSource`, which
  // follows this window's own snapshot `logTick`, instead of holding the whole log to answer it.
  // The rule is the band map's own: the call upper-cased and NOT trimmed, on both sides. The
  // spot poll hands over a fresh array every 15 s; the question only changes with the call set.
  // A poll that brings a new call asks a new question, and until it lands the map keeps the latest
  // answer for the calls it covered (no strike-through drops for the round trip); a call that
  // answer did not cover shows "—".
  const spotCalls = useMemo(() => [...new Set(allSpots.map((s) => s.call.toUpperCase()))].sort(), [allSpots])
  const workedQuestion = isBandMap ? ({ kind: 'workedCalls', calls: spotCalls } as const) : null
  const worked = useLatestLogAnswer(workedQuestion, snap?.logTick)
  const workedFailed = failureToShow(useLogStatus(workedQuestion))
  const workedCalls = useMemo(() => new Set(worked?.answer ?? []), [worked])
  const unansweredCalls = useMemo(() => {
    const answered = new Set(worked?.question.calls)
    return new Set(spotCalls.filter((c) => !answered.has(c)))
  }, [worked, spotCalls])

  // Refetch the band plan when the tier changes — FT8/FT4 use different dial frequencies
  // (14.074 vs 14.080), so a detached Operate window's QSY targets must follow the mode.
  useEffect(() => {
    let live = true
    getBandPlan().then((b) => live && setBandPlan(b)).catch(() => {})
    return () => {
      live = false
    }
  }, [snap?.link.tier])

  // Propagation + needs + band plan + settings: this window polls the shared engine. Each poll
  // is single-flight (#335): all three take the engine mutex, and a propagation read can run
  // for seconds behind a slow fetch, so a tick skips while its last read is still out — this
  // window is open by default, and every stacked read held a backend worker.
  useEffect(() => {
    let live = true
    const stopProp = pollSingleFlight('pop-out propagation', 10_000, (owns) =>
      getPropagation()
        .then((p) => {
          if (!owns()) return
          setProp(p)
          // A pop-out is its own JS context: publish so its band dropdown shows conditions too.
          publishBandConditions(p)
        })
        .catch(() => {}),
    )
    const stopNeeds = pollSingleFlight('pop-out needs', 15_000, (owns) =>
      getNeedAlerts().then((a) => owns() && setNeedAlerts(a)).catch(() => {}),
    )
    // Settings aren't in the snapshot, so poll them too — otherwise a preferRrr / QSO-macro
    // change in the main window never reaches the detached cockpit.
    const stopSettings = pollSingleFlight('pop-out settings', 15_000, (owns) =>
      getSettings().then((s) => owns() && setSettings(s)).catch(() => {}),
    )
    getBandPlan().then((b) => live && setBandPlan(b)).catch(() => {})
    return () => {
      live = false
      stopProp()
      stopNeeds()
      stopSettings()
    }
  }, [])
  // #391: this window's toast host follows the pop-up setting too, from the settings it polls.
  useEffect(() => {
    setPopupNotifications(settings?.popupNotifications !== false)
  }, [settings?.popupNotifications])

  // Band-map pop-out: poll the live spot feed. (The worked set follows the log above.) The
  // dashboard window polls it too, at the main window's 15 s, for its Spots box: the board's feed.
  const pollsSpots = isBandMap || panel === 'connect'
  useEffect(() => {
    if (!pollsSpots) return
    let live = true
    const load = () => {
      getAllSpots().then((s) => live && setAllSpots(s)).catch(() => {})
    }
    load()
    const id = setInterval(load, 15_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [pollsSpots])

  // Drive a command then mirror the returned snapshot immediately (the 300 ms poll would
  // catch it anyway, but this keeps the cockpit snappy).
  const apply = (p: Promise<AppSnapshot>) => {
    void p.then((s) => s && setSnap(s)).catch(() => {})
  }

  // `freqMhz` is the spot's exact frequency (source of truth — DXpeditions run off the
  // standard dial); fall back to the band's dial only when the spot has no frequency.
  const qsyBand = (band: string, freqMhz?: number) => {
    const ch = bandPlan.find((c) => c.band === band)
    if (ch) apply(setFrequency(freqMhz ?? ch.dialMhz, ch.band, ch.mode))
  }
  const onSelect = (call: string | null) => {
    // Drives the shared engine; `selected` then reflects it via the snapshot.
    // `null` is a real command — it clears the engine's active peer (deselect on
    // empty-map click / ✕ / re-click a dot); swallowing it left selection stuck.
    void selectPeer(call).catch(() => {})
  }
  // Mirrors App.tsx's handleArchive — the detached window had a silent no-op here, so the
  // ✕ did nothing at all in this panel.
  const onArchive = async (peer: string) => {
    if (
      !(await confirmDialog({
        title: t('shell.conversation.delete.title', { peer }),
        body: t('shell.conversation.delete.body'),
        confirmLabel: t('shell.conversation.delete.action'),
        danger: true,
      }))
    )
      return
    apply(archiveConversation(peer))
  }
  // `spot`, not `t` — `t` is the translator in this file.
  const onWorkSpot = async (spot: SpotTarget) => {
    const mode = modeClassOf(spot.mode).toLowerCase() as 'cw' | 'phone' | 'digital'
    // Nothing here can move the rig: no frequency of its own AND no channel for the band, which
    // is exactly the case `qsyBand` below silently does nothing about. Tagging anyway armed a
    // four-hour pend (HUNT_TTL_SECS) for a QSY that provably did not happen, waiting to stamp
    // that park on the next contact with the callsign, whatever band it was made on.
    if (spot.freqMhz == null && !bandPlan.some((c) => c.band === spot.band)) return
    // Tag the hunt target BEFORE the QSY — same order as PotaSotaView's own
    // setHuntTarget-then-QSY split (handleHunt) — so a POTA map pop-out credits the activator
    // too, not just the QSY. Awaited and said out loud: `set_hunt_target` rejects on a reference
    // it cannot normalize, and swallowing that left the contact logged with no park at all.
    if (spot.program && spot.reference) {
      const { program, reference } = spot
      await withErrorToast(
        () => setHuntTarget(spot.call, program, reference),
        t('ota.hunt.setFailed', { call: spot.call }),
      )
    }
    if (spot.freqMhz != null) apply(workSpot(mode, spot.freqMhz, spot.band, spot.call))
    else qsyBand(spot.band)
  }
  // THIS WINDOW'S BOARD PATHS, named so the boards and the Connect boxes share them. The Needed
  // board's work (below, its arm) and the POTA/SOTA board's hunt: the atomic workSpot, whose
  // snapshot nav-hint (workTick) makes the MAIN window follow to the matching cockpit — this
  // window cannot navigate it. The dashboard window's Spots box works a spot as the Spots view
  // does in the main window, `handleWorkSpot = handleWorkNeeded(spotNeed(s))`: this window's
  // Needed work applied to the spot as a need. Neither keys anything.
  //
  // Full work path from the pop-out: the atomic workSpot switches the rig's MODE + exact
  // frequency (a bare QSY left CW clicks in DATA-U).
  const workNeed = async (a: NeedAlert) => {
    // `target`, not `t` — `t` is the translator in this file (App.tsx's own idiom).
    const target = workTarget(a, bandPlan)
    if (!target) {
      qsyBand(a.band, a.freqMhz ?? undefined)
      return
    }
    // A park/summit row names its activation, and the contact this leads to carries the park,
    // as from the docked board (handleWorkNeeded spells out the rule). A row that opens the
    // main window's Phone or CW log line hands its park there with the call, through the work
    // hint (`workSpot`'s park → the snapshot's `workPark`), and sets NO hunt. Any other row
    // still tags the hunt first: it is the only way that row's park reaches the contact.
    // AFTER the bail-out above and AWAITED — handleWorkNeeded spells out why, and here
    // the bail-out is the harder no-op of the two: `workTarget` is null only when the
    // band has no plan channel, and `qsyBand` then silently moves nothing at all.
    const modes = readEnabledModes()
    const fillsLogLine = (target.view === 'cw' && modes.cw) || (target.view === 'phone' && modes.phone)
    if (a.park && !fillsLogLine) {
      const park = a.park
      await withErrorToast(
        () => setHuntTarget(a.call, park.program, park.reference),
        t('ota.hunt.setFailed', { call: a.call }),
      )
    }
    // The board lists ALL modes, but the CW/Phone cockpits are opt-in features.
    // If the target cockpit is disabled, the MAIN window's nav-hint effect refuses
    // to follow (same gate as handleWorkNeeded) — so a workSpot would silently
    // switch the rig into a hidden mode with no UI. Just QSY to the spot instead.
    if ((target.view === 'cw' && !modes.cw) || (target.view === 'phone' && !modes.phone)) {
      qsyBand(a.band, a.freqMhz ?? undefined)
      return
    }
    const opMode = target.view === 'operate' ? 'digital' : target.view
    // A digital spot's FT8/FT4 protocol rides the same atomic call (the engine
    // no-ops on a same-tier request) — the pop-out used to not switch the tier at
    // all, leaving an FT4 click decoding FT8, and doing it as a second call would
    // recreate the main window's default-dial-first double retune.
    const m = a.mode?.toUpperCase()
    const spotTier = opMode === 'digital' && (m === 'FT4' || m === 'FT8') ? m : undefined
    apply(
      a.park && fillsLogLine
        ? workSpot(opMode, target.freqMhz, target.band, target.call, spotTier, a.park)
        : workSpot(opMode, target.freqMhz, target.band, target.call, spotTier),
    )
  }
  // The POTA/SOTA board has already called setHuntTarget itself (and handed us the fresh
  // snapshot via onSnap); this half is the QSY + rig-mode switch — the same atomic workSpot
  // the Needed work uses, with its same guard: a spot whose cockpit is a DISABLED feature only
  // QSYs, because the main window's nav-hint effect would refuse to follow a hidden mode.
  const huntOta = (a: OtaSpotClickArg) => {
    const modes = readEnabledModes()
    const view = a.modeClass === 'CW' ? 'cw' : a.modeClass === 'Phone' ? 'phone' : 'operate'
    if ((view === 'cw' && !modes.cw) || (view === 'phone' && !modes.phone)) {
      qsyBand(a.band, a.freqMhz)
      return
    }
    const opMode = view === 'operate' ? 'digital' : view
    apply(workSpot(opMode, a.freqMhz, a.band, a.call))
  }
  // Work a decoded/roster station from the cockpit (guards the self-QSO false toast).
  const onCall = (call: string, grid?: string, message?: string, snr?: number, freq?: number) => {
    const me = (snap?.mycall ?? '').trim()
    if (me && sameCall(call, me)) return
    apply(callStation(call, grid, message, snr, freq))
  }
  const onTune = (hz: number, target: 'tx' | 'rx' | 'both') => {
    if (target === 'rx') apply(setRxOffset(hz))
    else if (target === 'tx') apply(setTxOffset(hz))
    else apply(setTxOffset(hz).then(() => setRxOffset(hz)))
  }
  const changeLayout = (m: OperateLayout) => {
    setOperateLayout(m)
    surfaceSet('nexus.operate.layout', m)
  }

  // The per-type alert band scopes, exactly as App builds them — a torn-off surface must
  // not disagree with the docked one about which need icons this band earns.
  const needScopes = useMemo(
    () => ({
      dxcc: settings?.alertDxccBands,
      grid: settings?.alertGridBands,
      rareGrid: settings?.alertRareGridBands,
    }),
    [settings?.alertDxccBands, settings?.alertGridBands, settings?.alertRareGridBands],
  )
  // Connect's map colours stations by need the SAME way the docked map does — gated by the
  // operator's enabled modes (the Needed board has its own per-mode toggles separately) and
  // by the band scopes, which govern the icons as well as the alerts.
  const gatedAlerts = useMemo(
    () => visibleNeeds(needAlerts, readEnabledModes(), needScopes),
    [needAlerts, needScopes],
  )
  // The Spots board's needs (Hide worked's rescue), as App hands its Spots board: band scopes
  // honoured, mode-feature neutral (`boardNeeds`). For the dashboard window's Spots box.
  const boardAlerts = useMemo(() => boardNeeds(needAlerts, needScopes), [needAlerts, needScopes])
  // The SHARED chain, same as App.tsx — the hand-rolled loop this replaces was
  // the pre-fix last-tag-wins map (backend orders alerts priority-DESCENDING,
  // so "last" was reliably the WEAKEST need): a new entity on the band in
  // front of you painted in the dim confirmation colour, but only on the
  // pop-out, so the two windows disagreed about the same callsign.
  const grouped = useMemo(() => alertsByCall(gatedAlerts), [gatedAlerts])
  const needByCall = useMemo(() => topNeedByCall(grouped), [grouped])
  const typeByCall = useMemo(() => activityTypeByCall(gatedAlerts), [gatedAlerts])
  const needAlertsByCall = grouped

  if (isBandMap) {
    if (!snap) return <DetachedShell />
    const spotMode: 'CW' | 'Phone' = panel === 'bandmapCw' ? 'CW' : 'Phone'
    return (
      <DetachedShell>
        <BandMap
          band={snap.radio.band}
          dialMhz={snap.radio.dialMhz}
          txAllowed={snap.radio.txAllowed}
          // Phone-segment shade is meaningless on the CW map (matches the inline CW strip).
          phoneSegLo={spotMode === 'Phone' ? snap.radio.phoneSegLo : null}
          phoneSegHi={spotMode === 'Phone' ? snap.radio.phoneSegHi : null}
          spots={allSpots}
          spotMode={spotMode}
          needByCall={needByCall}
          typeByCall={typeByCall}
          workedCalls={workedCalls}
          unansweredCalls={unansweredCalls}
          unansweredTitle={workedFailed !== null ? t('logbook.readFailed', { reason: workedFailed }) : t('logbook.reading')}
          onDock={(side) => void dockBandmapWindow(side)}
          // Tuning from the map (#39). The map is a frequency scale, so it can act as one.
          sideband={snap.radio.sideband || 'USB'}
          tuneEnabled={
            snap.radio.catOk === true && !snap.radio.txBusyReason && !snap.radio.transmitting
          }
          onSnap={setSnap}
          onWorkSpot={(s) =>
            onWorkSpot({ call: s.call, band: s.band, mode: s.mode, freqMhz: s.freqMhz })
          }
        />
      </DetachedShell>
    )
  }

  if (panel === 'waterfall') {
    // The FT8/digital waterfall, torn off — it self-fetches its spectrum; clicks tune
    // the shared engine's RX/TX offsets exactly like the in-cockpit strip. `app` is
    // load-bearing: zoom lives on `.app` (styles.css), and this was the ONE branch
    // missing it — the window ignored the operator's UI scale while its Toasts measured
    // a --vh-eff computed for a zoom that never applied.
    return (
      <DetachedShell className="detached-waterfall">
        <Waterfall
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={snap?.radio.rxOffsetHz ?? 1500}
          txOffsetHz={snap?.radio.txOffsetHz ?? 1500}
          theme={theme}
          onTune={(hz, target) => {
            if (target === 'rx' || target === 'both') void setRxOffset(hz)
            if (target === 'tx' || target === 'both') void setTxOffset(hz)
          }}
          active
          paletteScope={FT_PALETTE_SCOPE}
          // #101: the torn-off copy shows WSPR's sub-band too, like the docked one.
          fixedWindow={snap?.link.tier === 'WSPR' ? WSPR_WATERFALL_WINDOW : undefined}
          // …and the same marker width as the docked one (WSJT-X's bracket for the tier).
          markerWidthHz={markerWidthHz(snap?.link.tier, {
            periodS: snap?.link.periodSecs,
            q65Submode: settings?.q65Submode,
            jt65Submode: settings?.jt65Submode,
          })}
          txBlanks // the torn-off FT waterfall — same surface, same 13 s over.
        />
      </DetachedShell>
    )
  }

  // THIS WINDOW'S NEEDED BOARD, one object for the Needed window below and the dashboard window's
  // Needed box, as App shares its own: a box can never be wired differently from its board.
  const neededBoard = {
    // Full un-gated list — the board's own mode toggles decide what shows.
    alerts: needAlerts,
    bandPlan,
    selectedCall: selected,
    myGrid: snap?.mygrid ?? '',
    onQsy: (a: NeedAlert) => qsyBand(a.band, a.freqMhz ?? undefined),
    onSelect,
    // Full work path from the pop-out too (`workNeed`, above).
    onWork: workNeed,
  }

  if (panel === 'needed') {
    return (
      <DetachedShell>
        <NeededPanel {...neededBoard} />
      </DetachedShell>
    )
  }

  if (panel === 'memories') {
    // Memories, torn off. The bank lives in localStorage and the store already syncs
    // across windows via the 'storage' event, so edits here appear in the main window
    // live (and vice versa). Recall mirrors App's recallMemory minus navigation: the
    // atomic workSpot tunes the shared engine, and the MAIN window follows to the
    // right cockpit via the same snapshot nav-hint the Needed pop-out uses.
    const recall = (m: Memory) => {
      const plan = planRecall(m)
      const target = plan.view
      const opMode: 'digital' | 'phone' | 'cw' = target === 'operate' ? 'digital' : target
      const modes = readEnabledModes()
      if ((target === 'cw' && !modes.cw) || (target === 'phone' && !modes.phone)) {
        return // cockpit disabled — the main window's Settings gate applies
      }
      const mode = m.mode.toUpperCase()
      const band = bandLabelForMhz(plan.freqMhz)
      void (async () => {
        const patch = plan.settingsPatch
        if (patch) {
          const cur = await getSettings()
          const isFm = patch.rptrShift !== undefined
          if (isFm || patch.phoneMode !== cur.phoneMode) {
            await persistSettings({ ...cur, ...patch })
          }
        }
        const s2 = await workSpot(opMode, plan.freqMhz, band)
        if (!s2) return
        apply(Promise.resolve(s2))
        if (target === 'phone' && (mode === 'USB' || mode === 'LSB')) {
          await setSidebandOverride(mode as 'USB' | 'LSB')
        }
        memoriesStore.update((b) => markRecalled(b, m.id, Math.floor(Date.now() / 1000)))
      })()
    }
    return (
      <DetachedShell>
        <MemoriesView
          dialMhz={snap?.radio.dialMhz ?? 0}
          dialMode={snap?.radio.rigMode || snap?.radio.sideband || 'USB'}
          myGrid={snap?.mygrid ?? ''}
          onRecall={recall}
        />
      </DetachedShell>
    )
  }

  if (panel === 'connect') {
    // THE DASHBOARD WINDOW: the clock and space-weather bar across the top, the
    // station from the shared snapshot and the indices from this window's own propagation poll
    // above. Its toggle keeps the window behind the others where the shell offers that; the
    // window's size, place and that choice are the shell's (`window_state` in src-tauri).
    return (
      <DetachedShell>
        <DashboardBar call={snap?.mycall ?? ''} grid={snap?.mygrid ?? ''} prop={prop}>
          <StayBehindToggle />
        </DashboardBar>
        <ConnectView
          myGrid={snap?.mygrid ?? ''}
          theme={theme}
          stations={snap?.stations ?? []}
          prop={prop}
          selectedCall={selected}
          onSelectCall={onSelect}
          needByCall={needByCall}
          onWorkSpot={onWorkSpot}
          needAlerts={gatedAlerts}
          amp={snap?.radio.amp ?? null}
          rigBand={snap?.radio.band ?? null}
          // The Spots, POTA/SOTA and Needed boxes, with THIS window's board paths (`workNeed`,
          // `huntOta`, its Needed board above): the Spots box a spot as a need, as App's
          // handleWorkSpot does, and the board's own feeds — the spot poll above, and the same
          // band-scoped needs App hands its Spots board for Hide worked's rescue. The POTA/SOTA box
          // waits for the first snapshot, as this window's POTA/SOTA arm does.
          spotsFeed={{
            rows: allSpots,
            board: {
              bandPlan,
              selectedCall: selected,
              myGrid: snap?.mygrid ?? '',
              onSelect,
              onWork: (s: SpotRow) => void workNeed(spotNeed(s)),
              needAlerts: boardAlerts,
            },
          }}
          otaBoard={snap ? { snap, onHunt: huntOta, onSnap: setSnap } : undefined}
          neededBoard={neededBoard}
          // A slot's tabs may rotate here, the dashboard window, and on the TV page — never in the
          // main window's Connect (the operator's pick: "Auto-rotating boxes on the dashboard/TV").
          autoRotate
          // The bar above is this window's clock; Connect's header draws none of its own.
          hostBar
          onPoint={
            // Same rotator gate as App (model-launched rotctld OR external host). This failure
            // IS still swallowed — but that is a gap, not a constraint. A detached window has a
            // toast host: `DetachedShell` mounts `<Toasts/>`, and `onWorkSpot` above reports a
            // failed hunt through it. Nothing has been wired to report a failed point yet.
            (settings?.rotatorModel ?? 0) > 0 || settings?.rotatorHost?.trim()
              ? (call) => void pointRotatorAtCall(call).catch(() => {})
              : undefined
          }
        />
      </DetachedShell>
    )
  }

  if (panel === 'dxped') {
    return (
      <DetachedShell>
        <DxpeditionsView snap={prop} onWorkSpot={onWorkSpot} onShowOnMap={onSelect} />
      </DetachedShell>
    )
  }

  if (panel === 'pota') {
    // The POTA/SOTA hunter, torn off — the pop-out its PER-SURFACE filter records
    // were built for: a POTA board beside a SOTA board, each window keeping its own
    // program/filter/sort. The board needs snap.hunt for its banner, so wait for the
    // first snapshot like the Operate arm. Its own toasts (hunt set/cleared, refresh
    // errors) are silent here — a GAP, not the detached pattern it used to be called. This
    // window can report: `DetachedShell` mounts `<Toasts/>` (see `onWorkSpot`, which toasts a
    // failed hunt through it). The Connect arm's `onPoint` is the other one still silent.
    if (!snap) {
      return (
        <DetachedShell>
          <div className="app loading">
            <span>{t('detached.connecting')}</span>
          </div>
        </DetachedShell>
      )
    }
    return (
      <DetachedShell>
        <PotaSotaView
          snap={snap}
          onSnap={setSnap}
          detached
          // The QSY half of a hunt (`huntOta`, above): the board tags the hunt itself first.
          onHunt={huntOta}
        />
      </DetachedShell>
    )
  }

  if (panel === 'sstvviewer') {
    // The received-picture viewer. It owns everything it shows — the gallery it polls, the
    // picture it points at (carried in localStorage from the main window) and its own
    // keyboard — so this branch is the shell and nothing else. No snapshot gate: the
    // pictures are files on disk and a viewer that waits for the radio to answer would sit
    // blank on a station whose rig is off.
    return (
      <DetachedShell className="detached-sstvviewer">
        <SstvViewer />
      </DetachedShell>
    )
  }

  if (panel === 'sats') {
    return (
      <DetachedShell>
        <SatellitesView snap={snap} />
      </DetachedShell>
    )
  }

  if (panel === 'fieldday') {
    const fd = snap?.fieldDay ?? null
    return (
      <DetachedShell>
        {fd ? (
          <FieldDayScoreboard
            fieldDay={fd}
            settings={settings}
            detached
            onSaveOperator={(call) => {
              if (!settings) return
              const op = call.trim().toUpperCase()
              setSettings({ ...settings, fdOperator: op }) // optimistic local mirror (useState)
              // Narrow write, not the whole-struct save: a seat swap is mid-QSO by
              // definition and the heavyweight path would end it (#54).
              apply(setFdOperator(op)) // persist + mirror the returned snapshot
            }}
          />
        ) : (
          <div className="app loading">
            <span>{t('detached.fieldDay.inactive')}</span>
          </div>
        )}
        {/* WHO IS ON WHICH BAND, in the window the operator already tears off beside the
            operator box — the place they asked for it. One row per band, so an empty row
            is the answer to "where can I move?". Only while a club event is running; a
            single-station Field Day has no bands to compete for. */}
        {fd?.club ? <FdBandOccupancy club={fd.club} big /> : null}
      </DetachedShell>
    )
  }

  if (panel === 'fdclub') {
    // The CLUB BAND BOARD, torn off — who is on what band across every
    // position on site, parked on a second monitor so the crew can keep up
    // (the `fieldday` pop-out beside it is the scoreboard, a different
    // surface). Read-only: no export buttons, because their success toast has
    // no host in a detached window, and no operator box — this board is about
    // the other tents, not this one.
    //
    // THREE states, not two, and the split is the findability fix. The rail
    // opens this window whenever Field Day is on, so most operators arrive here
    // BEFORE club sync exists; an empty board would be the same dead end that
    // hid the feature. Waiting for the first snapshot is its own state because
    // it is true for the first 300 ms of every launch, and "no snapshot yet" is
    // not the claim "sync is off".
    if (!snap) {
      return (
        <DetachedShell>
          <div className="app loading">
            <span>{t('detached.connecting')}</span>
          </div>
        </DetachedShell>
      )
    }
    const club = snap.fieldDay?.club ?? null
    // ⚠️ "NO CLUB DATA" IS NOT "SYNC IS OFF", and conflating them made this window lie to
    // the one operator it exists for. The whole `fieldDay` block is built only inside the
    // engine's Field Day mode, so it is absent the moment the operator steps into any other
    // section — one click on the rail does it — regardless of hosting. The host would open
    // the board on a second monitor, click away to check something, and watch a live board
    // become the words "Club sync is off" plus instructions to switch on the hosting that
    // was never switched off. Ask the SETTINGS whether sync is configured, which is true
    // wherever the operator happens to be standing.
    const syncConfigured =
      settings?.fdHostEnable === true || (settings?.fdJoinAddr ?? '').trim() !== ''
    const clubRefusal = clubSyncRefusal(settings, fdRuleset)
    return (
      <DetachedShell>
        {club ? (
          <FdClubSection
            club={club}
            detached
            fieldDay={isFieldDay(snap.fieldDay?.event)}
            keepsDupes={snap.fieldDay?.dupeRule?.logDupes === true}
          />
        ) : clubRefusal ? (
          // Switched on for a contest club sync cannot run, so the engine refuses it:
          // say why, never that the operator is "somewhere else" with a club running.
          <div style={FDCLUB_OFF_WRAP}>
            <h2 style={FDCLUB_OFF_HEAD}>{t('detached.fdClub.off.head')}</h2>
            <p style={FDCLUB_OFF_BODY}>
              {clubSyncRefusalText(clubRefusal, contestName(settings?.fdEvent?.trim()))}
            </p>
          </div>
        ) : syncConfigured ? (
          // Configured, but this window cannot see the club right now — the operator is
          // simply somewhere else in the app. Say that, and say nothing about settings.
          <div style={FDCLUB_OFF_WRAP}>
            <h2 style={FDCLUB_OFF_HEAD}>{t('detached.fdClub.away.head')}</h2>
            <p style={FDCLUB_OFF_BODY}>{t('detached.fdClub.away.body')}</p>
          </div>
        ) : (
          // Named in the words printed on the Settings tab, because this window
          // cannot deep-link into the main window's panel — a detached window is a
          // separate JS realm with no route into it, so the route has to be
          // readable and followed by hand.
          <div style={FDCLUB_OFF_WRAP}>
            <h2 style={FDCLUB_OFF_HEAD}>{t('detached.fdClub.off.head')}</h2>
            <p style={FDCLUB_OFF_BODY}>{t('detached.fdClub.off.body')}</p>
            <p style={FDCLUB_OFF_ROUTE}>{t('detached.fdClub.off.route')}</p>
            <p style={FDCLUB_OFF_WAIT}>{t('detached.fdClub.off.wait')}</p>
          </div>
        )}
      </DetachedShell>
    )
  }

  if (panel === 'operate') {
    if (!snap) {
      return (
        <DetachedShell>
          <div className="app loading">
            <span>{t('detached.connecting')}</span>
          </div>
        </DetachedShell>
      )
    }
    // The cockpit's Call Roster — a wired StationList. Chat-overlay props (unread, archive)
    // are simplified in the detached Operate window; the roster itself is fully live.
    const roster = (
      <StationList
        stations={snap.stations}
        myGrid={snap.mygrid}
        currentSlot={snap.radio.slot}
        activePeer={selected}
        dropAfterCycles={3}
        // Its head is the grip FT's ⊞ Arrange drags it by in this window too (panes/PaneDrag).
        grip="stations"
        unreadByPeer={{}}
        needByCall={needByCall}
        needAlertsByCall={needAlertsByCall}
        band={snap.radio.band}
        feedMode={snap.link.tier}
        onSelect={onSelect}
        onCall={onCall}
        conversations={snap.conversations as Conv[]}
        onArchive={onArchive}
        bandActive={selected === '*'}
        bandUnread={0}
        onSelectBand={() => onSelect('*')}
      />
    )
    return (
      <DetachedShell className="operate-detached">
        <OperateCockpit
          snap={snap}
          theme={theme}
          tier={snap.link.tier}
          onTierChange={(t: Tier) => apply(setTier(t))}
          bandPlan={bandPlan}
          onSetFrequency={(dialMhz: number, band: string, mode: string) =>
            apply(setFrequency(dialMhz, band, mode))
          }
          onSourceChange={(k: SourceKind) => apply(setSource(k))}
          onTune={onTune}
          onCall={onCall}
          onSetTxLevel={(lvl: number) => apply(setTxLevel(lvl))}
          onSetMode={(m: ModeRequest) => apply(setMode(m))}
          onSetTxEven={(even: boolean) => apply(setTxEven(even))}
          onSetTxCycleAuto={(auto: boolean) => apply(setTxCycleAuto(auto))}
          onResend={() => apply(qsoResend())}
          onFreetext={(text: string) => apply(qsoFreetext(text))}
          onLog={() => apply(logCurrentQso().then((r) => r.snapshot))}
          onOverrideTx={(call: string, grid: string | null, text: string) =>
            apply(overrideNextTx(call, grid, text))
          }
          onHaltTx={() => apply(haltTx())}
          onSetTxEnabled={(on: boolean) => apply(setTxEnabled(on))}
          onSetTune={(on: boolean) => apply(setTune(on))}
          onSetHoldTxFreq={(on: boolean) => apply(setHoldTxFreq(on))}
          onSnap={setSnap}
          preferRrr={settings?.preferRrr ?? false}
          qsoMacros={settings?.macros.qso ?? []}
          roster={roster}
          needByCall={needByCall}
          needAlertsByCall={needAlertsByCall}
          needScopes={needScopes}
          selectedCall={selected}
          onSelect={onSelect}
          // #204: the detached cockpit clears the card the same way the main window does.
          onClearSelection={() => onSelect(null)}
          layoutMode={operateLayout}
          onLayoutMode={changeLayout}
          panels={operatePanels}
          active
        />
      </DetachedShell>
    )
  }

  if (panel === 'operatemap') {
    // The POTA map pop-out — a bare MapView, no Connect chrome, with POTA hunting on by
    // default: `intent="pota"` is what turns the Parks (activator) layer on (see
    // MapView's INTENT_PRESETS), the same mechanism the Connect map's intent picker uses.
    // Gated on the first snapshot like the 'pota' arm above — MapView needs snap.mygrid/
    // snap.stations to place anything. `onWorkSpot` is the same tune-and-tag path the
    // 'connect' arm wires in: it tags the hunt target (program+reference present) before
    // the atomic QSY, so double-clicking a park here credits the activator too.
    if (!snap) {
      return (
        <DetachedShell>
          <div className="app loading">
            <span>{t('detached.connecting')}</span>
          </div>
        </DetachedShell>
      )
    }
    return (
      <DetachedShell>
        <MapView
          myGrid={snap.mygrid ?? ''}
          theme={theme}
          stations={snap.stations ?? []}
          prop={prop}
          selectedCall={selected}
          onSelectCall={onSelect}
          needByCall={needByCall}
          onWorkSpot={onWorkSpot}
          intent="pota"
          // This is a surface DEDICATED to POTA hunting, not a torn-off Connect map: it must
          // open on its own intent preset (Parks on), never inherit the Connect map's layer
          // picks off the shared primary key. See MapView's `dedicatedIntent`.
          dedicatedIntent
        />
      </DetachedShell>
    )
  }

  return (
    <DetachedShell>
      <div className="app loading">
        <span>{t('detached.unavailable', { panel })}</span>
      </div>
    </DetachedShell>
  )
}

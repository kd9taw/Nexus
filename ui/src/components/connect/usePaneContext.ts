// THE ONE BUILDER OF A CONNECT BOX'S CONTEXT. Connect's grid and the dashboard rail beside the
// cockpits hand their boxes the same `PaneContext`, built by the same code from the same feeds, so a
// box reads the same wherever it sits — and neither surface can grow a heuristic of its own.
//
// The feeds come from the window's one poll (features/connectFeeds).
import { useMemo } from 'react'
import type {
  AlertView,
  AmpStatus,
  DxpedWindow,
  FieldDayStatus,
  GettingOut,
  MapSpot,
  MufStation,
  NeedAlert,
  NeedTag,
  NoaaScalesView,
  PathPrediction,
  PropagationSnapshot,
  Station,
  WorkableCard,
} from '../../types'
import type { Theme } from '../../useTheme'
import type { MapIntent } from '../MapView'
import { latLonToGrid } from '../../grid'
import { useUnits } from '../../units'
import { useEntityCentroids } from '../../features/entityCentroids'
import {
  BAND_OUTLOOK,
  DXPED_WINDOWS,
  GETTING_OUT,
  KC2G_MUF,
  PATH_OUTLOOK,
  SPACE_WX_SCALES,
  XRAY_NOW,
  useFeed,
  useKeyedFeed,
} from '../../features/connectFeeds'
import { provLabel } from './paneFormat'
import type { NeededBoard, OtaBoard, PaneContext, SpotsFeed } from './paneContext'

/** What the selected call resolves to, against EVERYTHING plotted: a decoded station, a live spot,
 *  a DXpedition card — so a click on any map pixel fills the selection box. */
export interface Selection {
  selStation: Station | null
  selSpot: MapSpot | null
  selDxped: WorkableCard | null
  /** The grid the path outlook is asked for: a station's reported grid, else the spot's position
   *  as a Maidenhead square (centroid-placed spots give the entity's grid — approximate, labelled). */
  selGrid: string | null
}

export function resolveSelection(
  selectedCall: string | null,
  stations: readonly Station[],
  prop: PropagationSnapshot | null,
): Selection {
  const selStation = selectedCall ? (stations.find((s) => s.call === selectedCall) ?? null) : null
  const selSpot = selectedCall && !selStation ? (prop?.spots?.find((sp) => sp.call === selectedCall) ?? null) : null
  // Gated on !selStation: a DXpedition call we ALSO decoded locally renders as the decoded station
  // (worked from the cockpit) — the card's advertised band may differ from the band it was heard on,
  // and Work must never route the rig off what the operator is looking at.
  const selDxped =
    selectedCall && !selStation ? (prop?.dxpeditions.workableNow.find((c) => c.call === selectedCall) ?? null) : null
  const selGrid = !selectedCall
    ? null
    : selStation?.grid
      ? selStation.grid
      : selSpot
        ? latLonToGrid(selSpot.lat, selSpot.lon)
        : null
  return { selStation, selSpot, selDxped, selGrid }
}

/** The feeds a box context carries, from wherever this surface gets them. */
export interface ConnectFeedValues {
  bandOutlook: PathPrediction | null
  getout: GettingOut | null
  scales: NoaaScalesView | null
  alerts: AlertView[]
  muf: MufStation[]
  xrayNow: number | null
  dxpedWindows: Map<string, DxpedWindow>
}

// Stable empties: the map memoises on `muf`, and a fresh [] per render would recompute it each time.
const NO_ALERTS: AlertView[] = []
const NO_MUF: MufStation[] = []

/** The window's one poll of Connect's feeds, as a box context reads them. `enabled` false asks for
 *  nothing (the hosted Remote page). */
export function useConnectFeeds(enabled = true): ConnectFeedValues {
  const bandOutlook = useFeed(BAND_OUTLOOK, enabled).value ?? null
  const getout = useFeed(GETTING_OUT, enabled).value ?? null
  const swx = useFeed(SPACE_WX_SCALES, enabled).value
  const muf = useFeed(KC2G_MUF, enabled).value
  const xray = useFeed(XRAY_NOW, enabled).value
  const windows = useFeed(DXPED_WINDOWS, enabled).value
  const dxpedWindows = useMemo(() => new Map((windows ?? []).map((w) => [w.call.toUpperCase(), w])), [windows])
  return {
    bandOutlook,
    getout,
    scales: swx?.scales ?? null,
    alerts: swx?.alerts ?? NO_ALERTS,
    muf: muf ?? NO_MUF,
    xrayNow: xray?.flux ?? null,
    dxpedWindows,
  }
}

export interface PaneContextInput {
  myGrid: string
  theme: Theme
  intent: MapIntent
  prop: PropagationSnapshot | null
  needByCall: Map<string, NeedTag>
  needAlerts?: NeedAlert[]
  amp?: AmpStatus | null
  rigBand: string | null
  selectedCall: string | null
  selection: Selection
  onSelectCall: (call: string | null) => void
  onWorkSpot?: PaneContext['onWorkSpot']
  onPoint?: (call: string) => void
  focusBand: string | null
  toggleFocusBand: (band: string) => void
  /** The Spots, POTA/SOTA and Needed boxes' boards, lent by the window (PaneContext `spotsFeed` /
   *  `otaBoard` / `neededBoard`); absent where it has none to lend, and those boxes then show their one line. */
  spotsFeed?: SpotsFeed
  otaBoard?: OtaBoard
  neededBoard?: NeededBoard
  /** The running contest (PaneContext `fieldDay`), for the Operators box. */
  fieldDay?: FieldDayStatus | null
  /** The running activation (PaneContext `activation`), for the Activation box. */
  activation?: PaneContext['activation']
  /** The call in the log entry of the cockpit beside the box (PaneContext `entryCall`); absent on every
   *  surface with no cockpit beside it. */
  entryCall?: string | null
  /** The path-over-the-day scrub and its setter (PaneContext `dayScrub` / `onDayScrub`): Connect only. */
  dayScrub?: PaneContext['dayScrub']
  onDayScrub?: PaneContext['onDayScrub']
}

/** A box context, and the X-ray reading the map's flare layer draws (it is not a box's). */
export function usePaneContext(i: PaneContextInput): { ctx: PaneContext; xrayNow: number | null } {
  // Fetched once per window and shared (features/entityCentroids): several boxes render through
  // plain functions that cannot hold the hook themselves.
  const entityCentroids = useEntityCentroids()
  const units = useUnits()
  const f = useConnectFeeds()
  const pathPred = useKeyedFeed(PATH_OUTLOOK, i.selection.selGrid).value ?? null
  const { selStation, selSpot, selDxped, selGrid } = i.selection
  const ctx: PaneContext = {
    myGrid: i.myGrid,
    entityCentroids,
    units,
    theme: i.theme,
    intent: i.intent,
    prop: i.prop,
    prov: i.prop ? provLabel(i.prop.source, i.prop.asOf) : null,
    needByCall: i.needByCall,
    needAlerts: i.needAlerts ?? [],
    amp: i.amp ?? null,
    rigBand: i.rigBand,
    selectedCall: i.selectedCall,
    selStation,
    selSpot,
    selDxped,
    selDxpedWindow: selDxped ? (f.dxpedWindows.get(selDxped.call.toUpperCase()) ?? null) : null,
    dxpedWindows: f.dxpedWindows,
    selGrid,
    pathPred,
    bandOutlook: f.bandOutlook,
    pathOpen: pathPred?.bands.filter((b) => b.workability !== 'Closed') ?? [],
    outlookOpen: f.bandOutlook?.bands.filter((b) => b.workability !== 'Closed') ?? [],
    getout: f.getout,
    focusBand: i.focusBand,
    scales: f.scales,
    alerts: f.alerts,
    muf: f.muf,
    spotsFeed: i.spotsFeed,
    otaBoard: i.otaBoard,
    neededBoard: i.neededBoard,
    fieldDay: i.fieldDay,
    activation: i.activation,
    onSelectCall: i.onSelectCall,
    onWorkSpot: i.onWorkSpot,
    onPoint: i.onPoint,
    toggleFocusBand: i.toggleFocusBand,
    entryCall: i.entryCall ?? null,
    dayScrub: i.dayScrub ?? null,
    onDayScrub: i.onDayScrub,
  }
  return { ctx, xrayNow: f.xrayNow }
}

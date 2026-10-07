// The single object handed to every Connect pane — built once in ConnectView from
// already-lifted state (no new computation). Field set is exactly what the B1 panes
// consume, plus a few forward-compat fields for B2/B3.
import type { Theme } from '../../useTheme'
import type { LatLon } from '../../grid'
import type { Units } from '../../units'
import type { MapIntent } from '../MapView'
import type { SpotsPanelProps } from '../SpotsPanel'
import type { NeededPanelProps } from '../NeededPanel'
import type { OtaSpotClickArg } from '../PotaSotaView'
import type {
  AlertView,
  AmpStatus,
  AppSnapshot,
  BandOutlook,
  DxpedWindow,
  GettingOut,
  MapSpot,
  MufStation,
  NeedAlert,
  NeedTag,
  NoaaScalesView,
  PathPrediction,
  PropagationSnapshot,
  SpotRow,
  Station,
  WorkableCard,
} from '../../types'

/** The Spots board as its window has it: the feed it lists and the wiring its view is given (App's
 *  `spotsBoard`, the object the Spots view and the Phone cockpit's Spots pane get). */
export interface SpotsFeed {
  rows: SpotRow[]
  board: Omit<SpotsPanelProps, 'spots' | 'pane'>
}

/** The Needed board as its window has it: App's `neededBoard`, the object the Needed view and the Phone
 *  and CW cockpits' Needed panes get (its rows are in it). A box gives it a filter record of its own. */
export type NeededBoard = Omit<NeededPanelProps, 'pane' | 'onPopOut'>

/** The POTA/SOTA board's wiring as its window has it — what the POTA/SOTA view is given. */
export interface OtaBoard {
  snap: AppSnapshot
  onHunt: (arg: OtaSpotClickArg) => void
  onSnap: (s: AppSnapshot) => void
}

export interface PaneContext {
  // environment (B2/B3-ready; B1 panes mostly read prop/selection)
  myGrid: string
  /** DXCC entity → representative location, for the beam heading a pane prints beside
   * an entity name. Lives on the context because several panes render through plain
   * functions (`renderSelection`), which cannot call the hook themselves. Null until
   * the table arrives — panes then show grid-derived headings only. */
  entityCentroids: ReadonlyMap<string, LatLon> | null
  /** Settings ▸ Units, resolved (Automatic already decided by the OS locale). On the context for
   * the same reason as `entityCentroids`: the boxes that print a distance render through plain
   * functions, which cannot call `useUnits()` themselves. */
  units: Units
  theme: Theme
  intent: MapIntent
  // shared live state
  prop: PropagationSnapshot | null
  prov: { label: string; cls: string } | null
  needByCall: Map<string, NeedTag>
  needAlerts: NeedAlert[] // reserved for B2 best-band/needs pane
  // selection lifecycle
  selectedCall: string | null
  selStation: Station | null
  selSpot: MapSpot | null
  selDxped: WorkableCard | null
  /** The selected DXpedition's modelled best-shot window, when known. */
  selDxpedWindow: DxpedWindow | null
  /** ALL modelled expedition windows by UPPERCASE call (the chase feed ranks with them). */
  dxpedWindows: Map<string, DxpedWindow>
  selGrid: string | null
  // outlook (API-fetched in ConnectView)
  pathPred: PathPrediction | null
  bandOutlook: PathPrediction | null
  pathOpen: BandOutlook[]
  outlookOpen: BandOutlook[]
  // getting-out + band focus
  getout: GettingOut | null
  focusBand: string | null
  /** The active radio's amplifier, straight off the snapshot App already polls at 300 ms —
   * NOT a poll of its own. `null`/absent = no amplifier configured, which is what makes the
   * Amplifier pane draw only its one line, no readout. Display-only: it gates and stops nothing. */
  amp: AmpStatus | null
  /** The band the active radio is on, off the same snapshot (the band tiles ring it). Null when
   *  unknown — the Remote browser, the wall display. Display-only. */
  rigBand: string | null
  // B3 live external data (desktop-only; null/empty until the feeds answer)
  scales: NoaaScalesView | null
  alerts: AlertView[]
  muf: MufStation[]
  /** The Spots box's list and its Work, exactly the Spots board's (SpotsFeed). ABSENT where the
   *  window has no Spots board to lend — the wall display, whose read-only server serves neither
   *  the spot list nor any command — and then the box shows its one-line state and no Work. */
  spotsFeed?: SpotsFeed
  /** The POTA/SOTA box's wiring, exactly the POTA/SOTA board's (OtaBoard). Absent ⇒ its one-line
   *  state and no HUNT, as above. */
  otaBoard?: OtaBoard
  /** The Needed box's board, exactly the Needed board's (NeededBoard). Absent ⇒ its one-line state and
   *  no Work: the wall display, and the hosted Remote page (the desktop first, 2026-10-07). */
  neededBoard?: NeededBoard
  // callbacks
  onSelectCall: (call: string | null) => void
  onWorkSpot?: (t: { call: string; band: string; mode: string | null; freqMhz: number | null }) => void
  /** Point the rotator at a call (present only when a rotator is configured). */
  onPoint?: (call: string) => void
  toggleFocusBand: (band: string) => void
}

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The intent chips'
// words come from the catalog through getters; the two named for a programme or a band keep
// their token label (INTENT_TOKENS below), and `3D`/`2D` are the two renderers' own names.
//
// Connect — the unified situational-awareness surface. The grayline map and the
// live propagation nowcast are TWO VIEWS OF ONE STATE: both read the same prop
// snapshot, operator grid, heard stations, need-state, and selection lifted in
// App. Selecting a station on the map highlights its great-circle path here; the
// surrounding panes answer "what's open, where to point, what do I need" at a glance.
// The panes are an assignable wrap-the-globe grid: every panel is a
// reassignable pane with a Basic (one plain sentence) and Expert (full data) view; the
// globe stays the untouched centerpiece. See components/connect/* + features/connectConfig.
import { useState, useEffect, useId, useMemo, useRef, lazy, Suspense, type ReactNode } from 'react'
import type { NeedAlert, NeedTag, PropagationSnapshot, Station } from '../types'
import type { AmpStatus } from '../types'
import type { Theme } from '../useTheme'
import { effectiveXray } from '../flareAlert'
import { gpuCapableForGlobe } from '../gpu'
import { gridToLatLon, type LatLon } from '../grid'
import { packFor, useStreetMaps } from '../features/streetMaps'
import { StreetDownloadSheet } from './StreetDownloadSheet'
import { MapView, setIntentMapLayer, type MapIntent } from './MapView'
import { setGlobeLayer } from '../features/globeLayers'
// The 3-D WebGL globe is LAZY-loaded: three.js only downloads when an operator turns on
// 3-D mode, so the 2-D default (which runs anywhere) never pays for it.
const Globe3D = lazy(() => import('./Globe3D'))
import { PaneFrame } from './connect/PaneFrame'
import { UtcClock } from './UtcClock'
import { DashboardBar } from './DashboardBar'
import { remoteFeeds, resolveSelection, usePaneContext } from './connect/usePaneContext'
import { paneById } from './connect/panes'
import { RailSplitHandle, RailWidthHandle, useRailWidths } from './connect/RailHandles'
import { PanelsMenu } from './PanelsMenu'
import { PaneSeam } from './PaneSeam'
import { CONNECT_STRIP_MAX_SHARE, CONNECT_STRIP_SPLIT_MAX, CONNECT_STRIP_SPLIT_MIN } from '../features/paneSeam'
import type { OtaBoard, SpotsFeed } from './connect/paneContext'
import {
  SLOT_IDS,
  addableTo,
  loadConnectConfig,
  normalizeConfig,
  saveConnectConfig,
  slotBoxes,
  useConnectConfig,
  type PaneId,
  type SlotId,
} from '../features/connectConfig'
import {
  CONNECT_PRESET_IDS,
  CONNECT_PRESETS,
  STANDARD_LAYOUT,
  connectLayoutNow,
  layoutOf,
  layoutPanels,
  type ConnectLayout,
  type ConnectPresetId,
  type PresetMapLayer,
} from '../features/connectPresets'
import { loadRailWidths, parseRailWidths, saveRailWidths, type RailWidths } from '../features/connectRails'
import { durableGet, durableSet } from '../features/durableStore'
import {
  CONNECT_PANELS,
  coercePanelLayout,
  loadPanelLayout,
  panelStorageKey,
  savePanelLayout,
  usePanelLayout,
} from '../features/panelState'
import { surfaceGet, surfaceHasOwn, surfaceId, surfaceSet } from '../features/windowScope'
import { keepIntentSetup, loadIntentSetup, saveIntentSetup, type MapChoice } from '../features/intentMapSettings'
import { keepSatFavOnly } from '../features/satChase'
import { MapPicker, ALL_MAP_CHOICES, STREET_MAP_CHOICES } from './MapPicker'
import { t } from '../i18n'
import { NavigationMapContext, useNavigation, useSatelliteLive } from '../remote-web/useNavigation'
import { useStationCapability } from '../stationAccess'
import type { ConnectData, PathData, SatelliteData } from '../remote-web/navigation'

/** The two intents NAMED for a programme and a band. POTA and SOTA are the programmes' own
 * names and `6m/VHF` is a band plus a band group — tokens, exactly as they are everywhere
 * else in the app, so these two chips keep their label here while the other two read theirs
 * from the catalog. */
const INTENT_TOKENS = { pota: 'POTA/SOTA', vhf: '6m/VHF' }

/** Intent presets — beginner picks a goal once; map + prop configure themselves.
 *
 * The words resolve LAZILY, through getters: this is a module constant read during render,
 * so resolving at import time would freeze whichever locale loaded first (the treatment
 * `features/needVisuals.ts` established). */
const INTENTS: { id: MapIntent; label: string; title: string }[] = [
  {
    id: 'dx',
    get label() {
      return t('connect.intent.dx.label')
    },
    get title() {
      return t('connect.intent.dx.title')
    },
  },
  {
    id: 'pota',
    label: INTENT_TOKENS.pota,
    get title() {
      return t('connect.intent.pota.title')
    },
  },
  {
    id: 'casual',
    get label() {
      return t('connect.intent.casual.label')
    },
    get title() {
      return t('connect.intent.casual.title')
    },
  },
  {
    id: 'vhf',
    label: INTENT_TOKENS.vhf,
    get title() {
      return t('connect.intent.vhf.title')
    },
  },
]

/** Where each slot sits, in words — a ⊞ Panels entry names the pane AND where it comes back.
 * Literal keys, resolved lazily at render (the INTENTS treatment). */
const SLOT_WHERE: Record<SlotId, () => string> = {
  left1: () => t('connect.slot.where.left1'),
  left2: () => t('connect.slot.where.left2'),
  right1: () => t('connect.slot.where.right1'),
  right2: () => t('connect.slot.where.right2'),
  bottom1: () => t('connect.slot.where.bottom1'),
  bottom2: () => t('connect.slot.where.bottom2'),
  bottom3: () => t('connect.slot.where.bottom3'),
}

/** What the Layout picker offers: Standard first — every pane open at the usual widths, applied by a tap
 * like any layout's and undone the same way (the operator's batch 63), so trying a layout and going back
 * is one tap each way — then, when the one-time switch to Frame + bar kept one, the operator's own
 * earlier layout (`kept`, below), then the presets (features/connectPresets). */
type LayoutChoice = ConnectPresetId | 'standard' | 'kept'
const LAYOUT_CHOICES: readonly LayoutChoice[] = ['standard', ...CONNECT_PRESET_IDS]
const LAYOUT_CHOICES_KEPT: readonly LayoutChoice[] = ['standard', 'kept', ...CONNECT_PRESET_IDS]

/** The ⊞ Layout picker's words (features/connectPresets). Literal keys, resolved lazily at render
 * (the INTENTS treatment). */
const LAYOUT_WORDS: Record<LayoutChoice, { label: () => string; title: () => string }> = {
  standard: { label: () => t('connect.layout.standard'), title: () => t('connect.layout.standard.title') },
  kept: { label: () => t('connect.layout.kept.label'), title: () => t('connect.layout.kept.title') },
  mapFirst: { label: () => t('connect.layout.mapFirst.label'), title: () => t('connect.layout.mapFirst.title') },
  listFirst: { label: () => t('connect.layout.listFirst.label'), title: () => t('connect.layout.listFirst.title') },
  dashboard: { label: () => t('connect.layout.dashboard.label'), title: () => t('connect.layout.dashboard.title') },
  frame: { label: () => t('connect.layout.frame.label'), title: () => t('connect.layout.frame.title') },
  frameBar: { label: () => t('connect.layout.frameBar.label'), title: () => t('connect.layout.frameBar.title') },
}

/** THE LAYOUT PICKER. ONE component, drawn behind two doors: Connect's own Layout button
 * (LayoutMenu, below) and the top of ⊞ Panels, where it has always been. Both are handed the same
 * read-back and the same pick, so the two can never disagree about what is on screen. A column of
 * choices, not a chip row: the popover is 220 px wide, where three chips side by side wrap at Large
 * text or in German. The words over the operator's own arrangement say what a tap costs before it is
 * made. Its ids are its own (`useId`), so the two doors never share one. */
function LayoutPicker({
  now,
  onPick,
  kept,
}: {
  now: LayoutChoice | 'custom'
  onPick: (id: LayoutChoice) => void
  /** The one-time switch kept this surface's earlier layout: offer it. */
  kept: boolean
}) {
  const id = useId()
  return (
    <div className="connect-layouts" role="group" aria-labelledby={`${id}-head`}>
      <div className="connect-layouts-head">
        <span id={`${id}-head`}>{t('connect.layout.heading')}</span>
        <span className="connect-layout-now">
          {now === 'custom' ? t('connect.layout.custom') : LAYOUT_WORDS[now].label()}
        </span>
      </div>
      {(kept ? LAYOUT_CHOICES_KEPT : LAYOUT_CHOICES).map((p) => (
        <button
          key={p}
          type="button"
          className={`connect-layout-opt${now === p ? ' active' : ''}`}
          aria-pressed={now === p}
          aria-describedby={now === 'custom' ? `${id}-cost` : undefined}
          title={LAYOUT_WORDS[p].title()}
          onClick={() => onPick(p)}
        >
          {LAYOUT_WORDS[p].label()}
        </button>
      ))}
      {now === 'custom' && (
        <span className="connect-layout-note" id={`${id}-cost`}>
          {t('connect.layout.replaces')}
        </span>
      )}
    </div>
  )
}

/** CONNECT'S LAYOUT BUTTON (the operator's pick, 2026-10-01: "A visible Layout button"). The layouts
 * sat only at the top of ⊞ Panels, and the operator went looking for them on Connect and did not find
 * them. This opens the same LayoutPicker with the same Undo — the panel record's one history, so an
 * Undo pressed here or in ⊞ Panels takes back the same step (the picker's own words name that button).
 * It closes as ⊞ Panels does: a click anywhere else, or Escape, which goes back to the button and does
 * NOT stop propagating — on Connect Escape is also the stop (App), and it has to get there. */
function LayoutMenu({ picker, onUndo, canUndo }: { picker: ReactNode; onUndo: () => void; canUndo: boolean }) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const btnRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', onDown)
    return () => document.removeEventListener('pointerdown', onDown)
  }, [open])
  return (
    <div className="connect-layout-menu" ref={rootRef}>
      <button
        type="button"
        ref={btnRef}
        className={`connect-layout-btn${open ? ' active' : ''}`}
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title={t('connect.layout.button.title')}
      >
        {t('connect.layout.button')}
      </button>
      {open && (
        <div
          className="connect-layout-pop"
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setOpen(false)
              btnRef.current?.focus({ preventScroll: true })
            }
          }}
        >
          {picker}
          <div className="connect-layout-actions">
            <button type="button" onClick={onUndo} disabled={!canUndo} title={t('panels.undo.title')}>
              {t('panels.undo')}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

/** PER-SURFACE: whether this window's view draws the clock-and-indices bar across its top (a layout
 *  writes it: Frame + bar on, every other layout and Reset layout off). '1' is on; anything else, off. */
const BAR_KEY = 'nexus.connect.bar'

// THE ONE-TIME SWITCH TO THE DEFAULT (step 5, the operator's "Everyone, once", 2026-10-01: "All users
// switch to Frame + bar on update, with Standard one click away. This overrides layouts people chose.")
// With the condition the operator was told: what each surface had is KEPT, so one tap brings back
// exactly that, not just Standard, and the switch runs once per surface, never again.
//
// ONE RECORD PER SURFACE, `nexus.connect.switch.<surface>`: present once the switch has run there, and
// holding the arrangement it replaced when that was the operator's own (the screen read Custom). When it
// was Standard or a layout, that choice in the picker is already the way back, so nothing is kept. The
// MAIN window's record is durable (features/durableStore): it rides in the profile's ui-state.json, and
// so in a backup, so neither a reinstall nor a restore runs the switch a second time, and the kept
// layout is as safe as the panel record it replaced. A pop-out's is per-surface chrome, the panel
// record's own split, which is why the key carries its surface itself instead of windowScope's bare key.
//
// It runs in the view's first render, before the records are read, so nothing paints the old layout
// first, and it writes what a tap on Frame + bar writes: the placement and its tabs, the panel record
// (the text sizes kept), the widths and the bar. Never the map. A fresh install starts in Frame + bar
// the same way, with nothing kept.
//
// A SURFACE THAT HAS NO LAYOUT OF ITS OWN is not switched: a dashboard window that never made a change
// reads the main window's records (windowScope inheritance), before the switch and after it, so it
// switches when the main window does and goes back when the main window goes back. Switching it on its
// own would override, a second time, a layout the operator may already have chosen to keep. It records
// that it has been past this point (so a later change of its own does not set the switch off), with the
// main window's earlier layout, which is what it showed too, for its own one tap back.
const SWITCH_V = 1
const switchKey = (surface: string) => `nexus.connect.switch.${surface}`

/** A surface's switch record: absent until the switch has run there. */
function switchRecord(surface: string): { kept?: unknown } | null {
  try {
    const raw = durableGet(switchKey(surface))
    return raw == null ? null : ((JSON.parse(raw) as { kept?: unknown }) ?? {})
  } catch {
    return {}
  }
}

/** Whether a pop-out surface has written any of the records a layout is made of (connectConfig's
 *  placement, panelState's record, connectRails' widths, the bar) — or still reads the main window's. */
function hasOwnLayout(surface: string): boolean {
  return (
    surfaceHasOwn('nexus.connect.config') ||
    surfaceHasOwn('nexus.connect.railWidths') ||
    surfaceHasOwn(BAR_KEY) ||
    durableGet(panelStorageKey(CONNECT_PANELS.view, surface)) != null
  )
}

function switchToDefaultOnce(presets: Readonly<Record<ConnectPresetId, ConnectLayout>>): void {
  const surface = surfaceId()
  const key = switchKey(surface)
  if (switchRecord(surface)) return
  if (surface !== 'main' && !hasOwnLayout(surface)) {
    durableSet(key, JSON.stringify({ v: SWITCH_V, kept: switchRecord('main')?.kept ?? null }))
    return
  }
  const cfg = loadConnectConfig()
  const panels = loadPanelLayout(CONNECT_PANELS, surface, 'main')
  const rails = loadRailWidths()
  const now = { slots: cfg.slots, tabs: cfg.tabs, panels, rails, bar: surfaceGet(BAR_KEY) === '1' }
  const reads = connectLayoutNow(now, presets)
  const def = presets.frameBar
  if (reads !== 'frameBar') {
    saveConnectConfig(normalizeConfig({ ...cfg, slots: def.slots, tabs: def.tabs, rotate: {} }))
    savePanelLayout(panelStorageKey(CONNECT_PANELS.view, surface), { ...layoutPanels(def), scale: panels.scale })
    saveRailWidths({ left: def.rails.left, right: def.rails.right })
    surfaceSet(BAR_KEY, def.bar ? '1' : '0')
  }
  durableSet(key, JSON.stringify({ v: SWITCH_V, kept: reads === 'custom' ? layoutOf(now, cfg.rotate) : null }))
}

/** The layout this surface's switch kept, each part coerced as the record it came from is on load;
 *  null when it kept none, and for anything unreadable. */
function loadKeptLayout(): ConnectLayout | null {
  try {
    const k = switchRecord(surfaceId())?.kept
    if (!k || typeof k !== 'object') return null
    const o = k as Record<string, unknown>
    const cfg = normalizeConfig({ slots: o.slots, tabs: o.tabs, rotate: o.rotate })
    const hidden = Array.isArray(o.hidden) ? SLOT_IDS.filter((s) => (o.hidden as unknown[]).includes(s)) : []
    const { share } = coercePanelLayout(CONNECT_PANELS, { v: 1, state: {}, share: o.share })
    const rails = parseRailWidths(JSON.stringify(o.rails ?? null))
    return {
      slots: cfg.slots,
      tabs: cfg.tabs,
      rotate: cfg.rotate,
      hidden,
      share,
      rails: { left: rails.left, right: rails.right },
      bar: o.bar === true,
    }
  } catch {
    return null
  }
}

/** Read a PER-SURFACE enum preference: a board's own preset/mode, not a station setting. */
function persisted<T extends string>(key: string, allow: readonly T[], fallback: T): T {
  const v = surfaceGet(key)
  if (v && (allow as readonly string[]).includes(v)) return v as T
  return fallback
}

interface Props {
  myGrid: string
  /** The station as the dashboard bar a layout puts over the view shows it — the main window's snapshot's
   *  call and grid, as the dashboard window's bar shows them. The Remote page reads the station's own;
   *  the hosts that draw their own bar need not pass it. */
  station?: { call: string; grid: string }
  theme: Theme
  stations: Station[]
  prop: PropagationSnapshot | null
  selectedCall: string | null
  onSelectCall: (call: string | null) => void
  needByCall: Map<string, NeedTag>
  /** Double-click-to-work a map spot/DXpedition (forwarded to MapView). */
  onWorkSpot?: (t: { call: string; band: string; mode: string | null; freqMhz: number | null }) => void
  /** The ranked needed-now alerts (App's shared 30 s poll) — reserved for the B2
   * best-band/needs cross-ref pane; passed through to the pane context. */
  needAlerts?: NeedAlert[]
  /** Point the rotator at a call (only passed when a rotator is configured). */
  onPoint?: (call: string) => void
  /** Click a map satellite → open it in the Satellites section (forwarded to MapView). */
  onSelectSat?: (name: string) => void
  /** The active radio's amplifier status, off App's existing 300 ms snapshot poll. Null when
   * none is configured (the Amplifier pane then renders nothing). Read-only; it stops nothing. */
  amp?: AmpStatus | null
  /** Open Connect in its own window (omit when already standalone). */
  onPopOut?: () => void
  /** The band the active radio is on, off App's existing snapshot poll: the band tiles ring it. */
  rigBand?: string | null
  /** The Spots box's list and wiring: this window's Spots board, lent whole (paneContext
   *  SpotsFeed). Omitted ⇒ the box shows its one-line state and offers no Work. */
  spotsFeed?: SpotsFeed
  /** The POTA/SOTA box's wiring: this window's POTA/SOTA board's (paneContext OtaBoard). Omitted ⇒
   *  its one-line state and no HUNT. */
  otaBoard?: OtaBoard
  /** AUTO-ROTATE a slot's tabs (⋯ ▸ Rotate the tabs), offered and run only where the host says so:
   *  the dashboard window (DetachedPanel's Connect) and the TV page (ConnectTv), never the main
   *  window's Connect (the operator's pick, 2026-09-29: "Auto-rotating boxes on the dashboard/TV").
   *  Without it a stored interval is inert and the menu offers none. */
  autoRotate?: boolean
  /** Settings ▸ Workspace's local clock (#253): the header's clock shows this computer's time beside
   *  UTC, as the top bar's did. */
  showLocalClock?: boolean
  /** The host draws the dashboard bar over this view (the dashboard window, DetachedPanel; the TV page,
   *  ConnectTv), so the header draws no clock of its own: the bar's big one is the clock there. */
  hostBar?: boolean
  /** This host's layout table, when it is not the app's: the TV page's (features/connectPresets
   *  TV_PRESETS) has its own Frame + bar, the default it opens in, without the boxes the page can never
   *  fill. Omitted ⇒ CONNECT_PRESETS. */
  presets?: Readonly<Record<ConnectPresetId, ConnectLayout>>
}

export function ConnectView({
  myGrid: nativeMyGrid,
  station: nativeStation,
  theme,
  stations,
  prop: nativeProp,
  selectedCall: nativeSelectedCall,
  onSelectCall: nativeOnSelectCall,
  onWorkSpot,
  needByCall,
  needAlerts,
  amp,
  onPoint,
  onSelectSat,
  onPopOut,
  rigBand,
  spotsFeed,
  otaBoard,
  autoRotate,
  showLocalClock,
  hostBar,
  presets,
}: Props) {
  const remoteConnect=useNavigation<ConnectData>('connect')
  const remoteSats=useNavigation<SatelliteData>('satellites')
  const remoteTrack=useSatelliteLive()
  const remote=remoteConnect.remote
  // A browser may point the rotator only while the station advertises it.
  const remoteRotator=useStationCapability('rotator')
  const [remoteSelection,setRemoteSelection]=useState<string|null>(null)
  const selectedCall=remote?remoteSelection:nativeSelectedCall
  const onSelectCall=remote?setRemoteSelection:nativeOnSelectCall
  const prop=remote?remoteConnect.value?.prop??null:nativeProp
  const myGrid=remote?remoteConnect.value?.mygrid??'':nativeMyGrid
  const station=remote?{call:remoteConnect.value?.mycall??'',grid:remoteConnect.value?.mygrid??''}:nativeStation??{call:'',grid:''}
  const [intent, setIntent] = useState<MapIntent>(() =>
    persisted('nexus.connect.intent', ['dx', 'pota', 'casual', 'vhf'] as const, 'dx'),
  )
  const pickIntent = (id: MapIntent) => {
    // PER-INTENT MAP PICK (features/intentMapSettings): every pick is stored as it is made, so the
    // intent being left already has its own; the next one comes back on its pick, or on Globe.
    setMapPick(loadIntentSetup(id)?.map ?? 'globe')
    setIntent(id)
    surfaceSet('nexus.connect.intent', id)
  }
  // THE MAP PICK — Globe (the 2-D orthographic globe, the default) · 3D (the WebGL globe) · Flat ·
  // Beam: ONE picker for what used to be a 2D/3D header toggle plus the 2-D map's own projection
  // buttons (operator decision 2026-09-13, late). PER-SURFACE and PER-INTENT: stored in the intent's
  // record, where the old shared projection and 3-D flag migrate on first load.
  //
  // 3D needs a GPU that can carry the textured, bloomed globe (gpuCapableForGlobe). Without one the
  // choice stays in the row, unavailable and saying why, and a 3D pick stored on this surface shows
  // Globe instead of a globe this machine cannot draw — without discarding the stored pick.
  const [gpuOk] = useState(gpuCapableForGlobe)
  const [mapPick, setMapPick] = useState<MapChoice>(() => loadIntentSetup(intent)?.map ?? 'globe')
  const map3d = mapPick === '3d' && gpuOk
  // THE STREET MAP (features/streetMaps.ts; operator rulings 2026-10-04, D5): a fifth choice, offered
  // only once the street map is (hidden until its maps are hosted) and never on the Remote page. It
  // draws the installed pack that holds the station, else the newest. A stored Street pick that cannot
  // draw here (no pack, no WebGL2, not offered) shows Flat and says why, without discarding the pick,
  // as 3D does; while the answer is still being read nothing is said.
  const streetMaps = useStreetMaps()
  const streetOffered = !remote && streetMaps.offered === true
  const streetPack = streetOffered ? packFor(streetMaps.packs, gridToLatLon(myGrid)) : null
  const streetCanDraw = streetOffered && streetMaps.webgl2 && streetPack != null
  const streetKnown = streetMaps.offered === false || (streetMaps.offered === true && streetMaps.packs !== null)
  const streetWhy =
    mapPick !== 'street' || streetCanDraw || !streetKnown
      ? null
      : !streetOffered
        ? t('map.street.standIn.hidden')
        : !streetMaps.webgl2
          ? t('map.street.standIn.noWebgl2')
          : t('map.street.standIn.noPack')
  const [sheetOpen, setSheetOpen] = useState(false)
  // Where the 2-D map's centre was when it last drew (MapView `centreRef`), for the sheet.
  const mapCentreRef = useRef<LatLon | null>(null)
  const shownPick: MapChoice =
    mapPick === '3d' && !gpuOk ? 'globe' : mapPick === 'street' && !streetCanDraw ? 'world' : mapPick
  const chooseMap = (choice: MapChoice) => {
    if (choice === '3d' && !gpuOk) return
    if (choice === 'street') {
      if (!streetOffered || !streetMaps.webgl2) return
      // Without a pack, the press asks for one: the sheet, never a map that cannot draw.
      if (!streetPack) {
        setSheetOpen(true)
        return
      }
    }
    setMapPick(choice)
    saveIntentSetup(intent, { map: choice })
  }
  // FULL-SCREEN MAP (operator request): the map fills the window and everything framing it
  // goes — this header, the four rail panes, the bottom strip, and the map's own Layers
  // panel. MapView owns the state (it owns the button, the Escape key and the per-surface
  // record); this mirror exists only so the frame can get out of the way, and it is reported
  // on mount so a surface reopens the way it was left.
  //
  // `&& !map3d` is a structural guard, not a nicety: the button lives in MapView, so a
  // full-screen flag left standing while the 3-D globe is mounted would hide the header and the
  // panes with nothing on screen able to bring them back.
  const [mapFull, setMapFull] = useState(false)
  // THE ONE-TIME SWITCH TO FRAME + BAR (switchToDefaultOnce, above): here, in the first render, so the
  // placement, the panel record, the widths and the bar below are all read after it. Then the layout
  // it kept, if it kept one, for the picker's "Your earlier layout".
  const table = presets ?? CONNECT_PRESETS
  const [kept] = useState(() => {
    switchToDefaultOnce(table)
    return loadKeptLayout()
  })
  // Basic/Expert + the per-slot pane assignment (persisted; basic-default, remember-last).
  const { slots, tabs, rotate, assignPane, addTab, removeTab, showTab, setRotate, resetSlots, restoreSlots } = useConnectConfig()
  // Band focus (advisor/opening row click) — the map highlights that band's heat
  // + spots; click the same band again (or the clear chip) to release.
  const [focusBand, setFocusBand] = useState<string | null>(null)
  const toggleFocusBand = (band: string) => setFocusBand((f) => (f === band ? null : band))
  // NOTE: focus is a deliberate user action and STICKS until toggled — a modeled-open-
  // but-unheard band is a legitimate focus target; the map just doesn't dim when a
  // focused band has no spots (MapView), so focusing it can't black out the map.
  // WHAT THE BOXES READ: the selection resolved against everything plotted (a click on ANY map pixel
  // fills the selection box), the window's one poll of Connect's feeds, and the path outlook for the
  // selection — built by the code the dashboard rail beside the cockpits uses too
  // (connect/usePaneContext), so a box reads the same in both places and nothing is polled twice.
  // The hosted Remote page polls nothing: its copies come from the station's `connect` and `path`
  // collections (`remoteFeeds` is the mapping this view's own effect used to make).
  const selection = useMemo(() => resolveSelection(selectedCall, stations, prop), [selectedCall, stations, prop])
  const selGrid = selection.selGrid
  const remotePath=useNavigation<PathData>('path',selGrid??'',!!selGrid)
  const remoteFeedValues = useMemo(
    () => (remote ? remoteFeeds(remoteConnect.value, remoteConnect.ageMs) : null),
    [remote, remoteConnect.value, remoteConnect.ageMs],
  )
  const { ctx, xrayNow } = usePaneContext({
    myGrid,
    theme,
    intent,
    prop,
    needByCall,
    needAlerts,
    amp,
    // The Remote browser's copy carries no radio band of its own here.
    rigBand: remote ? null : (rigBand ?? null),
    selectedCall,
    selection,
    onSelectCall,
    onWorkSpot,
    onPoint: remote&&!remoteRotator?undefined:onPoint,
    focusBand,
    toggleFocusBand,
    remote: remoteFeedValues
      ? { feeds: remoteFeedValues, pathPred: remotePath.value?.mygrid===myGrid?remotePath.value.prediction:null }
      : null,
    spotsFeed,
    otaBoard,
  })
  const { pathPred, bandOutlook, muf } = ctx
  // The one flux value the map renders (dev-override > fast lane > snapshot).
  const xrayLong = effectiveXray(xrayNow, prop?.spaceWx.xrayLong)
  const chromeHidden = mapFull && !map3d

  // CLOSE + RESIZE (operator-approved 2026-09-13). Visibility is per SLOT, in the shared
  // panel record (features/panelState CONNECT_PANELS) — placement stays in the config above.
  // Scoped by SURFACE: `?panel=connect` carries no instance token, so the default key would
  // be the main window's own (and durable) record. The pop-out reads the main window's layout
  // until it makes a change of its own.
  const surface = useMemo(() => surfaceId(), [])
  const panels = usePanelLayout(CONNECT_PANELS, surface, 'main')
  // A POP-OUT KEEPS WHAT IT SHOWS (operator report, 2026-10-04: the dashboard window "all of a sudden
  // shows every satellite ever launched ... I never turned them on"). Until it writes a value of its
  // own, a pop-out reads the main window's (features/windowScope `surfaceGet`), and it read the intent,
  // that intent's map pick (the 2-D map takes its setup over when it mounts; on the 3-D globe nothing
  // did) and the ★/All choice afresh on every open. A reload or a reopen, with
  // nothing pressed in it, put it on whatever the main window had moved to since, and the layers of
  // that intent came with it. So the first time a pop-out shows Conditions, what it shows becomes its
  // own, and after that only a press here changes it. The main window reads only its own values, so it
  // writes nothing here.
  useEffect(() => {
    if (surface === 'main') return
    if (!surfaceHasOwn('nexus.connect.intent')) surfaceSet('nexus.connect.intent', intent)
    keepIntentSetup(intent, mapPick)
    keepSatFavOnly()
  }, [surface, intent, mapPick])
  // THE BAR (a layout's, features/connectPresets `bar`): this surface's record of whether the view draws
  // the dashboard bar over its header. Where the host draws the bar itself (`hostBar`) the record is
  // still kept, and read back, but the view draws no second bar.
  const [barOn, setBarOn] = useState(() => surfaceGet(BAR_KEY) === '1')
  const writeBar = (on: boolean) => {
    setBarOn(on)
    surfaceSet(BAR_KEY, on ? '1' : '0')
  }
  const barShown = barOn && !hostBar
  // RESET LAYOUT IS THE OUT-OF-BOX STATE (operator 2026-09-13): default pane in every slot, every
  // pane open, default widths and splits — Standard, the install's layout until step 5 made a first run
  // open in Frame + bar (switchToDefaultOnce); the picker's Frame + bar (default) is the way to that one.
  // The panel record's one-level Undo already covers the
  // visibility + split half of a Reset; the slot placement lives in the config, so the placement
  // Reset replaced is held here and put back by the SAME Undo press. It is dropped by the next
  // change of any kind, so Undo only ever reverts the last change. Widths are not undo steps
  // (operator ruling), so an Undo after Reset leaves them at their defaults.
  //
  // A LAYOUT PRESET (⊞ Layout, below) is held the same way and ALSO keeps the widths it replaced.
  // A preset sets the whole board in one tap and its widths are most of what it changes, so an
  // Undo that left them would not put the operator's arrangement back — and a saved arrangement
  // is never overwritten silently. A width drag is still no undo step of its own.
  // …and the map layers a layout turned on (Frame: the satellites), exactly those and on exactly the
  // map whose record the tap changed, so its Undo turns off what the tap turned on and nothing the
  // operator already had on.
  const beforeSwitch = useRef<{
    slots: typeof slots
    tabs: typeof tabs
    rotate: typeof rotate
    rails?: RailWidths
    mapLayers?: { intent: MapIntent; turnedOn: Array<{ layer: PresetMapLayer; map: '2d' | '3d' }> }
    bar?: boolean
  } | null>(null)
  // A LAYOUT'S REACH INTO THE MAP (features/connectPresets `mapLayers`): the layers it turns on are
  // written into the record of the map on screen on this surface — the 2-D map's for the intent in
  // use, or the 3-D globe's — and this revision tells that map to read its layers again.
  const [mapLayersRev, setMapLayersRev] = useState(0)
  const change = <A extends unknown[]>(fn: (...a: A) => void) => (...a: A) => {
    beforeSwitch.current = null
    fn(...a)
  }
  const shown = (s: SlotId) => panels.stateOf(s) !== 'removed'
  const leftSlots = (['left1', 'left2'] as const).filter(shown)
  const rightSlots = (['right1', 'right2'] as const).filter(shown)
  const stripSlots = (['bottom1', 'bottom2', 'bottom3'] as const).filter(shown)
  // A rail renders only with a pane in it; the grid template follows what renders, so a
  // closed rail hands its width to the map instead of leaving an empty track.
  const present = {
    left: !chromeHidden && leftSlots.length > 0,
    right: !chromeHidden && rightSlots.length > 0,
  }
  const railsState = present.left ? (present.right ? 'both' : 'left') : present.right ? 'right' : 'none'
  const gridRef = useRef<HTMLDivElement>(null)
  const widths = useRailWidths(gridRef, present)
  // THE BOTTOM STRIP'S HEIGHT (layout L7): the divider above the strip owns it (a PaneSeam strip,
  // stored per surface in `nexus.split.connect.strip`, a % of the grid). Reset layout is the
  // out-of-box state, so it clears the height too, and remounts the divider so it reads the stock
  // height back: `stripEpoch` is its key.
  const stripRef = useRef<HTMLDivElement>(null)
  const [stripEpoch, setStripEpoch] = useState(0)
  const gridStyle = {
    ...(widths.applied.left != null ? { '--cn-rail-l': `${widths.applied.left}px` } : {}),
    ...(widths.applied.right != null ? { '--cn-rail-r': `${widths.applied.right}px` } : {}),
  } as React.CSSProperties

  // LAYOUT PRESETS (features/connectPresets): Map first · List first · Dashboard · Frame · Frame + bar,
  // and Standard and the kept layout, which a tap applies the same way.
  // Which one is on screen is READ BACK from the placement (its tabs included), the panel record, the
  // stored rail widths and the bar's record — never stored — so a pane moved or resized after a pick
  // reads Custom and nothing can snap back.
  const layoutNow = connectLayoutNow({ slots, tabs, panels: panels.layout, rails: widths.pref, bar: barOn }, table, kept)
  // Only ever an explicit tap. One undoable step: the panel record takes the visibility and the
  // splits in one write, and the placement + widths it replaced are held for the same Undo.
  const pickLayout = (id: LayoutChoice) => {
    if (layoutNow === id) return // already on screen: a tap must not spend the one Undo on nothing
    // Standard is a layout here like the others: no closed slots, the default widths, no tabs, no bar,
    // no map layers. Only Reset layout also puts the text sizes and the bottom row's height back. The
    // kept layout is one too, with its own splits and its tabs' rotation.
    const p = id === 'standard' ? STANDARD_LAYOUT : id === 'kept' ? kept : table[id]
    if (!p) return
    // On the map on screen only (2026-10-04). Ticked into the other map's record too, a layer waited
    // there unseen: unticked on the map in view, it came back the day the other map was shown.
    const turnedOn: Array<{ layer: PresetMapLayer; map: '2d' | '3d' }> = []
    for (const layer of p.mapLayers ?? []) {
      if (map3d ? setGlobeLayer(layer, true) : setIntentMapLayer(intent, layer, true))
        turnedOn.push({ layer, map: map3d ? '3d' : '2d' })
    }
    beforeSwitch.current = { slots, tabs, rotate, rails: widths.pref, mapLayers: turnedOn.length ? { intent, turnedOn } : undefined, bar: barOn }
    // The panes' own text sizes (⋯ ▸ A− / A+) ride through a layout: a layout decides where the panes
    // go and how much room each gets, never how big their words are — the rule it already keeps for
    // the map's own settings. Reset layout is what puts every pane back at the app's size.
    panels.setLayout({ ...layoutPanels(p), scale: panels.layout.scale })
    // Its tab plan, or one pane per slot (Frame + bar is the one preset with tabs; the kept layout has
    // the operator's own, and their rotation).
    restoreSlots(
      p.slots,
      p.tabs ? Object.fromEntries(Object.entries(p.tabs).map(([s, l]) => [s, [...l]])) : undefined,
      p.rotate ? { ...p.rotate } : undefined,
    )
    widths.setPrefs({ left: p.rails.left, right: p.rails.right })
    writeBar(!!p.bar)
    if (turnedOn.length) setMapLayersRev((n) => n + 1)
  }
  // The ONE Undo, behind both doors (⊞ Panels and the Layout button): the panel record steps back,
  // and the placement, widths and map layers a layout replaced come back with it.
  const undoLayout = () => {
    const before = beforeSwitch.current
    beforeSwitch.current = null
    panels.undo()
    if (before) {
      restoreSlots(before.slots, before.tabs, before.rotate)
      if (before.rails) widths.setPrefs(before.rails)
      if (before.bar !== undefined) writeBar(before.bar)
      if (before.mapLayers) {
        for (const { layer, map } of before.mapLayers.turnedOn) {
          if (map === '2d') setIntentMapLayer(before.mapLayers.intent, layer, false)
          else setGlobeLayer(layer, false)
        }
        setMapLayersRev((n) => n + 1)
      }
    }
  }
  const picker = <LayoutPicker now={layoutNow} onPick={pickLayout} kept={kept != null} />

  // TABS (features/connectConfig): a slot holds one or more panes and shows one. Showing a tab is not
  // an arrangement change, so it leaves the one Undo alone; adding or removing one is, like a pick.
  const frame = (s: SlotId, share?: number) => (
    <PaneFrame
      key={s}
      slotId={s}
      paneId={slots[s]}
      ctx={ctx}
      onAssign={change(assignPane)}
      share={share}
      onHide={change(() => panels.setPanelState(s, 'removed'))}
      textScale={panels.scaleOf(s)}
      onTextScale={change((f: number) => panels.setScale(s, f))}
      tabs={slotBoxes({ slots, tabs }, s)}
      onShowTab={(p: PaneId) => showTab(s, p)}
      addable={addableTo({ slots, tabs }, s)}
      onAddTab={change((p: PaneId) => addTab(s, p))}
      onRemoveTab={change(() => removeTab(s))}
      rotateSecs={autoRotate ? rotate[s] : undefined}
      onRotate={autoRotate ? change((secs: number | null) => setRotate(s, secs)) : undefined}
    />
  )
  const rail = (side: 'left' | 'right', ids: readonly SlotId[]) => {
    const [top, bottom] = ids
    const split = ids.length === 2
    const a = panels.shareOf(top)
    const b = split ? panels.shareOf(bottom) : 1
    const fraction = split ? a / (a + b) : 0.5
    return (
      <div className="connect-rail" data-side={side} style={{ '--connect-split': fraction } as React.CSSProperties}>
        {frame(top, a)}
        {split && frame(bottom, b)}
        {split && (
          <RailSplitHandle
            label={side === 'left' ? t('connect.rail.left.split') : t('connect.rail.right.split')}
            fraction={fraction}
            onCommit={change((above: number, below: number) => {
              const next: Partial<Record<SlotId, number>> = {}
              next[top] = above
              next[bottom] = below
              panels.setShares(next)
            })}
          />
        )}
        <RailWidthHandle
          side={side}
          label={side === 'left' ? t('connect.rail.left.width') : t('connect.rail.right.width')}
          api={widths}
          gridRef={gridRef}
        />
      </div>
    )
  }

  return (
    <NavigationMapContext.Provider value={remote?{connect:remoteConnect.value,satellites:remoteSats.value?.mygrid===myGrid?remoteSats.value.view:null,track:remoteTrack?.track??null,ageMs:remoteConnect.ageMs}:null}>
    <main className="layout single">
      <div className={`connect-shell${chromeHidden ? ' map-full' : ''}`}>
        {/* FRAME + BAR's bar (the operator's pick: "Yes, the full bar" — the dashboard window's, call
            and grid included) across the top of the view, over the header. It goes with the header in
            full screen. */}
        {!chromeHidden && barShown && <DashboardBar call={station.call} grid={station.grid} prop={prop} />}
        {!chromeHidden && (
        <div className="connect-header">
          {remote&&<span role="status" className="dim">{remoteConnect.value?t('remote.collectionObserver'):remoteConnect.loading?t('remote.collectionLoading'):t('remote.collectionUnavailable')}</span>}
          <div
            className="map-proj connect-intent"
            role="group"
            aria-label={t('connect.intent.aria')}
          >
            {INTENTS.map((it) => (
              <button
                key={it.id}
                className={intent === it.id ? 'active' : ''}
                onClick={() => pickIntent(it.id)}
                title={it.title}
              >
                {it.label}
              </button>
            ))}
          </div>
          {/* The Layout button and ⊞ Panels stand together, whatever else the header holds (the
              pop-out and the TV page have no Pop out to pack them against). */}
          <div className="connect-header-menus">
            <LayoutMenu picker={picker} onUndo={undoLayout} canUndo={panels.canUndo} />
            {/* The restore surface for a closed pane, and Reset layout. Always in the header, so
                with every pane closed the way back is still one click away. */}
            <PanelsMenu
              items={SLOT_IDS.map((s) => ({
                id: s,
                label: t('connect.panels.item', { title: paneById(slots[s])?.title ?? '', where: SLOT_WHERE[s]() }),
                state: panels.stateOf(s),
              }))}
              onToggle={change((id: string, show: boolean) => panels.setPanelState(id as SlotId, show ? 'docked' : 'removed'))}
              onUndo={undoLayout}
              canUndo={panels.canUndo}
              onReset={() => {
                beforeSwitch.current = { slots, tabs, rotate, bar: barOn }
                panels.reset()
                resetSlots()
                widths.resetAll()
                // The out-of-box state has no bar.
                writeBar(false)
                // '' reads back as "never set": the strip's own height (a height is not an undo step,
                // like the widths).
                surfaceSet('nexus.split.connect.strip', '')
                setStripEpoch((n) => n + 1)
              }}
              lead={picker}
            />
          </div>
          {onPopOut && !remote && (
            <button
              type="button"
              className="connect-popout"
              onClick={onPopOut}
              title={t('connect.popOut.title')}
            >
              {t('connect.popOut.label')}
            </button>
          )}
          {/* THE STATION CLOCK, in every layout (the operator, 2026-10-01). The top bar left Connect
              with its radio controls and took its UTC clock with it, and "things like time are very
              good" on a second monitor or the TV; the alerts, REC, the watchdog alert, Help and Field
              stay off. The top bar's own clock, last in the header. Where a dashboard bar is drawn over
              the view (Frame + bar's, or the host's), its big clock is the one. */}
          {!hostBar && !barShown && (
            <div className="connect-clock">
              <UtcClock />
              {showLocalClock && <UtcClock local />}
            </div>
          )}
        </div>
        )}
        {/* Children keep FIXED positions (a closed rail or strip is a `false` hole, never a
            shift), so opening or closing a rail can never remount the map beside it. */}
        <div className="connect" ref={gridRef} data-rails={railsState} style={gridStyle}>
          {present.left && rail('left', leftSlots)}
          <div className="connect-map">
            {/* THE MAP PICKER, once, above whichever renderer is mounted: the same node in every
                choice, so it can never vanish with the 2-D map when 3D mounts (MapPicker). */}
            <div className="connect-map-bar">
              <MapPicker
                choices={streetOffered ? STREET_MAP_CHOICES : ALL_MAP_CHOICES}
                value={shownPick}
                onPick={chooseMap}
                threeDUnavailable={!gpuOk}
                street={
                  streetOffered
                    ? {
                        installed: streetPack != null,
                        percent: streetMaps.download.state === 'running' ? streetMaps.download.percent : null,
                        unavailable: streetMaps.webgl2 ? null : t('map.street.noWebgl2'),
                      }
                    : undefined
                }
              />
              {streetWhy && (
                <span className="connect-map-note" role="status">
                  {streetWhy}
                </span>
              )}
              {/* A 3D PICK THIS MACHINE CANNOT DRAW RIGHT NOW is shown as Globe, and said so here for as
                  long as Globe stands in (2026-10-04). Whether 3D can draw is asked once, when this view
                  opens, so a reload or a reopen that finds the GPU fallen back to software (or a remote
                  desktop) opened on a different map with only the 3D button's tooltip to say why. */}
              {mapPick === '3d' && !gpuOk && (
                <span className="connect-map-note" role="status">
                  {t('connect.globe3d.standIn')}
                </span>
              )}
            </div>
            {map3d ? (
              <Suspense
                fallback={<div className="globe3d-loading">{t('connect.globe3d.loading')}</div>}
              >
                <Globe3D
                  myGrid={myGrid}
                  prop={prop}
                  selectedCall={selectedCall}
                  onSelectCall={onSelectCall}
                  outlook={selectedCall ? pathPred : bandOutlook}
                  onBandClick={toggleFocusBand}
                  activeBand={focusBand}
                  muf={muf}
                  xrayLong={xrayLong}
                  stations={stations}
                  layersRev={mapLayersRev}
                />
              </Suspense>
            ) : (
            <MapView
              myGrid={myGrid}
              theme={theme}
              stations={stations}
              prop={prop}
              selectedCall={selectedCall}
              onSelectCall={onSelectCall}
              needByCall={needByCall}
              intent={intent}
              projection={shownPick === '3d' ? 'globe' : shownPick}
              streetPack={shownPick === 'street' ? (streetPack ?? undefined) : undefined}
              centreRef={mapCentreRef}
              onWorkSpot={onWorkSpot}
              onSelectSat={onSelectSat}
              focusBand={focusBand}
              onFocusBand={toggleFocusBand}
              outlook={selectedCall ? pathPred : bandOutlook}
              muf={muf}
              xrayLong={xrayLong}
              onFullChange={setMapFull}
              layersRev={mapLayersRev}
            />
            )}
            {streetOffered && (
              <StreetDownloadSheet
                open={sheetOpen}
                onClose={() => setSheetOpen(false)}
                myGrid={myGrid}
                mapCentre={sheetOpen && !map3d ? mapCentreRef.current : null}
                onInstalled={() => {
                  // A download asked for from the picker shows the map it brought, as soon as it is in.
                  setSheetOpen(false)
                  setMapPick('street')
                  saveIntentSetup(intent, { map: 'street' })
                }}
              />
            )}
          </div>
          {present.right && rail('right', rightSlots)}
          {/* UNMOUNTED, not hidden — for full screen AND for a closed pane: the panes read
              everything from `ctx` (lifted here and still polling), so there is no state to
              keep warm — and a display:none pane would leave its ResizeObserver firing 0×0
              into a canvas that has to be re-stamped on re-show. Nothing to keep, nothing to
              re-stamp. An empty strip is not rendered at all: it would be a dead row. */}
          {!chromeHidden && stripSlots.length > 0 && (
            <div className="connect-strip" ref={stripRef}>
              {stripSlots.map((s) => frame(s))}
              {/* The map | strip divider (layout L7): in the gap above the strip, out of flow
                  (styles.css `.connect-strip > .pane-splitter`), so the strip's panes still split
                  its width. The strip comes after it on screen: moving it down shrinks the strip. */}
              <PaneSeam
                key={stripEpoch}
                axis="y"
                varName="--cn-strip-h"
                strip={stripRef}
                after
                storageKey="nexus.split.connect.strip"
                min={CONNECT_STRIP_SPLIT_MIN}
                max={CONNECT_STRIP_SPLIT_MAX}
                maxShare={CONNECT_STRIP_MAX_SHARE}
                defaultPct={null}
                label={t('connect.strip.height.label')}
              />
            </div>
          )}
        </div>
      </div>
    </main>
    </NavigationMapContext.Provider>
  )
}

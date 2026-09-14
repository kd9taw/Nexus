// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The intent chips'
// words come from the catalog through getters; the two named for a programme or a band keep
// their token label (INTENT_TOKENS below), and `3D`/`2D` are the two renderers' own names.
//
// Connect — the unified situational-awareness surface. The grayline map and the
// live propagation nowcast are TWO VIEWS OF ONE STATE: both read the same prop
// snapshot, operator grid, heard stations, need-state, and selection lifted in
// App. Selecting a station on the map highlights its great-circle path here; the
// surrounding panes answer "what's open, where to point, what do I need" at a glance.
// The panes are an assignable wrap-the-globe grid (HamClock-style): every panel is a
// reassignable pane with a Basic (one plain sentence) and Expert (full data) view; the
// globe stays the untouched centerpiece. See components/connect/* + features/connectConfig.
import { useState, useEffect, useMemo, useRef, lazy, Suspense } from 'react'
import type {
  GettingOut,
  MapSpot,
  NeedAlert,
  NeedTag,
  PathPrediction,
  PropagationSnapshot,
  Station,
  WorkableCard,
} from '../types'
import type { AlertView, AmpStatus, MufStation, NoaaScalesView } from '../types'
import type { Theme } from '../useTheme'
import { getPathOutlook, getBandOutlook, getGettingOut, getSpaceWxScales, getKc2gMuf, getXrayNow, getDxpedWindows } from '../api'
import type { DxpedWindow } from '../types'
import { effectiveXray } from '../flareAlert'
import { latLonToGrid } from '../grid'
import { gpuCapableForGlobe } from '../gpu'
import { MapView, type MapIntent } from './MapView'
// The 3-D WebGL globe is LAZY-loaded: three.js only downloads when an operator turns on
// 3-D mode, so the 2-D default (which runs anywhere) never pays for it.
const Globe3D = lazy(() => import('./Globe3D'))
import { provLabel } from './connect/paneFormat'
import { PaneFrame } from './connect/PaneFrame'
import { paneById } from './connect/panes'
import { RailSplitHandle, RailWidthHandle, useRailWidths } from './connect/RailHandles'
import { PanelsMenu } from './PanelsMenu'
import type { PaneContext } from './connect/paneContext'
import { SLOT_IDS, useConnectConfig, type SlotId } from '../features/connectConfig'
import { CONNECT_PANELS, usePanelLayout } from '../features/panelState'
import { surfaceGet, surfaceId, surfaceSet } from '../features/windowScope'
import { loadIntentSetup, saveIntentSetup, type MapChoice } from '../features/intentMapSettings'
import { MapPicker, ALL_MAP_CHOICES } from './MapPicker'
import { useEntityCentroids } from '../features/entityCentroids'
import { t } from '../i18n'
import { NavigationMapContext, useNavigation, useSatelliteLive } from '../remote-web/useNavigation'
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

/** Read a PER-SURFACE enum preference: a board's own preset/mode, not a station setting. */
function persisted<T extends string>(key: string, allow: readonly T[], fallback: T): T {
  const v = surfaceGet(key)
  if (v && (allow as readonly string[]).includes(v)) return v as T
  return fallback
}

interface Props {
  myGrid: string
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
}

export function ConnectView({
  myGrid: nativeMyGrid,
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
}: Props) {
  const remoteConnect=useNavigation<ConnectData>('connect')
  const remoteSats=useNavigation<SatelliteData>('satellites')
  const remoteTrack=useSatelliteLive()
  const remote=remoteConnect.remote
  const [remoteSelection,setRemoteSelection]=useState<string|null>(null)
  const selectedCall=remote?remoteSelection:nativeSelectedCall
  const onSelectCall=remote?setRemoteSelection:nativeOnSelectCall
  const prop=remote?remoteConnect.value?.prop??null:nativeProp
  const myGrid=remote?remoteConnect.value?.mygrid??'':nativeMyGrid
  const prov = prop ? provLabel(prop.source, prop.asOf) : null
  // Fetched here, once, and handed to every pane through the context — the panes that
  // need it include plain render functions that cannot hold a hook of their own.
  const entityCentroids = useEntityCentroids()
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
  const shownPick: MapChoice = mapPick === '3d' && !gpuOk ? 'globe' : mapPick
  const chooseMap = (choice: MapChoice) => {
    if (choice === '3d' && !gpuOk) return
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
  // Basic/Expert + the per-slot pane assignment (persisted; basic-default, remember-last).
  const { slots, assignPane, resetSlots, restoreSlots } = useConnectConfig()
  // Band focus (advisor/opening row click) — the map highlights that band's heat
  // + spots; click the same band again (or the clear chip) to release.
  const [focusBand, setFocusBand] = useState<string | null>(null)
  const toggleFocusBand = (band: string) => setFocusBand((f) => (f === band ? null : band))
  // NOTE: focus is a deliberate user action and STICKS until toggled — a modeled-open-
  // but-unheard band is a legitimate focus target; the map just doesn't dim when a
  // focused band has no spots (MapView), so focusing it can't black out the map.
  // Resolve the selection against EVERYTHING plotted: my decoded stations, the
  // live cluster/RBN/PSKR spots, and the DXpedition cards — so clicking ANY map
  // pixel populates the selection pane (the map's "so what").
  const selStation = useMemo(
    () => (selectedCall ? (stations.find((s) => s.call === selectedCall) ?? null) : null),
    [selectedCall, stations],
  )
  const selSpot = useMemo<MapSpot | null>(
    () =>
      selectedCall && !selStation
        ? (prop?.spots?.find((sp) => sp.call === selectedCall) ?? null)
        : null,
    [selectedCall, selStation, prop],
  )
  // Gated on !selStation: a DXpedition call we ALSO decoded locally renders as the
  // decoded station (worked from the cockpit) — the dxped card's advertised band may
  // differ from the band it was actually heard on, and the Work button must never
  // route the rig off what the operator is looking at.
  const selDxped = useMemo<WorkableCard | null>(
    () =>
      selectedCall && !selStation
        ? (prop?.dxpeditions.workableNow.find((c) => c.call === selectedCall) ?? null)
        : null,
    [selectedCall, selStation, prop],
  )
  // Per-path outlook for the selection (the PathPredictor seam): a station's
  // reported grid when we have one, else the spot's coordinates as a Maidenhead
  // square (centroid-placed spots = the entity's grid — approximate, labeled).
  const selGrid = useMemo(() => {
    if (!selectedCall) return null
    if (selStation?.grid) return selStation.grid
    if (selSpot) return latLonToGrid(selSpot.lat, selSpot.lon)
    return null
  }, [selectedCall, selStation, selSpot])
  const remotePath=useNavigation<PathData>('path',selGrid??'',!!selGrid)
  const [nativePathPred, setPathPred] = useState<PathPrediction | null>(null)
  const pathPred=remote?(remotePath.value?.mygrid===myGrid?remotePath.value.prediction:null):nativePathPred
  useEffect(() => {
    if(remote)return
    if (!selGrid) {
      setPathPred(null)
      return
    }
    let live = true
    getPathOutlook(selGrid)
      .then((p) => live && setPathPred(p))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [selGrid,remote])
  const pathOpen = pathPred?.bands.filter((b) => b.workability !== 'Closed') ?? []

  // The no-selection general "Band outlook (modelled)": modeled per-band workability
  // + MUF to a long-haul DX ring. Fetched only when no station is selected; refreshed
  // on the prop cadence so the modeled day tracks the current space weather.
  const [bandOutlook, setBandOutlook] = useState<PathPrediction | null>(null)
  // ⭐ KEPT WARM UNCONDITIONALLY, on its own cadence — like `getout` below.
  //
  // This used to early-return `if (selectedCall) return` and hang off `prop?.asOf`. Both
  // were written for the ONE consumer visible from here: the map/outlook strip, which
  // shows `pathPred` instead whenever a station is selected (:419/:441), so it genuinely
  // does not need this value then. But `bandOutlook` is also read UNCONDITIONALLY by three
  // panes — Chase (ChasePane.tsx:44), the Chase feed (ChaseFeedPane.tsx:25) and the
  // band-outlook heatmap (connect/panes.tsx:274/:569) — and for them the guard was
  // starvation: selecting a station froze their openness/"best window" column at whatever
  // it last held, indefinitely, while every sibling pane kept updating off its own poll.
  // That is the operator report ("the Chase section stays stuck on old information"), and it
  // was never pop-out-specific — the detached window only made it obvious, because it sits
  // on a second monitor for hours with a selection active.
  //
  // The `prop?.asOf` dep was the second half: `asOf` is stamped only on a real SWPC fetch
  // and served from a 300 s cache (PROP_TTL_SECS, src-tauri/src/lib.rs:1239), so even with
  // nothing selected this refreshed at most every five minutes rather than on any poll.
  // A plain interval is both simpler and honest about the cadence.
  useEffect(() => {
    if(remote)return
    let live = true
    const load = () =>
      getBandOutlook()
        .then((p) => live && setBandOutlook(p))
        .catch(() => {})
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [remote])
  const outlookOpen = bandOutlook?.bands.filter((b) => b.workability !== 'Closed') ?? []
  // "Am I getting out?" — who is hearing me now (observed). Polled on the prop
  // cadence; the backend reads the live PSK Reporter / RBN firehose each call.
  const [getout, setGetout] = useState<GettingOut | null>(null)
  useEffect(() => {
    if(remote)return
    let live = true
    const load = () =>
      getGettingOut()
        .then((g) => live && setGetout(g))
        .catch(() => {})
    load()
    const id = window.setInterval(load, 30_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [remote])
  // B3 live external feeds (desktop-only; cached server-side, polled on the TTL cadence).
  // Graceful: any failure leaves the last value, never throws — the panes degrade honestly.
  const [scales, setScales] = useState<NoaaScalesView | null>(null)
  const [alerts, setAlerts] = useState<AlertView[]>([])
  const [muf, setMuf] = useState<MufStation[]>([])
  useEffect(() => {
    if(remote)return
    let live = true
    const load = () => {
      getSpaceWxScales()
        .then((s) => {
          if (live) {
            setScales(s.scales)
            setAlerts(s.alerts)
          }
        })
        .catch(() => {})
      getKc2gMuf()
        .then((m) => live && setMuf(m))
        .catch(() => {})
    }
    load()
    // 5 min = the kc2g MUF cache TTL; the 15-min SWPC scales cache is intentionally
    // over-polled (harmless — the server serves cached, so it's a cheap freshness check).
    const id = window.setInterval(load, 300_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [remote])
  // X-ray fast lane (60 s) so the map's D-RAP flare layer moves at ~1 min cadence
  // during an event instead of the 5-min prop snapshot. Best-effort: a failed
  // fetch just leaves the snapshot's value driving the layer.
  const [xrayNow, setXrayNow] = useState<number | null>(null)
  useEffect(() => {
    if(remote)return
    let live = true
    const load = () =>
      getXrayNow()
        .then((x) => live && setXrayNow(x.flux))
        .catch(() => {})
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [remote])
  // The one flux value the map renders (dev-override > fast lane > snapshot).
  const xrayLong = effectiveXray(xrayNow, prop?.spaceWx.xrayLong)
  // DXpedition best-shot windows (server-cached climatology) — the selection
  // pane shows the selected expedition's line. 10-min poll is generous.
  const [dxpedWindows, setDxpedWindows] = useState<Map<string, DxpedWindow>>(new Map())
  useEffect(() => {
    if(remote)return
    let live = true
    const load = () =>
      getDxpedWindows()
        .then((list) => {
          if (live) setDxpedWindows(new Map(list.map((w) => [w.call.toUpperCase(), w])))
        })
        .catch(() => {})
    load()
    const id = window.setInterval(load, 600_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [remote])

  useEffect(()=>{
    if(!remote)return
    const d=remoteConnect.value,age=remoteConnect.ageMs
    setBandOutlook(d?.bandOutlook??null);setGetout(d?.gettingOut??null)
    const scales=d?.scales&&d.scales.ageMs+age<d.scales.validForMs?d.scales.value:null
    setScales(scales?.[0]??null);setAlerts(scales?.[1]??[])
    setMuf(d?.muf&&d.muf.ageMs+age<d.muf.validForMs?d.muf.value:[])
    setXrayNow(d?.xray&&d.prop.asOf+Math.floor((d.sourceAgeMs+age)/1000)-d.xray.asOf<120?d.xray.flux:null)
    setDxpedWindows(new Map())
  },[remote,remoteConnect.value,remoteConnect.ageMs])

  // One context handed to every pane (built from the already-lifted state above).
  const ctx: PaneContext = {
    myGrid,
    entityCentroids,
    theme,
    intent,
    prop,
    prov,
    needByCall,
    needAlerts: needAlerts ?? [],
    amp: amp ?? null,
    selectedCall,
    selStation,
    selSpot,
    selDxped,
    selDxpedWindow: selDxped ? (dxpedWindows.get(selDxped.call.toUpperCase()) ?? null) : null,
    dxpedWindows,
    selGrid,
    pathPred,
    bandOutlook,
    pathOpen,
    outlookOpen,
    getout,
    focusBand,
    scales,
    alerts,
    muf,
    onSelectCall,
    onWorkSpot: remote?undefined:onWorkSpot,
    onPoint: remote?undefined:onPoint,
    toggleFocusBand,
  }
  const chromeHidden = mapFull && !map3d

  // CLOSE + RESIZE (operator-approved 2026-09-13). Visibility is per SLOT, in the shared
  // panel record (features/panelState CONNECT_PANELS) — placement stays in the config above.
  // Scoped by SURFACE: `?panel=connect` carries no instance token, so the default key would
  // be the main window's own (and durable) record. The pop-out reads the main window's layout
  // until it makes a change of its own.
  const surface = useMemo(() => surfaceId(), [])
  const panels = usePanelLayout(CONNECT_PANELS, surface, 'main')
  // RESET LAYOUT IS THE OUT-OF-BOX STATE (operator 2026-09-13): default pane in every slot, every
  // pane open, default widths and splits. The panel record's one-level Undo already covers the
  // visibility + split half of a Reset; the slot placement lives in the config, so the placement
  // Reset replaced is held here and put back by the SAME Undo press. It is dropped by the next
  // change of any kind, so Undo only ever reverts the last change. Widths are not undo steps
  // (operator ruling), so an Undo after Reset leaves them at their defaults.
  const slotsBeforeReset = useRef<typeof slots | null>(null)
  const change = <A extends unknown[]>(fn: (...a: A) => void) => (...a: A) => {
    slotsBeforeReset.current = null
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
  const gridStyle = {
    ...(widths.applied.left != null ? { '--cn-rail-l': `${widths.applied.left}px` } : {}),
    ...(widths.applied.right != null ? { '--cn-rail-r': `${widths.applied.right}px` } : {}),
  } as React.CSSProperties

  const frame = (s: SlotId, share?: number) => (
    <PaneFrame
      key={s}
      slotId={s}
      paneId={slots[s]}
      ctx={ctx}
      onAssign={change(assignPane)}
      share={share}
      onHide={change(() => panels.setPanelState(s, 'removed'))}
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
          {/* The restore surface for a closed pane, and Reset layout. Always in the header, so
              with every pane closed the way back is still one click away. */}
          <PanelsMenu
            items={SLOT_IDS.map((s) => ({
              id: s,
              label: t('connect.panels.item', { title: paneById(slots[s])?.title ?? '', where: SLOT_WHERE[s]() }),
              state: panels.stateOf(s),
            }))}
            onToggle={change((id: string, show: boolean) => panels.setPanelState(id as SlotId, show ? 'docked' : 'removed'))}
            onUndo={() => {
              const before = slotsBeforeReset.current
              slotsBeforeReset.current = null
              panels.undo()
              if (before) restoreSlots(before)
            }}
            canUndo={panels.canUndo}
            onReset={() => {
              slotsBeforeReset.current = slots
              panels.reset()
              resetSlots()
              widths.resetAll()
            }}
          />
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
                choices={ALL_MAP_CHOICES}
                value={shownPick}
                onPick={chooseMap}
                threeDUnavailable={!gpuOk}
              />
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
              onWorkSpot={remote?undefined:onWorkSpot}
              onSelectSat={onSelectSat}
              focusBand={focusBand}
              onFocusBand={toggleFocusBand}
              outlook={selectedCall ? pathPred : bandOutlook}
              muf={muf}
              xrayLong={xrayLong}
              onFullChange={setMapFull}
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
            <div className="connect-strip">{stripSlots.map((s) => frame(s))}</div>
          )}
        </div>
      </div>
    </main>
    </NavigationMapContext.Provider>
  )
}

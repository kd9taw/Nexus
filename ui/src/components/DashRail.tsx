// THE DASHBOARD RAIL — a column of Connect boxes beside the operating cockpit (default: the Clock,
// Bands for you, Space Wx and Getting Out), so the time, the bands and who hears you are in view
// while you operate, without Connect or a second window. The operator's picks and the records behind
// them are in features/dashRail.
//
// WHERE IT STANDS. A sibling of the cockpits inside App's `.shell`, after them — never inside a cockpit
// shell, so it adds no child to one and no id to any cockpit's ⊞ vocabulary. It renders NO transmit
// control: its boxes are Connect's (connect/panes.tsx: ▶ Work moves the rig and opens a cockpit; it
// keys nothing), and every cockpit's stop controls stay in that cockpit's header and dock. Its track is
// one flat rule in cockpit-panes.css (`.dash-rail`); its width is a stored preference fitted into the
// window on load and on every resize, and never so wide that the cockpit beside it is narrower than on
// a 1024×768 screen (features/dashRail). Below `lg` App does not render it at all: beside FT, Phone, CW and
// JS8 its boxes then stand at the foot of the cockpit's columns (panes/CockpitBox `foldedRailBoxes`).
//
// WHAT IT IS MADE OF — nothing a second system: Connect's `PaneFrame`s and registry (every box is
// pickable in every slot), Connect's pane context built by the same code (connect/usePaneContext, the
// window's one poll of the feeds — the rail adds no request Connect or App already makes), the ⊞ Panels
// menu for its own four slots, and `PaneSeam` for the width and for the split between two boxes. A
// box's ⋯ menu offers its own text size and its manual link, as on Connect; tabs and their rotation
// are Connect's alone (the dashboard window and the TV page), so the rail offers neither.
//
// ITS RECORDS ARE APP'S, ONE PER COCKPIT (the operator's "Per cockpit", 2026-10-07): which box is in each
// slot, and which slots show with their splits and text sizes, are the cockpit's own once the operator changes
// them beside it; until then it reads the rail every cockpit shared before (features/dashRail). App owns them
// (`useDashRail`) beside the cockpits' own records and hands the rail the one beside the cockpit on screen.
//
// A CRASH IN A BOX COSTS THE RAIL, NOT THE COCKPIT: the boundary is inside the rail's own box, so the
// cockpit keeps its width and its controls, and the panel's way out turns the rail off.
//
// ⛔ A CLICK IN THE RAIL NEVER CHANGES THE STATION THE COCKPIT IS WORKING. On Connect a click on a
// station selects it app-wide (`select_peer`), and the selected station is the one a CW macro's `!`
// sends (engine `expand_cw` reads `active_peer` as the worked call). Beside a cockpit, one stray click
// on a Getting Out or Chase row mid-QSO would re-target the next macro to a different callsign — a
// change to what the rig transmits, made from a display. So the rail's boxes keep a selection of
// their OWN: it fills the rail's Selection and Outlook boxes and nothing else. ▶ Work stays — an
// explicit "work this station", the path the boards and the Pounce banner already offer beside a
// cockpit (it moves the rig and opens a cockpit; it keys nothing). The Spots, POTA/SOTA and Needed
// boxes are the boards themselves, and a row click on the Spots or the Needed board is a select AND a
// Work: the select is the rail's own here too, and the Work is the board's (the cockpit it opens then
// arms the station it was handed, as after a Work from any board).
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import { Fragment, createRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties, RefObject } from 'react'
import type { AmpStatus, NeedAlert, NeedTag, PropagationSnapshot, Station } from '../types'
import type { Theme } from '../useTheme'
import type { MapIntent } from './MapView'
import { t } from '../i18n'
import { PaneFrame } from './connect/PaneFrame'
import { paneById } from './connect/panes'
import { resolveSelection, usePaneContext } from './connect/usePaneContext'
import type { NeededBoard, OtaBoard, PaneContext, SpotsFeed } from './connect/paneContext'
import { PanelsMenu } from './PanelsMenu'
import { PaneSeam } from './PaneSeam'
import { ErrorBoundary } from './ErrorBoundary'
import { DASH_PANELS, MIN_SHARE, usePanelLayout } from '../features/panelState'
import {
  DASH_SLOT_IDS,
  dashRailInstance,
  fitRailWidth,
  loadRailWidth,
  railWidthMax,
  saveRailWidth,
  stepRailWidth,
  useDashSlots,
  type DashRailSection,
  type DashSlotId,
  type DashSlotsApi,
} from '../features/dashRail'
import type { PaneId } from '../features/connectConfig'
import { RAIL_MIN } from '../features/connectRails'
import { surfaceGet, windowInstance } from '../features/windowScope'
import { effWidth } from '../usePaneWidths'

/** Where each slot sits, in words — a ⊞ entry names the box AND where it comes back. */
const SLOT_WHERE: Record<DashSlotId, () => string> = {
  rail1: () => t('dashRail.slot.where.rail1'),
  rail2: () => t('dashRail.slot.where.rail2'),
  rail3: () => t('dashRail.slot.where.rail3'),
  rail4: () => t('dashRail.slot.where.rail4'),
}

const INTENTS: readonly MapIntent[] = ['dx', 'pota', 'casual', 'vhf']

/** A cockpit's rail panel record: which slots are shown, their splits and their text sizes. */
export type DashRailPanels = ReturnType<typeof usePanelLayout<DashSlotId>>

/**
 * THE RAIL'S RECORDS, as App owns them (the operator's "Per cockpit", 2026-10-07: FT's rail can differ from
 * Phone's, and each starts from today's rail): the window's placement record, whose `slotsOf` is a cockpit's
 * own slots or the shared rail's, and a panel record per cockpit, each reading the window's shared one until
 * the cockpit has its own (features/dashRail `dashRailInstance`). Owned above the rail, by App, so the cockpit
 * beside it and the rail read one record for what is on screen. One record per cockpit, all mounted, as App
 * owns each cockpit's own panel record: a cockpit switch reads the next one, it never reloads one in place.
 */
export function useDashRail(): { slots: DashSlotsApi; panels: Record<DashRailSection, DashRailPanels> } {
  const slots = useDashSlots()
  const shared = windowInstance()
  const operate = usePanelLayout(DASH_PANELS, dashRailInstance('operate'), shared)
  const phone = usePanelLayout(DASH_PANELS, dashRailInstance('phone'), shared)
  const cw = usePanelLayout(DASH_PANELS, dashRailInstance('cw'), shared)
  const rtty = usePanelLayout(DASH_PANELS, dashRailInstance('rtty'), shared)
  const psk = usePanelLayout(DASH_PANELS, dashRailInstance('psk'), shared)
  const sstv = usePanelLayout(DASH_PANELS, dashRailInstance('sstv'), shared)
  const aprs = usePanelLayout(DASH_PANELS, dashRailInstance('aprs'), shared)
  const js8 = usePanelLayout(DASH_PANELS, dashRailInstance('js8'), shared)
  return { slots, panels: { operate, phone, cw, rtty, psk, sstv, aprs, js8 } }
}

/** One cockpit's rail, from its records: what the rail draws and every change it can make. */
export interface DashRailRecords {
  /** Which box each slot shows: the record's, or, on a screen where the cockpit shows a slot's board as a
   *  pane of its own, the one App gives that slot instead (features/dashRail `railOnScreen`). */
  slots: Record<DashSlotId, PaneId>
  /** Which box each slot holds in the record, where that differs from `slots` (what ⊞ Undo puts back after
   *  a Reset). Omitted ⇒ `slots`. */
  stored?: Record<DashSlotId, PaneId>
  /** The panes on screen beside the rail, in the cockpit, which every picker marks. */
  marked?: ReadonlySet<PaneId>
  /** A slot's pick: that box in that slot. */
  assignPane: (slot: DashSlotId, pane: PaneId) => void
  /** Every slot back to its stock box (⊞ Reset). */
  resetSlots: () => void
  /** A whole placement back (⊞ Undo after a Reset). */
  restoreSlots: (slots: Record<DashSlotId, PaneId>) => void
  /** The slots' visibility, splits and text sizes. */
  panels: DashRailPanels
}

/** A cockpit's rail records (`useDashRail`), for the rail beside it. */
export function dashRailRecords(
  rail: { slots: DashSlotsApi; panels: Record<DashRailSection, DashRailPanels> },
  section: DashRailSection,
): DashRailRecords {
  return {
    slots: rail.slots.slotsOf(section),
    assignPane: (slot, pane) => rail.slots.assignPane(section, slot, pane),
    resetSlots: () => rail.slots.resetSlots(section),
    restoreSlots: (slots) => rail.slots.restoreSlots(section, slots),
    panels: rail.panels[section],
  }
}

export interface DashRailProps {
  /** The section beside it: a crashed box is retried when the operator moves to another. */
  section: string
  myGrid: string
  theme: Theme
  stations: Station[]
  prop: PropagationSnapshot | null
  needByCall: Map<string, NeedTag>
  needAlerts?: NeedAlert[]
  amp?: AmpStatus | null
  rigBand?: string | null
  onWorkSpot?: PaneContext['onWorkSpot']
  onPoint?: (call: string) => void
  /** The window's Spots board and its feed, the POTA/SOTA board's hunt wiring and the Needed board, lent
   *  to the rail's Spots, POTA/SOTA and Needed boxes as Connect's are (connect/paneContext). Absent ⇒
   *  their one line. */
  spotsFeed?: SpotsFeed
  otaBoard?: OtaBoard
  neededBoard?: NeededBoard
  /** The call in the log entry of the cockpit beside the rail (PaneContext `entryCall`), for the Rotor
   *  box: App reads it off the cockpit on screen. */
  entryCall?: string | null
  /** The rail beside this cockpit: its own records (`dashRailRecords`), which App owns. */
  rail: DashRailRecords
  /** Turn the rail off for this section: its ✕, and the crash panel's way out. */
  onHide: () => void
  /** The UI scale, so a zoom change re-fits the width (usePaneWidths' reason). */
  scale?: number
}

/** The rail's width: the stored preference fitted into this window before first paint, on every
 *  resize and on every zoom change, never written back by a fit (a bigger window gets it back). */
function useRailWidth(scale: number | undefined, rail: RefObject<HTMLElement | null>) {
  const pref = useRef<number | null | undefined>(undefined)
  if (pref.current === undefined) pref.current = loadRailWidth()
  const [view, setView] = useState(() => {
    const ew = effWidth()
    return { ew, px: fitRailWidth(pref.current ?? null, ew) }
  })
  useEffect(() => {
    let raf = 0
    const apply = () => {
      const ew = effWidth()
      const px = fitRailWidth(pref.current ?? null, ew)
      setView((v) => (v.ew === ew && v.px === px ? v : { ew, px }))
    }
    const onResize = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(apply)
    }
    // Deferred a frame, as useViewport is: a just-changed --ui-zoom is on <html> by then.
    raf = requestAnimationFrame(apply)
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('resize', onResize)
      cancelAnimationFrame(raf)
    }
  }, [scale])
  const commit = useCallback((px: number) => {
    const ew = effWidth()
    const w = stepRailWidth(px, ew)
    pref.current = w
    saveRailWidth(w)
    setView({ ew, px: w })
  }, [])
  const reset = useCallback(() => {
    pref.current = null
    saveRailWidth(null)
    const ew = effWidth()
    setView({ ew, px: fitRailWidth(null, ew) })
  }, [])
  // A drag paints the variable on the rail itself, with no render per move; the release commits.
  const paint = useCallback((px: number) => rail.current?.style.setProperty('--dash-rail-w', `${px}px`), [rail])
  return { px: view.px, max: railWidthMax(view.ew), commit, reset, paint }
}

export function DashRail(props: DashRailProps) {
  const railRef = useRef<HTMLElement>(null)
  const width = useRailWidth(props.scale, railRef)
  return (
    <aside
      ref={railRef}
      className="dash-rail"
      aria-label={t('dashRail.name')}
      style={{ '--dash-rail-w': `${width.px}px` } as CSSProperties}
    >
      {/* The rail's LEFT edge: moving it left widens the rail (grows −1). */}
      <PaneSeam
        axis="x"
        className="dash-rail-seam"
        label={t('dashRail.width.label')}
        value={width.px}
        min={RAIL_MIN}
        max={width.max}
        grows={-1}
        onPaint={width.paint}
        onCommit={width.commit}
        onReset={width.reset}
      />
      <ErrorBoundary
        label={t('dashRail.crash.label')}
        resetKey={props.section}
        action={{ label: t('dashRail.hide.label'), onClick: props.onHide }}
      >
        <DashRailBody {...props} />
      </ErrorBoundary>
    </aside>
  )
}

/** Clamp a stored share into the range the panel record keeps on load (coercePanelLayout), so what
 *  a divider stores is what a reload shows. */
const keepShare = (v: number) => Math.min(2 - MIN_SHARE, Math.max(MIN_SHARE, v))

function DashRailBody(p: DashRailProps) {
  const { slots, assignPane, resetSlots, restoreSlots, panels } = p.rail
  // ⊞ Reset puts the stock boxes back as well as the stock visibility; the placement it replaced is
  // held for the SAME Undo press (Connect's rule), and dropped by the next change of any kind.
  const beforeReset = useRef<Record<DashSlotId, PaneId> | null>(null)
  const change = <A extends unknown[]>(fn: (...a: A) => void) => (...a: A) => {
    beforeReset.current = null
    fn(...a)
  }
  // The band tiles' click focuses a band, as it does on Connect's map; here it is the box's own
  // highlight and nothing else (no rig command, like everywhere).
  const [focusBand, setFocusBand] = useState<string | null>(null)
  const toggleFocusBand = useCallback((band: string) => setFocusBand((f) => (f === band ? null : band)), [])
  // The rail's OWN selection (see the header): never the app's, whose station a CW macro sends.
  const [selectedCall, setSelectedCall] = useState<string | null>(null)
  // Connect's goal chips live on Connect; the rail reads the one this window last picked there.
  const [intent] = useState<MapIntent>(() => {
    const v = surfaceGet('nexus.connect.intent')
    return v != null && (INTENTS as readonly string[]).includes(v) ? (v as MapIntent) : 'dx'
  })
  const selection = useMemo(
    () => resolveSelection(selectedCall, p.stations, p.prop),
    [selectedCall, p.stations, p.prop],
  )
  // The Spots and Needed boards with the rail's selection in place of the app's (see the header):
  // their Work is the board's own.
  const spotsFeed = p.spotsFeed && {
    rows: p.spotsFeed.rows,
    board: { ...p.spotsFeed.board, selectedCall, onSelect: setSelectedCall },
  }
  const neededBoard = p.neededBoard && { ...p.neededBoard, selectedCall, onSelect: setSelectedCall }
  const { ctx } = usePaneContext({
    myGrid: p.myGrid,
    theme: p.theme,
    intent,
    prop: p.prop,
    needByCall: p.needByCall,
    needAlerts: p.needAlerts,
    amp: p.amp,
    rigBand: p.rigBand ?? null,
    selectedCall,
    selection,
    onSelectCall: setSelectedCall,
    onWorkSpot: p.onWorkSpot,
    onPoint: p.onPoint,
    focusBand,
    toggleFocusBand,
    remote: null,
    spotsFeed,
    otaBoard: p.otaBoard,
    neededBoard,
    entryCall: p.entryCall,
  })
  const frames = useMemo(
    () => Object.fromEntries(DASH_SLOT_IDS.map((s) => [s, createRef<HTMLElement>()])) as Record<DashSlotId, RefObject<HTMLElement>>,
    [],
  )
  const shown = DASH_SLOT_IDS.filter((s) => panels.stateOf(s) !== 'removed')
  // What every picker marks as on screen: the cockpit's (App's reading) and the rail's own slots, so a pick
  // of one says it moves here (once per screen across the two; within the rail the two slots swap).
  const marked = new Set<PaneId>([...(p.rail.marked ?? []), ...shown.map((s) => slots[s])])
  return (
    <>
      {/* `dash-head` is the head's content hook for styles.css, which may not name the structural
          classes (the `dash-boxes` pattern below). The ⊞ and ✕ are one group, so where the head
          wraps they go under the title together. */}
      <header className="dash-rail-head dash-head">
        <span className="dash-rail-title">{t('dashRail.title')}</span>
        <div className="dash-rail-acts">
          {/* The restore surface for a closed box, and Reset: always in the rail's head, so with every
              box closed the way back is still one click away. */}
          <PanelsMenu
            items={DASH_SLOT_IDS.map((s) => ({
              id: s,
              label: t('connect.panels.item', { title: paneById(slots[s])?.title ?? '', where: SLOT_WHERE[s]() }),
              state: panels.stateOf(s),
            }))}
            onToggle={change((id: string, show: boolean) => panels.setPanelState(id as DashSlotId, show ? 'docked' : 'removed'))}
            onUndo={() => {
              const before = beforeReset.current
              beforeReset.current = null
              panels.undo()
              if (before) restoreSlots(before)
            }}
            canUndo={panels.canUndo}
            onReset={() => {
              beforeReset.current = p.rail.stored ?? slots
              panels.reset()
              resetSlots()
            }}
            // The rail's own menu: its ✕ is the off switch here, so it offers no "Dashboard rail" row.
            offersRail={false}
          />
          <button
            type="button"
            className="pane-close"
            onClick={p.onHide}
            aria-label={t('dashRail.hide.label')}
            title={t('dashRail.hide.title')}
          >
            ✕
          </button>
        </div>
      </header>
      <div className="dash-rail-col dash-boxes">
        {shown.map((s, i) => {
          const above = i > 0 ? shown[i - 1] : null
          // A divider moves only the boundary between its two boxes: it redistributes their CURRENT
          // total (PaneSeam `scale`), so the boxes it does not touch stay exactly where they are.
          const scale = above ? (panels.shareOf(above) + panels.shareOf(s)) / 2 : 1
          return (
            <Fragment key={s}>
              {above && (
                <PaneSeam
                  above={frames[above]}
                  below={frames[s]}
                  varName="--connect-share"
                  className="in-column"
                  scale={scale}
                  label={t('dashRail.split.label', {
                    above: paneById(slots[above])?.title ?? '',
                    below: paneById(slots[s])?.title ?? '',
                  })}
                  onCommit={change((a: number, b: number) =>
                    panels.setShares({ [above]: keepShare(a * scale), [s]: keepShare(b * scale) }),
                  )}
                  onReset={change(() => panels.setShares({ [above]: null, [s]: null }))}
                />
              )}
              <PaneFrame
                slotId={s}
                slotName={SLOT_WHERE[s]()}
                paneId={slots[s]}
                ctx={ctx}
                onAssign={change(assignPane)}
                share={panels.shareOf(s)}
                onHide={change(() => panels.setPanelState(s, 'removed'))}
                frameRef={frames[s]}
                // A box's own text size (⋯ ▸ A− / A+), kept in the rail's own record as Connect keeps
                // its own: one Undo step, and the rail's Reset puts every box back at the app's size.
                textScale={panels.scaleOf(s)}
                onTextScale={change((f: number) => panels.setScale(s, f))}
                marked={marked}
              />
            </Fragment>
          )
        })}
      </div>
    </>
  )
}

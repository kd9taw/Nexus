// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the prose of the
// rig scope — the S-meter's three states, the feed chips, the Δ readout's explanation, the
// gain/zero knobs, the resolution/3D/pause/scroll buttons and the canvas — is in the catalog
// under `scope.*`. This is an INSTRUMENT, not a transmit surface: it draws, and its one
// gesture tunes the RX dial.
//
// The units rule lands on the PASSBAND: every S-unit and dB reading, the audio and RF spans, and
// the two feed names below are measurements and product names, so they stay in the code — as
// does the DIAL plate drawn on the overlay, which is an instrument mark placed by canvas
// arithmetic rather than a sentence. (The window widths moved to the ⚙ strip, ScaleStrip.tsx.)
//
// THREE LAYERS. The picture — the trace, the waterfall and the 3D stack — is the spectrum
// renderer's (`spectrum/`, WebGL2 or canvas-2D behind one contract), on its own canvas in
// `.ph-scope-render`. Everything the operator reads or aims with — the dial and carrier lines,
// their plates, the frequency scale and the pitch marker — is drawn on `.ph-scope-canvas` on top,
// which is also the element every pointer gesture lands on. The renderer swaps its canvas
// while a lost WebGL2 context is away, so nothing here ever holds or listens on that one.
//
// THE OVERLAYS (`spectrum/overlays.ts`) sit between the two, on `.ph-scope-overlays`: spot tags,
// the licence-class band edges, and on the RF scope pane the FT decodes and the RX/TX offsets.
// They are data-only layers, drawn again only when their data or the axis under them moves (the
// overlay key below), so a spot change redraws no waterfall. The band edges SHOW the transmit
// gate and never decide anything; a tag's click is the host's BandMap action, offered only where
// the scope tunes, and a grabbable filter edge keeps the press.
//
// THE RECEIVER MARKERS (`spectrum/markers.ts`). Where the host passes the rig's REPORTED width, the
// receiver's passband is drawn over the picture, and where it also passes `onPassband` the edges a
// width can move are grabbable: dragging one, or [ and ] on the focused scope, commands the filter
// through the host's coalescing. A press on an edge that does not move is still a click — the snap
// is untouched, as are the box drag, the edge scan and the wheel. Every gesture has a key on the
// focused scope (←/→ tune, Shift for bigger steps; Enter snaps onto the signal in the passband; [ ]
// the width; ↑/↓ scroll back while paused), and none of them is Esc or Space, which belong to the
// cockpit's stop and its PTT.
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { getRfFrame, getScopeFrame } from '../api'
import type { SpectrumFrameWire, SpotRow } from '../types'
import { useStationControl } from '../stationAccess'
import {
  applyGainZero,
  bakeLut,
  dbToSpan,
  isRfScopeSource,
  isSymmetricMode,
  normalize,
  resolveColormap,
  agcRange,
  RowFetchLatch,
  SCOPE_WINDOW_DB,
  WF_FLOOR_PCT,
  scopeView,
  sidebandSign,
  TRACE_HOLD_MS,
  traceHoldDecay,
  axisTicks,
  overlayTextScale,
} from '../waterfall'
import { boxEdges, boxWidthFor, clampBoxCenterHz, clickTuneTarget, dialFromBoxCenter } from '../tuneSnap'
import type { ScopeTuneRequest } from '../useScopeTune'
import {
  MARK_RGB,
  PASSBAND_LIMITS,
  clampPassband,
  drawNotch,
  drawPassband,
  edgeNear,
  edgesOnAxis,
  keyStepHz,
  notchOnAxis,
  passbandOnAxis,
  stepPassband,
  widthForEdge,
  type AxisKind,
  type AxisPassband,
  type Edge,
} from '../spectrum/markers'
import {
  axisToRf,
  drawOverlays,
  readOverlayInks,
  type FtOverlay,
  type OverlayHit,
  type OverlayInks,
  type OverlaySpans,
  type ScopeSpot,
} from '../spectrum/overlays'
import { usePrivilegeSpans } from '../spectrum/usePrivilegeSpans'
import { useWaterfallPalette } from '../waterfallPalette'
import { useNightActive } from '../useNight'
import { useSkinActive } from '../useSkin'
import { createSpectrumRenderer, type DisplayRange, type SpectrumFrame, type SpectrumRenderer } from '../spectrum'
import { axisOf, rendererFrame } from '../spectrum/scale'
import { LogRecursiveAverager } from '../spectrum/scaleAverage'
import { useScaleSettings } from '../spectrum/scaleSettings'
import { ScaleStrip } from '../spectrum/ScaleStrip'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
import { WheelRange } from './WheelRange'

/** The scope's own vocabulary: the unit on the Δ readout, the two native-panadapter feed
 *  names (a product and a protocol), and the plate drawn beside the carrier line. Tokens,
 *  named so the catalog guard reads them as the deliberate constants they are. */
const DB = 'dB'
const FLEX_RF = 'FLEX RF'
const CIV_RF = 'CI-V RF'
const DIAL_PLATE = 'DIAL'
/** The plate on a second receiver's dial line: the rig's own name for it. */
const SUB_PLATE = 'SUB'
/** How close (CSS px) a press must land to a filter edge to grab it, or to the Sub's line to be on it. */
const EDGE_TOL_PX = 5
/** How long a width just commanded stands on screen ahead of the rig's read-back (ms). */
const PENDING_WIDTH_MS = 2000
/** A pause between tuning keys after which the next press starts again from the live dial (ms). */
const KEY_IDLE_MS = 800
/** The outer band where a held drag scrolls the band (the edge scan), in CSS px for a scope
 *  `widthPx` wide. A filter edge is never grabbed inside it: that band is the scan's. */
const scanZonePx = (widthPx: number) => Math.min(36, widthPx / 4)
/** The focused scope's keys, for `aria-keyshortcuts` (key names, not prose). */
const SCOPE_KEYS = 'ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Enter [ ] ArrowUp ArrowDown'

/** Persisted 3D-view toggle for the rig scope (shared by the Phone + CW cockpits; per-surface
 * so a popped-out cockpit keeps its own choice). Static literal — the storage-scope test
 * classifies surfaceGet/Set keys by matching this declaration. */
const PHSCOPE_DSS_KEY = 'nexus.phonescope.dss'

/** Persisted scroll direction for the rig scope's waterfall band — same contract as the FT8
 * waterfall's FLOW_KEY (operator ask, 2026-08-16: the toggle everywhere a waterfall scrolls,
 * default DOWN to match). Only the exact string 'up' opts out; anything else is the default,
 * so a stale/foreign value can never pick a direction nobody chose. Static literal — the
 * storage-scope test classifies keys off this declaration. */
const PHSCOPE_FLOW_KEY = 'nexus.phonescope.flow'

/**
 * Persisted look of a SLOW scope: how the waterfall moves when the radio sweeps slower than this
 * scope polls (20 times a second). The operator asked for both, to compare on the air (2026-10-04,
 * "Build both, I'll compare"):
 *
 *  - `smooth` (the default) commits a row on every poll. Between two sweeps the newest one is
 *    committed again as a REPEAT, and each repeat is marked as one: it carries the number of the
 *    sweep it repeats and its own time, so the history says which rows are new data and which are
 *    copies, and the cadence probe (ui/spectrum-harness) can tell a repeat from a defect.
 *  - `sweep` commits one row per sweep and nothing else. The waterfall moves only when a new sweep
 *    arrives, so a 3-sweep-a-second scope scrolls slowly and in steps, and no row is ever a copy.
 *
 * The analysis window, averaging, detector, G and Z are the cockpit's scale record
 * (`spectrum/scaleSettings.ts`); this, like 3D and flow, is a look of THIS window's scope. Only the
 * exact string 'sweep' opts out of the default, so a stale or foreign value can never pick a look
 * nobody chose. Static literal — the storage-scope test classifies keys off this declaration.
 */
const PHSCOPE_ROWS_KEY = 'nexus.phonescope.rows'
type RowCadence = 'smooth' | 'sweep'

/** Rows of history kept for pause and scrollback: 51 s at 20 rows a second, as before. */
const HISTORY_ROWS = 1024

/** A frame drawn on an axis that runs against its row (LSB on the carrier-centred axis): bins
 *  reversed, span negated, so the frame says what it means on the axis it is drawn on. */
function mirrorFrame(f: SpectrumFrame): SpectrumFrame {
  const n = f.bins.length
  const bins = new Float64Array(n)
  for (let i = 0; i < n; i++) bins[i] = f.bins[n - 1 - i]
  return { ...f, loHz: -f.hiHz, hiHz: -f.loHz, bins }
}

interface Props {
  transmitting: boolean
  theme: string
  /** Pause the fetch/draw loop when the cockpit is hidden (kept for parity; Phone unmounts
   * on nav today so it's effectively always true). */
  active?: boolean
  /** Displayed audio window (Hz) within the captured 200–2900 row. Defaults = the
   * full voice passband; the CW cockpit narrows to an 800 Hz window around the pitch
   * (cwScopeWindow) so individual carriers are readable for tone placement. On a native
   * RF panadapter row the same window is mapped onto RF around the dial (scopeView), so
   * the width still applies. With `carrierCentered` the width is the OCCUPIED SIDEBAND's,
   * and the axis adds a W/8 guard band on the empty side. */
  viewLoHz?: number
  viewHiHz?: number
  /** PHONE only: draw the audio row on a rig-style axis — RF offset from the dial, with the
   * dial (audio 0 Hz, the suppressed carrier) at the 1/9 mark on USB, the 8/9 mark on LSB,
   * and the occupied sideband taking the other 3/4 of the panel. Off = the plain audio
   * window, which is what CW uses (its axis is centered on the PITCH instead — see
   * cwScopeWindow). Never applies to a native RF row; those are already dial-centered. */
  carrierCentered?: boolean
  /** Draw a hairline at this audio frequency (the CW pitch) — tune a signal onto
   * the marker and you're zero-beat. Omitted = no marker. */
  markerHz?: number | null
  /** CAT S-meter reading (dB relative to S9). When present, drives a calibrated
   * S-unit meter; `null`/absent = the rig doesn't report STRENGTH → meter shows "—". */
  smeterDb?: number | null
  /** Rig sideband/mode ("USB"/"LSB"/"FM"/"CW-L"…). Only matters for a native RF
   * panadapter row, where it sets which way the audio view window maps onto RF
   * (USB-side up from the dial, LSB-side down; FM/AM centered). See scopeView. */
  sideband?: string
  /** Live dial (absolute Hz). Only matters for a native RF panadapter row, where it
   * anchors the view window on the ACTUAL dial — the row center can sit off frequency
   * (Flex RETUNE_EPS lag, Icom fixed-edge sweeps). `null`/absent = unknown → scopeView
   * falls back to the row center. */
  dialHz?: number | null
  /** Reports the drawn window whenever it changes (feed source + absolute Hz span) so
   * the host cockpit can keep its scope label honest (RX audio vs real RF span). */
  onFeed?: (source: string, loHz: number, hiHz: number) => void
  /** Click/drag tuning (Flex-style). A single click snap-detects the signal under the
   * cursor and reports the dial that works it; press-and-hold on a native RF row shows
   * a passband-width box that live-tunes as it drags. The request carries the FINAL
   * dial (all mode math done here, where the spectrum row lives) — the host just
   * coalesces + commands CAT (useScopeTune). Absent = scope stays display-only. */
  onTune?: (t: ScopeTuneRequest) => void
  /** Optional click-only owner. Captured at press and used once at release;
   * movement cancels the click and cannot enter native drag/scan callbacks. */
  onBeginClick?: () => ((dialHz: number) => void) | null
  /** Effective RX filter width (Hz) for the drag box — the cockpit passes the rig's
   * read-back width or its per-mode fallback. */
  filterWidthHz?: number
  /** The rig's REPORTED passband width (Hz): the receiver marker's. null/absent = the rig does not
   *  report one, and then no passband is drawn and no edge can be grabbed — a marker drawn from
   *  the fallback above would place a filter the radio never said it had. */
  passbandHz?: number | null
  /** Commands a filter width (Hz) from a grabbed edge or the [ ] keys, already clamped to the
   *  cockpit's range; the host coalesces it (`useScopePassband`). Absent = the edges are drawn but
   *  cannot be moved. Never used by a click-only (Remote) scope. */
  onPassband?: (widthHz: number) => void
  /** The MANUAL notch's audio frequency (Hz), passed only while CAT reports the notch on: drawn as
   *  a band through the picture. Absent = no notch mark. */
  notchHz?: number | null
  /** A SECOND receiver's marker — the Sub of a dual-receiver radio: its dial, mode and reported
   *  width (null = unknown), drawn in its own colour on an RF row while its dial is in view, never
   *  grabbable, and DISPLAY ONLY: a click on it tunes nothing. ⚠️ PER HOST (dual-receiver ruling
   *  D9): Phone and CW pass it (operator ruling 2026-10-05, "Phone and CW scopes"); a host that
   *  does not draws what it always did. */
  subReceiver?: { dialHz: number; sideband: string; widthHz: number | null } | null
  /** CW sidetone pitch (Hz) for the click math (zero-beat targets). Distinct from
   * `markerHz`, which is only the audio-row visual hairline. Phone omits. */
  pitchHz?: number
  /** CW only: true (default) = the rig is in TRUE CW mode (CAT/WinKeyer), dial reads a
   * zero-beat signal's RF directly. False = soundcard keyer (CW carried through SSB),
   * where the dial must sit sign×pitch off the signal to hear it at the pitch. */
  cwPitchRefDial?: boolean
  /** Master gate for click/drag tuning: the cockpit passes catOk && !transmitting &&
   * dial-known. False → no pointer capture, no box, no cursor affordance. */
  interactive?: boolean
  /** Hide the thin S-meter strip in this header. Phone passes it because its analog meter
   *  reads S on the same screen and two readings of one number is worse than either. CW has no
   *  analog meter and keeps the strip — one host's change must not reach the other
   *  (`TxMeters`' four-row contract leaking into Operate is what that costs). */
  hideSmeter?: boolean
  /** Trace peak-hold time constant (ms) — how long a column's peak stands after the signal
   * stops. See TRACE_HOLD_MS: the cockpit picks it, because the right answer depends on the
   * SIGNAL, not on the operator. CW passes `fast` (48 ms dits have to be visible as keying);
   * phone takes the `normal` default (a hold short enough for CW flickers on every syllable). */
  traceHoldMs?: number
  /** The mouse wheel moves the G and Z sliders, one of their steps a notch (#384), rather than
   *  tuning the rig through them. Phone passes it; CW keeps its scope's wheel as it was. */
  wheelSliders?: boolean
  /** Whose scale record this scope draws with — its analysis window, averaging, detector, G and Z
   *  (`spectrum/scaleSettings.ts`). CW passes 'cw', whose averaging defaults to off; Phone takes
   *  the default; the RF scope pane passes 'rfpan', its own record in every digital cockpit. */
  cockpit?: 'phone' | 'cw' | 'rfpan'
  /** What this scope draws. 'scope' (the default) is the rig scope of Phone and CW: the radio's
   *  panadapter when one streams, else the audio FFT over the scope's window (`getScopeFrame`).
   *  'rf' is the RF scope pane of the digital cockpits: the radio's panadapter ONLY (`getRfFrame`),
   *  and while none streams it says so rather than drawing the audio FFT, which the cockpit's own
   *  waterfall already shows. Its poll is also the pane's request for the stream (see getRfFrame).
   *  Fixed for the scope's life: the loop reads it once. */
  feed?: 'scope' | 'rf'
  /** Spot tags (`spectrum/overlays.ts` `scopeSpots`): BandMap's set for this scope — its mode on its
   *  band, each with its mark — drawn above the trace with BandMap's fade and collision rules.
   *  Absent = no tags. */
  spots?: ScopeSpot[] | null
  /** Work the spot whose tag was clicked: the host's BandMap action (QSY to it and prefill the log).
   *  Offered only while the scope tunes (`interactive`), never on the click-only Remote scope, and a
   *  grabbable filter edge keeps the press. Absent = the tags are display only. */
  onSpot?: (s: SpotRow) => void
  /** The live operating section whose licence-class band edges are tinted ('phone' | 'cw' | 'digital'
   *  | 'rtty' | 'keyboard': the snapshot's `radio.operatingMode`), read from the transmit gate's own
   *  table. Display only. Absent = no edges. */
  privilegeMode?: string | null
  /** The FT cockpit's newest decodes and its RX/TX offsets, drawn at dial ± offset (the RF scope
   *  pane). Display only. Absent = none. */
  ft?: FtOverlay | null
}

/**
 * Real-time PHONE bandscope — a traditional rig display, distinct from the FT8 waterfall:
 * a fast COLORED panadapter trace (instantaneous spectrum, filled) on top, a FASTER waterfall
 * below, and a rapid colored S-meter. Polls the same RX spectrum (~30 Hz vs the FT8 8 Hz) with
 * a snappy AGC, so a voice op gets the live, fast, colored scope they expect. Reuses the shared
 * AGC/LUT/colormap helpers; never touches the FT8 Operate waterfall.
 */
/** Map a CAT S-meter reading (dB relative to S9) to a bar fraction + S-unit label.
 * S1..S9 span -48..0 dB (6 dB/unit); above S9 is shown as +dB (the classic red zone). */
function sMeterDisplay(db: number): { frac: number; label: string; zone: 'ok' | 'warn' | 'hot' } {
  const frac = Math.max(0, Math.min(1, (db + 54) / 114)) // S0 (-54 dB) .. S9+60
  let label: string
  let zone: 'ok' | 'warn' | 'hot'
  if (db >= 0) {
    // Over S9 — show the EXACT amount; never round up (that would overstate strength).
    const over = Math.round(db)
    label = over > 0 ? `S9+${over}` : 'S9'
    zone = 'hot'
  } else {
    const s = Math.max(0, Math.min(9, Math.round(9 + db / 6)))
    label = `S${s}`
    // Zone follows the displayed S-unit, so a given label always renders one color.
    zone = s >= 9 ? 'hot' : s >= 7 ? 'warn' : 'ok'
  }
  return { frac, label, zone }
}

export function PhoneScope({
  hideSmeter = false,
  transmitting,
  theme,
  active = true,
  viewLoHz = 0,
  viewHiHz = 4000,
  carrierCentered = false,
  markerHz = null,
  smeterDb = null,
  sideband = 'USB',
  dialHz = null,
  onFeed,
  onTune,
  onBeginClick,
  filterWidthHz,
  passbandHz = null,
  onPassband,
  notchHz = null,
  subReceiver = null,
  pitchHz = 600,
  cwPitchRefDial = true,
  interactive = false,
  traceHoldMs = TRACE_HOLD_MS.normal,
  wheelSliders = false,
  cockpit = 'phone',
  feed = 'scope',
  spots = null,
  onSpot,
  privilegeMode = null,
  ft = null,
}: Props) {
  const control = useStationControl()
  // The licence-class spans for the band edges: the gate's own table, for the live section.
  const privSpans = usePrivilegeSpans(privilegeMode, active)
  // The RF pane starts out saying the radio's scope is not there yet: it is only once a sweep lands.
  const [scopeAvailable, setScopeAvailable] = useState(control && feed !== 'rf')
  const rfOnly = feed === 'rf'
  // Master palette shared with the FT8 waterfall + all scopes ('auto' = theme-driven, and Amber
  // CRT at night — useNight.ts).
  const [palette] = useWaterfallPalette()
  const night = useNightActive()
  // …and on a built-in theme, Auto is the theme's own palette.
  const skin = useSkinActive()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const rafRef = useRef<number | null>(null)
  const txRef = useRef(transmitting)
  const themeRef = useRef(theme)
  const paletteRef = useRef(palette)
  const nightRef = useRef(night)
  const skinRef = useRef(skin)
  const activeRef = useRef(active)
  const viewLoRef = useRef(viewLoHz)
  const viewHiRef = useRef(viewHiHz)
  const carrierCenteredRef = useRef(carrierCentered)
  const markerRef = useRef(markerHz)
  const sidebandRef = useRef(sideband)
  const dialRef = useRef(dialHz)
  const onFeedRef = useRef(onFeed)
  const lutRef = useRef<Uint8ClampedArray>(bakeLut(resolveColormap(palette, theme, night, skin)))
  // The picture's host: the renderer puts its canvas here, under the overlay canvas. Retained
  // history lives in the renderer's ring (the DATA at each row's native resolution, with its own
  // span and range), so a palette, view, flow or size change redraws history rather than losing it.
  const renderHostRef = useRef<HTMLDivElement>(null)
  const rendererRef = useRef<SpectrumRenderer | null>(null)
  const rebuildRef = useRef<(() => void) | null>(null)

  // Pause + scrollback (review a moment you just missed) and the 3D stacked-spectrum view.
  const [paused, setPaused] = useState(false)
  const pausedRef = useRef(paused)
  pausedRef.current = paused
  const [dss, setDss] = useState<boolean>(() => surfaceGet(PHSCOPE_DSS_KEY) === '1')
  const dssRef = useRef(dss)
  dssRef.current = dss

  // The cockpit's scale record: the analysis window the row is computed with, the trace's
  // averaging, the detector, and the operator's G and Z (persisted per cockpit, clamped on load).
  const [scale, setScale] = useScaleSettings(cockpit)
  const scaleRef = useRef(scale)
  scaleRef.current = scale
  /** The ⚙ strip (window, averaging, detector, slow-scope look): shown on demand. */
  const [gear, setGear] = useState(false)
  const gearId = useId()
  const [rowCadence, setRowCadence] = useState<RowCadence>(() =>
    surfaceGet(PHSCOPE_ROWS_KEY) === 'sweep' ? 'sweep' : 'smooth',
  )
  const rowCadenceRef = useRef(rowCadence)
  rowCadenceRef.current = rowCadence
  /** Newest row at the TOP (scrolls down) — the default, matching the FT8 waterfall. */
  const [newestAtTop, setNewestAtTop] = useState<boolean>(() => surfaceGet(PHSCOPE_FLOW_KEY) !== 'up')
  const newestAtTopRef = useRef(newestAtTop)
  newestAtTopRef.current = newestAtTop
  /** Scrollback offset in history rows while paused (0 = live tail). */
  const offsetRef = useRef(0)
  // Which scope feed is live: '' / 'audio' = soundcard FFT, 'flex'/'civ' = a native RF panadapter.
  // Lifted out of the draw loop (updated only when it changes) so the badge can render it.
  const [source, setSource] = useState('')
  const sourceRef = useRef('')
  // The live Δ readout: the strongest signal's height above the noise floor.
  const [spanDb, setSpanDb] = useState<number | null>(null)
  const spanDbRef = useRef<number | null>(null)
  // Click/drag tuning state — the latest row + drawn view (captured each drawRow so the
  // pointer handlers can hit-test), the tune callback + math inputs, and the gesture.
  const onTuneRef = useRef(onTune)
  const onBeginClickRef = useRef(onBeginClick)
  const filterWidthRef = useRef(filterWidthHz)
  const passbandRef = useRef(passbandHz)
  const onPassbandRef = useRef(onPassband)
  const notchRef = useRef(notchHz)
  const subRef = useRef(subReceiver)
  const cockpitRef = useRef(cockpit)
  const onSpotRef = useRef(onSpot)
  /** The overlays' canvas: spot tags, band edges and the FT offsets, between the picture and the marks. */
  const overlaysRef = useRef<HTMLCanvasElement>(null)
  /** What the overlays draw from, and a count that moves whenever any of it does (in the overlay key). */
  const overlayDataRef = useRef<{ v: number; spots: ScopeSpot[]; spans: OverlaySpans | null; ft: FtOverlay | null }>({
    v: 0,
    spots: [],
    spans: null,
    ft: null,
  })
  /** Draw the overlays again if their key moved: their data changed between two sweeps, or while paused. */
  const overlaysSyncRef = useRef<(() => void) | null>(null)
  /** The spot labels as last drawn — the click targets — in the overlay canvas's device px. */
  const hitsRef = useRef<{ w: number; h: number; hits: OverlayHit[] }>({ w: 0, h: 0, hits: [] })
  /** The Sub's marker as last drawn, in the marks canvas's device px: its line's x, and its plate's
   *  right and bottom edges. null = not drawn (no Sub, not in view, not an RF row). */
  const subMarkRef = useRef<{ w: number; h: number; x: number; plateR: number; plateB: number } | null>(null)
  /** A width just commanded, shown until the rig's read-back agrees (or PENDING_WIDTH_MS passes):
   *  the flush and the radio loop's apply are both still ahead of it. */
  const pendingWidthRef = useRef<{ hz: number; until: number } | null>(null)
  /** The dial the tuning keys are walking, so a held key never re-reads a dial the flush has not
   *  moved yet; re-seeded from the live dial after KEY_IDLE_MS. */
  const keyDialRef = useRef<{ hz: number; at: number } | null>(null)
  /** Redraw only the overlay — a gesture or a mark's prop between two sweeps, or while paused. */
  const overlayRef = useRef<(() => void) | null>(null)
  const pitchRef = useRef(pitchHz)
  const cwPitchRefRef = useRef(cwPitchRefDial)
  const interactiveRef = useRef(interactive)
  const traceHoldRef = useRef(traceHoldMs)
  const lastRowRef = useRef<{ row: number[]; rowLo: number; rowHi: number } | null>(null)
  // `mirrored` = the drawn axis runs against the row (LSB on a carrier-centered axis), so
  // every pixel↔Hz conversion here negates: axis Hz is RF offset, row Hz is receiver audio.
  const lastViewRef = useRef<{ lo: number; hi: number; rf: boolean; mirrored: boolean } | null>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{
    click?: (dialHz: number) => void
    clickContext?: string
    pointerId: number
    x0: number
    y0: number
    rf: boolean
    moved: boolean
    dragging: boolean
    centerHz: number
    /** Latest cursor clientX — the box pins here during an edge-scan. */
    cursorX: number
    /** Optimistic dial during an edge-scan (advances rate×dt per tick); null = not scanning. */
    scanDialHz: number | null
    /** Audio-scope relative drag anchor: the view-Hz under the hand at grab… */
    grabAfHz: number
    /** …and the dial at grab (null = needs re-seeding, e.g. after an edge-scan). */
    grabDialHz: number | null
    /** A press on a grabbable FILTER EDGE: which edge, the passband and axis it was grabbed on,
     *  and the width (`hz`) the drag has reached. Moved, it resizes; released unmoved, it is a
     *  click. */
    edge?: { edge: Edge; p: AxisPassband; axis: AxisKind; hz: number }
    /** A press on a spot's tag (where no edge was grabbed): released unmoved, it works that spot. */
    spot?: SpotRow
    /** A press on the Sub's marker: released unmoved, it does nothing at all. */
    sub?: boolean
  } | null>(null)
  // Edge-scan while dragging: holding the box in the outer edge zone keeps scrolling the
  // band. The BOX stays pinned under the cursor (never repainted from Hz — the view is
  // dial-centered and recentering would spring it back to mid-screen); the DIAL advances
  // in small per-tick increments so the band scrolls smoothly instead of jumping half a
  // view per CAT flush. dir/depth set by pointermove; a rAF loop does the advancing.
  const scanRef = useRef<{ dir: -1 | 0 | 1; depth: number }>({ dir: 0, depth: 0 })
  const scanRafRef = useRef<number | null>(null)
  const scanTsRef = useRef(0)

  txRef.current = transmitting
  themeRef.current = theme
  paletteRef.current = palette
  nightRef.current = night
  skinRef.current = skin
  activeRef.current = active
  viewLoRef.current = viewLoHz
  viewHiRef.current = viewHiHz
  carrierCenteredRef.current = carrierCentered
  markerRef.current = markerHz
  sidebandRef.current = sideband
  dialRef.current = dialHz
  onFeedRef.current = onFeed
  onTuneRef.current = onTune
  onBeginClickRef.current = onBeginClick
  filterWidthRef.current = filterWidthHz
  passbandRef.current = passbandHz
  onPassbandRef.current = onPassband
  notchRef.current = notchHz
  subRef.current = subReceiver
  cockpitRef.current = cockpit
  onSpotRef.current = onSpot
  pitchRef.current = pitchHz
  cwPitchRefRef.current = cwPitchRefDial
  interactiveRef.current = interactive && (!onBeginClick || scopeAvailable)
  traceHoldRef.current = traceHoldMs

  // ---- The receiver marker's state, read by the overlay and the gestures alike (refs only) ----
  /** The width the marker shows: an edge being dragged, else a width just commanded (until the
   *  rig's read-back agrees, or it expires), else the rig's own. null = no width to show. */
  const shownWidth = (): number | null => {
    const g = dragRef.current
    if (g?.edge && g.dragging) return g.edge.hz
    const pend = pendingWidthRef.current
    const rig = passbandRef.current
    if (pend) {
      if (performance.now() < pend.until && pend.hz !== rig) return pend.hz
      pendingWidthRef.current = null
    }
    return rig != null && rig > 0 ? rig : null
  }
  /** The drawn axis, for the marks: the newest row's kind and the live props. */
  const axisKind = (rf: boolean): AxisKind => ({
    rf,
    carrierCentered: carrierCenteredRef.current && !rf,
    sideband: sidebandRef.current,
    dialHz: dialRef.current,
    pitchHz: pitchRef.current,
    cwPitchRefDial: cwPitchRefRef.current !== false,
  })
  /** May the marker's edges be grabbed now: the cockpit's tuning gate (CAT up, not transmitting),
   *  a host that commands widths, a width the rig reported, and not the click-only Remote scope. */
  const canEditPassband = () =>
    interactiveRef.current && onPassbandRef.current != null && onBeginClickRef.current == null && shownWidth() != null
  /** May a spot's tag be clicked now: the cockpit's tuning gate (CAT up, not transmitting), a host that
   *  works spots, and not the click-only Remote scope. */
  const spotsClickable = () => interactiveRef.current && onSpotRef.current != null && onBeginClickRef.current == null

  useLayoutEffect(() => {
    lutRef.current = bakeLut(resolveColormap(palette, theme, night, skin))
    rebuildRef.current?.() // recolor the accumulated waterfall history in the new palette
  }, [palette, theme, night, skin])

  // A new detector redraws the history through it, paused or not.
  useEffect(() => {
    rebuildRef.current?.()
  }, [scale.detector])

  // Unmount safety: a mid-drag nav away must not leave the edge-scan rAF running.
  useEffect(
    () => () => {
      if (scanRafRef.current != null) cancelAnimationFrame(scanRafRef.current)
    },
    [],
  )

  useEffect(() => {
    const canvas = canvasRef.current
    const host = renderHostRef.current
    if (!canvas || !host) return
    // The OVERLAY: write-only, cleared and redrawn on every draw over the renderer's picture. The
    // picture itself is the renderer's (see the file header), on its own canvas in `host`.
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    // The overlays' own canvas (`spectrum/overlays.ts`). Where it has no context the scope draws as it
    // always did, without tags or edges.
    const overlays = overlaysRef.current
    const octx = overlays?.getContext('2d') ?? null
    const renderer = createSpectrumRenderer(host, { depth: HISTORY_ROWS })
    rendererRef.current = renderer

    let running = true
    // Single-flight WITH a watchdog — see `RowFetchLatch`. The scope needs it at least as much
    // as the waterfall does: there is no independent overlay repaint here, so one never-settling
    // fetch froze the whole instrument (trace and all) for the life of the mount.
    const latch = new RowFetchLatch('scope')
    let acc = 0
    let last = performance.now()
    // ~20 Hz — plenty smooth for a scope, and the row is now cached engine-side (computed once per
    // audio feed in the radio loop) so each poll is a cheap read, not a Goertzel recompute under
    // the lock. Fewer + lighter polls = no more contention stutter (the "choppy" report).
    const ROW_MS = 50
    const ROW_MS_REDUCED = 120 // gentler under reduced-motion
    const AGC_ALPHA = 0.4 // snappy attack/release — a rig scope, not a slow FT8 noise floor
    const TRACE_FRAC = 0.45 // top fraction = panadapter trace; rest = waterfall
    // Trace persistence (fast attack / slow decay, the classic rig peak-hold): the trace
    // column jumps up instantly with a signal but FADES between syllables and key-ups instead
    // of strobing at frame rate with every gap in a bursty voice/CW signal (the "flashing
    // vertical line" report). Waterfall rows stay raw — the scroll IS their history; only the
    // instantaneous trace gets the hold.
    //
    // The TIME CONSTANT is the cockpit's (traceHoldMs, read live from the ref so changing it
    // never restarts the poll loop). It was one 400 ms constant for both, which is far too long
    // for CW: a 25 WPM dit gap gave back 11% of the trace height, so keying drew a static bar.

    const mq = window.matchMedia('(prefers-reduced-motion: reduce)')
    const reducedMotion = () =>
      mq.matches || document.documentElement.getAttribute('data-motion') === 'reduce'

    let agcFloor = 0
    let agcInit = false
    let lastFeed = '' // last onFeed-reported "source:lo:hi" (fire only on change)
    // The AGC window, reused. `row.slice(vLo, vHi)` allocated a fresh array 20x/second for a
    // read-only percentile scan. Float64Array, not Float32Array: `row` arrives from JSON as
    // doubles and narrowing it would nudge the AGC floor for no benefit (see agcScratch).
    let visBuf: Float64Array | null = null

    // ---- Which rows are committed: the source's frame number decides ----------------------
    //
    // ⚠️ THE REPEATED-ROW DEFECT, and why this is a frame number and not a timer. The scope used to
    // commit a waterfall row on every 50 ms poll whether or not the source had swept again, so a
    // source slower than 20 sweeps a second scrolled copies of its last sweep that looked exactly
    // like new data: on an IC-9700 at 3 sweeps a second, five rows in six. The backend now numbers
    // every frame it publishes (`seq`), each poll names the newest number this scope has drawn, and
    // the answer is null when the source has not advanced past it. What a null does is the
    // operator's slow-scope look (PHSCOPE_ROWS_KEY): a marked repeat, or nothing.
    /** The number of the newest frame drawn: what the next poll asks the source to be past. 0 =
     *  none yet, or a source nobody numbers (each of its answers is then a new row). */
    let lastSeq = 0
    /** The newest sweep as it arrived (never mirrored: a repeat or a redraw orients it to the axis
     *  in force then), the range it was committed in, and where it came from. */
    let newest: { frame: SpectrumFrame; range: DisplayRange; src: string; rowLo: number; rowHi: number } | null = null
    /** The drawn window, projected from the newest sweep: audio Hz or absolute RF Hz per the feed
     *  source, or RF offset from the dial on the carrier-centred axis. */
    let view = { loHz: 200, hiHz: 2900, markerAtHz: null as number | null, mirrored: false }
    // The trace's own frame: the newest sweep through the cockpit's averaging (the waterfall rows
    // stay raw, so averaging never smears history), then the peak hold. The hold is kept PER BIN,
    // as a strength in the trace's range — the same 0..1 the hold always decayed in — so it stays
    // on its frequencies when the view pans, and starts again when the bins mean other ones.
    const averager = new LogRecursiveAverager(scaleRef.current.averageMs)
    let averaged: SpectrumFrame | null = null
    let hold: Float32Array | null = null
    let heldBins: Float64Array | null = null
    let holdKey = ''
    let traceFrame: SpectrumFrame | null = null
    let lastHoldTs = 0

    // Device-pixel backing store (correct under the app's CSS zoom), mirroring Waterfall.
    let devW = 0
    let devH = 0
    let cssW = 1
    let cssH = 1
    let scaleY = 1
    // #215: this scope's overlay text — the DIAL plates and the frequency scale — follows the
    // UI scale, exactly as the Operate waterfall's axis digits do. `scaleY` alone does NOT
    // carry it: both it and the rect it divides are measured post-zoom on Chromium, so the
    // zoom cancels and a `10 * scaleY` font stays the same physical size at every UI scale
    // while the scope around it grows. `overlayTextScale` recovers the zoom from rect ÷ layout
    // width, and reads 1 on an engine that already carries it (see its own doc comment).
    let textScale = 1

    /** A frame as the drawn axis wants it: a MIRRORED axis (LSB on the carrier-centred one) reads
     *  the row backwards, so the frame is stored and drawn mirrored — bins reversed, span negated —
     *  rather than flagged for the renderer to flip later. A stored row then says what it means ON
     *  THE AXIS IT WAS DRAWN ON, so the 2D band, the 3D stack and scrollback all reproduce the live
     *  picture through the mapping they already have, and rows received on the other sideband keep
     *  their own honest frame. */
    const oriented = (f: SpectrumFrame): SpectrumFrame => (view.mirrored ? mirrorFrame(f) : f)

    /** Append a row to the history. While paused the picture is held still: the scrollback offset
     *  advances with every row that arrives (history keeps filling; nothing is lost). */
    const commit = (f: SpectrumFrame, range: DisplayRange) => {
      renderer.commitRow(oriented(f), range)
      if (pausedRef.current) offsetRef.current = Math.min(offsetRef.current + 1, Math.max(0, renderer.rows - 1))
    }

    /** Project the audio view window onto the newest row — audio rows directly, native RF
     *  panadapter rows anchored on the live dial (row-center fallback when the dial is unknown or
     *  outside the row). See scopeView. Done on every poll, so the view and the pointer's mapping
     *  follow the dial and the props between two sweeps of a slow source. */
    const project = (src: string, rowLo: number, rowHi: number) => {
      // ⚠️ A FILTER EDGE BEING DRAGGED HOLDS THE AXIS STILL. Phone's Auto span and CW's window are
      // sized FROM the filter, so every width the drag commands would rescale the axis under the
      // hand and run the edge away from it. The axis follows the new width on release instead.
      const held = dragRef.current
      if (held?.edge && held.dragging && lastViewRef.current) return
      view = scopeView(
        rowLo,
        rowHi,
        src,
        viewLoRef.current,
        viewHiRef.current,
        markerRef.current,
        sidebandSign(sidebandRef.current),
        dialRef.current,
        isSymmetricMode(sidebandRef.current),
        carrierCenteredRef.current,
      )
      // Tell the host what's actually drawn (only on change) — the honest scope label.
      const feed = `${src}:${view.loHz}:${view.hiHz}`
      if (feed !== lastFeed) {
        lastFeed = feed
        onFeedRef.current?.(src, view.loHz, view.hiHz)
      }
      // The drawn window for the pointer handlers (click hit-testing and drag-box Hz↔px mapping
      // happen against exactly what's on screen).
      lastViewRef.current = {
        lo: view.loHz,
        hi: view.hiHz,
        rf: isRfScopeSource(src),
        mirrored: view.mirrored,
      }
    }

    /** A new sweep: the AGC, the readout, the row, and the trace's averaging. */
    const accept = (wire: SpectrumFrameWire) => {
      const row = wire.bins
      // Surface which feed is live (only re-render on a change, not every 30 Hz frame).
      const src = wire.source ?? ''
      if (src !== sourceRef.current) {
        sourceRef.current = src
        setSource(src)
      }
      if (wire.seq > 0) lastSeq = wire.seq
      // Data-driven capture extent (DTO); fall back to the legacy constants for
      // older backends that don't report it.
      const rowLo = wire.loHz ?? 200
      const rowHi = wire.hiHz ?? 2900
      const span = Math.max(1, rowHi - rowLo)
      project(src, rowLo, rowHi)
      // The row as it arrived, for the click's signal snap (clickTuneTarget reads ROW Hz).
      lastRowRef.current = { row, rowLo, rowHi }

      // AGC over the VISIBLE window only — a loud signal outside the view (e.g.
      // the FT8 cluster above a narrow CW window) must not compress what's shown.
      //
      // Bin indices are ROW Hz and the carrier-centered axis is not: a MIRRORED (LSB) axis
      // runs the row backwards, so its bounds are the row's bounds negated and swapped. Undo
      // that here, once. A symmetric ±W axis hid this — it negates to itself — but the
      // asymmetric one does not, and unfixed an LSB view would window the wrong third of the
      // row: the AGC floor and the ▲dB readout would be measured where the voice isn't.
      const nb = row.length
      const winLo = view.mirrored ? -view.hiHz : view.loHz
      const winHi = view.mirrored ? -view.loHz : view.hiHz
      const vLo = Math.max(0, Math.floor(((winLo - rowLo) / span) * (nb - 1)))
      const vHi = Math.min(nb, Math.ceil(((winHi - rowLo) / span) * (nb - 1)) + 1)
      let visible: ArrayLike<number> = row
      if (vHi - vLo >= 8) {
        const n = vHi - vLo
        if (!visBuf || visBuf.length !== n) visBuf = new Float64Array(n)
        for (let i = 0; i < n; i++) visBuf[i] = row[vLo + i]
        visible = visBuf
      }
      // A FIXED WINDOW ABOVE THE NOISE — not a re-fit to this row's own peak.
      //
      // ⚠️ THIS IS WHAT MAKES A LOUD SIGNAL DRAW TALL (operator, 2026-08-15: "on my FTDX10 I see
      // big vertical spikes where the voice is; on Nexus it seems like it's all smoothed out
      // without the aggressive peaks"). It used to be `agcRange(visible)` — floor at the 5th
      // percentile, ceiling at the 99.5th — which had two independent faults, both measured:
      //   * the CEILING WAS THE SIGNAL, so every peak normalised to exactly 1.000 whether it was
      //     12 dB or 40 dB out of the noise. A signal could not get taller; it was already at
      //     the top, and the display just re-scaled around it every row.
      //   * the FLOOR WAS THE LEFT TAIL, which on an audio row is the rig's SSB stopband — 42 dB
      //     below the passband noise. So the noise floor itself rendered at 0.956 of full
      //     height: the whole passband sat as a bright slab at the top with nowhere to rise.
      // See `SCOPE_WINDOW_DB` and `scopeNoiseFloor` for the numbers and the reasoning.
      //
      // The floor still SMOOTHS (it is an estimate of a slowly-changing thing, and a jittering
      // one would bounce the whole picture vertically); the ceiling no longer needs smoothing at
      // all, because it is now the floor plus a constant and moves only when the floor does.
      // WF_FLOOR_PCT (the MEDIAN), not agcRange's 5% default — see SCOPE_WINDOW_DB.
      //
      // ⚠️ NOT the spectrum module's auto range (`scaleRange.ts`, the row MEAN − 5 dB): on an audio
      // row the rig's SSB stopband is a 40 dB cliff across a sixth of the row and more, which drags
      // a mean far below the passband noise and lights the whole passband — the bright slab above,
      // by another road. The median is what that header measures as barely moved by it.
      const floor = agcRange(visible, WF_FLOOR_PCT).floor
      // Frozen while keyed, same reason as the FT waterfall: the muted receiver drags the
      // noise estimate to digital silence and key-up clamps the panel until the EMA recovers.
      if (!txRef.current) {
        if (!agcInit) {
          agcFloor = floor
          agcInit = true
        } else {
          agcFloor += (floor - agcFloor) * AGC_ALPHA
        }
      }
      // Operator Gain/Zero on top, same semantics as the FT8 waterfall's controls: G widens
      // the window (2x at G-1) or tightens it (0.4x at G+1), Z trims the black point.
      //
      // ⚠️ THE 10 dB MINIMUM-SPAN CLAMP THAT USED TO SIT HERE IS GONE, and it is worth saying
      // why rather than leaving dead arithmetic that implies a constraint. It existed because
      // the window was fitted to each row, so a row with no signal had a tiny span that got
      // stretched across the whole palette — a quiet band rendered as full-width rainbow (the
      // "stuff on the waterfall with a quiet band" report). The window is now a fixed
      // SCOPE_WINDOW_DB above the noise and can never shrink, so that failure is
      // unrepresentable rather than clamped. A quiet band sits dark at the palette bottom
      // because there is genuinely nothing above the noise, which is the honest picture.
      const range = applyGainZero(
        agcFloor,
        agcFloor + dbToSpan(SCOPE_WINDOW_DB),
        scaleRef.current.gain,
        scaleRef.current.zero,
      )
      // Live readout: the strongest signal's height ABOVE THE NOISE FLOOR, in dB.
      //
      // It used to report the AGC window's own width, which was a real measurement while the
      // window was fitted to the row — and is now a constant, so it would have read "Δ50 dB"
      // forever. Peak-over-noise is the number that actually changes, and on a rig scope it is
      // the more useful one anyway: it is what the operator is looking at the spikes FOR.
      //
      // In the dB of the frame's own axis (`spectrum/scale.ts`): a CI-V scope's 0..1 spans the
      // 80 dB Icom gives its display, where the 120 every reading used to assume read 1.5x high.
      let peakDisp = 0
      for (let i = 0; i < visible.length; i++) if (visible[i] > peakDisp) peakDisp = visible[i]
      const db = Math.round((peakDisp - agcFloor) * axisOf(wire.scale, src).dbPerUnit)
      if (Number.isFinite(db) && db !== spanDbRef.current) {
        spanDbRef.current = db
        setSpanDb(db)
      }

      // The ROW's own bins over the row's own span, at the source's resolution — the renderer
      // maps each stored row from its own frame onto whatever view is asked for, so storing the
      // row is strictly more information for strictly less work than storing screen columns (the
      // clipped-history defect: a row interpolated to the view kept nothing outside it, and
      // scrollback could never widen). A frame with no producer time gets its arrival time.
      const frame: SpectrumFrame = { ...rendererFrame(wire), tMs: wire.tMs > 0 ? wire.tMs : Date.now() }
      newest = { frame, range, src, rowLo, rowHi }
      commit(frame, range)
      averager.tauMs = scaleRef.current.averageMs
      averaged = averager.push(frame, performance.now())
    }

    /** The trace: the newest averaged sweep through the peak hold, in the newest row's range. A
     *  new signal jumps up instantly; a pause fades down over ~traceHoldMs instead of strobing. */
    const holdTrace = () => {
      const a = averaged
      if (!a || !newest) return
      const nowTs = performance.now()
      const dt = lastHoldTs > 0 ? nowTs - lastHoldTs : ROW_MS
      lastHoldTs = nowTs
      const decay = traceHoldDecay(dt, traceHoldRef.current)
      const n = a.bins.length
      const key = `${n}:${a.loHz}:${a.hiHz}`
      if (!hold || !heldBins || key !== holdKey) {
        // Other bins, other frequencies (a retune, a zoom, a feed swap): held peaks would sit at
        // the wrong frequencies now, so drop them rather than painting ghosts.
        hold = new Float32Array(n)
        heldBins = new Float64Array(n)
        holdKey = key
      }
      const { floor, ceil } = newest.range
      for (let b = 0; b < n; b++) {
        const s = normalize(a.bins[b], floor, ceil)
        const h = hold[b] * decay
        hold[b] = s > h ? s : h
        heldBins[b] = floor + hold[b] * (ceil - floor)
      }
      traceFrame = { ...a, bins: heldBins }
    }

    /** The picture, then the overlay over it. */
    const draw = () => {
      if (!(devW > 0 && devH > 0)) return
      const dssOn = dssRef.current
      const pausedNow = pausedRef.current
      renderer.draw({
        view: { loHz: view.loHz, hiHz: view.hiHz },
        lut: lutRef.current,
        // 3D maximize hides the trace so the stacked-spectrum hill uses the full panel.
        layout: { traceH: dssOn ? 0 : Math.max(1, Math.round(devH * TRACE_FRAC)), stripH: 0, lineWidth: Math.max(1, scaleY) },
        detector: scaleRef.current.detector,
        mode: dssOn ? 'dss' : '2d',
        offsetRows: pausedNow ? offsetRef.current : 0,
        // Direction is the operator's (PHSCOPE_FLOW_KEY).
        newestAtTop: newestAtTopRef.current,
        // Paused = review: the trace band is the floor, as it always was while scrolled back.
        trace: !pausedNow && traceFrame && newest ? { frame: oriented(traceFrame), range: newest.range } : null,
      })
      syncOverlays()
      drawOverlay()
    }
    rebuildRef.current = draw
    overlayRef.current = () => {
      if (devW > 0 && devH > 0) {
        syncOverlays()
        drawOverlay()
      }
    }

    /** Everything the overlays last drew from, as one string; '' = draw them again. */
    let overlaysKey = ''
    let inks: OverlayInks | null = null
    let inksTheme = ''
    /** The overlays, drawn again only when their data or the axis under them moved: a sweep that moves
     *  neither leaves them as they are, and nothing here ever asks the picture to redraw. */
    const syncOverlays = () => {
      if (!octx || !overlays || !(devW > 0 && devH > 0)) return
      const d = overlayDataRef.current
      // 3D maximize: the stack fills the panel and carries no marks, as with the marks layer.
      const shown = !dssRef.current && newest != null
      const rf = newest != null && isRfScopeSource(newest.src)
      const axis = axisKind(rf)
      const theme = `${themeRef.current}|${nightRef.current}|${skinRef.current}`
      const key = shown
        ? [devW, devH, scaleY, textScale, view.loHz, view.hiHz, rf, axis.carrierCentered, axis.sideband,
            axis.dialHz, axis.pitchHz, axis.cwPitchRefDial, txRef.current, theme, d.v].join('|')
        : 'off'
      if (key === overlaysKey) return
      overlaysKey = key
      octx.clearRect(0, 0, devW, devH)
      hitsRef.current = { w: devW, h: devH, hits: [] }
      // Nothing to overlay (a host that passes none): no inks read, nothing drawn.
      if (!shown || (d.spots.length === 0 && d.spans == null && d.ft == null)) return
      if (!inks || inksTheme !== theme) {
        inks = readOverlayInks(overlays)
        inksTheme = theme
      }
      const out = drawOverlays(
        octx,
        {
          w: devW,
          h: devH,
          textPx: scaleY * textScale,
          lineW: Math.max(1, scaleY),
          lo: view.loHz,
          hi: view.hiHz,
          axis,
          spans: d.spans,
          spots: d.spots,
          ft: d.ft,
          transmitting: txRef.current,
        },
        inks,
      )
      hitsRef.current = { w: devW, h: devH, hits: out.hits }
    }
    overlaysSyncRef.current = syncOverlays

    /** The marks over the picture: what the operator reads and aims with. Cleared every draw. */
    const drawOverlay = () => {
      ctx.clearRect(0, 0, devW, devH)
      subMarkRef.current = null
      // 3D maximize: the stack fills the panel and carries no marks, as it never has.
      if (dssRef.current || !newest) return
      const src = newest.src
      const Wd = devW
      const lo = view.loHz
      const hi = view.hiHz
      // #215: device px per unit of OVERLAY TEXT — the pixel ratio `scaleY` carries, times the
      // UI scale it does not. Only the plates and the frequency scale use it; the rules, ticks
      // and the trace stay on `scaleY`, because they are graphics the operator does not read.
      const textPx = scaleY * textScale

      // ---- The receivers' passbands and the manual notch: under every line and plate below ----
      // Main's passband from the rig's REPORTED width (`spectrum/markers.ts`), its grabbable edges
      // heavier; a second receiver in its own colour, on an RF row only (an audio row is Main's own
      // receiver audio, with no place for another dial).
      const xOf = (hz: number) => ((hz - lo) / (hi - lo)) * Wd
      const rfRow = isRfScopeSource(src)
      const axis = axisKind(rfRow)
      const sub = subRef.current
      if (sub && rfRow) {
        const subAxis: AxisKind = { ...axis, sideband: sub.sideband, dialHz: sub.dialHz, cwPitchRefDial: true }
        const sp = sub.widthHz != null ? passbandOnAxis(subAxis, sub.widthHz) : null
        if (sp) drawPassband(ctx, xOf, devH, sp, MARK_RGB.sub, [], scaleY)
        if (sub.dialHz >= lo && sub.dialHz <= hi) {
          const sx = Math.round(xOf(sub.dialHz))
          ctx.strokeStyle = `rgba(${MARK_RGB.sub}, 0.8)`
          ctx.lineWidth = Math.max(1, scaleY)
          ctx.beginPath()
          ctx.moveTo(sx, 0)
          ctx.lineTo(sx, devH)
          ctx.stroke()
          ctx.fillStyle = `rgba(${MARK_RGB.sub}, 0.8)`
          const fontPx = Math.max(8, Math.round(10 * textPx))
          ctx.font = `${fontPx}px system-ui, sans-serif`
          ctx.textAlign = 'left'
          ctx.textBaseline = 'top'
          ctx.fillText(SUB_PLATE, sx + 3 * textPx, 2 * textPx)
          // Where it was drawn is where a press is on it (`subUnder`).
          subMarkRef.current = {
            w: devW,
            h: devH,
            x: sx,
            plateR: sx + 3 * textPx + ctx.measureText(SUB_PLATE).width,
            plateB: 2 * textPx + fontPx,
          }
        }
      }
      const width = shownWidth()
      const mp = width != null ? passbandOnAxis(axis, width) : null
      if (mp) drawPassband(ctx, xOf, devH, mp, MARK_RGB.main, canEditPassband() ? edgesOnAxis(axis) : [], scaleY)
      const notchAudio = notchRef.current
      const notchAt = notchAudio != null ? notchOnAxis(axis, notchAudio) : null
      if (notchAt != null && notchAt > lo && notchAt < hi) drawNotch(ctx, xOf(notchAt), devH, scaleY)

      // ---- Carrier line (Phone): the DIAL, at the 1/9 mark (USB) or the 8/9 mark (LSB) ----
      //
      // WHY THE GUARD BAND IS ALWAYS QUIET, and it is not a bug to be fixed later. This scope
      // is fed by DEMODULATED RECEIVER AUDIO, which is one-sided: an SSB detector folds the
      // wanted sideband down to 0–3 kHz and throws the image away, so there is no signal on
      // the other side of the carrier to draw. That is why the axis is not centered — a
      // centered dial spent half the panel on that side and squeezed the voice into ~30% of
      // the width (operator screenshot, 2026-08-16). The guard band is the whole cost of having
      // the dial read as a line rather than an edge, and it was cut from W/3 to W/8 on 2026-08-23:
      // a third of the panel standing empty beside the marker read as the marker being misplaced,
      // when in fact a USB dial belongs at the LOW edge of its own voice. A radio that streams its OWN panadapter
      // (Flex, Icom CI-V) sends real RF and genuinely fills both sides; that feed takes the RF
      // branch in scopeView and never reaches this code.
      //
      // The x is derived from AXIS COORDINATE 0, the one place the dial is defined, so the
      // line cannot land anywhere but where the row was painted around it — change the
      // geometry in scopeView and this follows with no edit here.
      if (carrierCenteredRef.current && !isRfScopeSource(src)) {
        const cx = Math.round(((0 - lo) / (hi - lo)) * Wd)
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)'
        ctx.lineWidth = Math.max(1, scaleY)
        ctx.beginPath()
        ctx.moveTo(cx, 0)
        ctx.lineTo(cx, devH)
        ctx.stroke()
        ctx.fillStyle = 'rgba(255, 255, 255, 0.7)'
        ctx.font = `${Math.max(8, Math.round(10 * textPx))}px system-ui, sans-serif`
        ctx.textAlign = 'left'
        ctx.textBaseline = 'top'
        ctx.fillText(DIAL_PLATE, cx + 3 * textPx, 2 * textPx)
      }

      // ---- Dial line (native RF panadapter): where the VFO actually is ----
      //
      // Operator request from an IC-7300 user, 2026-08-20: "The RF Panadapter is now
      // displayed! Is there a way to place a dial indicator on the freq tuned?" There was
      // not. The carrier line above is explicitly `!isRfScopeSource`, because it marks audio
      // axis 0 — a coordinate a real RF row does not have — so a native panadapter drew a
      // spectrum with nothing saying which part of it you were listening to. Every other
      // panadapter in the hobby marks the dial, and the rig's own screen does.
      //
      // The dial is an ABSOLUTE RF frequency here and so are lo/hi, which is what makes this
      // three lines: the same projection the row itself was painted with. It is NOT assumed
      // to be the middle. scopeView centres the window on the dial only when the dial is
      // inside the row and no marker moves it, so on a rig in FIXED scope mode — where the
      // span is a band segment and the VFO sits wherever you tuned it — the line lands where
      // the VFO really is, off-centre, which is the whole point of that mode.
      //
      // DRAWN ONLY WHEN IT IS GENUINELY IN VIEW. A dial outside the window is not clamped to
      // an edge: a line pinned to the left of a panadapter saying DIAL, while the dial is a
      // megahertz further down, is worse than no line. scopeView already falls back to the
      // row centre when the dial is outside the row, and this refuses to draw on that
      // fallback rather than marking a frequency nobody is tuned to.
      if (isRfScopeSource(src) && dialRef.current != null) {
        const dialHz = dialRef.current
        const dx = Math.round(((dialHz - lo) / (hi - lo)) * Wd)
        // The CW pitch marker below lands EXACTLY here on an RF row (scopeView maps the
        // marker through the dial), so drawing both would stack a solid line under a dashed
        // one. Where they coincide the marker wins: it carries the extra meaning.
        const mAt = view.markerAtHz
        const mx = mAt == null ? null : Math.round(((mAt - lo) / (hi - lo)) * Wd)
        // INCLUSIVE bounds, and that is not a rounding detail. On USB a plain audio window
        // projects onto RF as dial → dial+span, so the dial IS the window's left edge (its
        // right edge on LSB); only the symmetric modes (FM/AM) centre it. An exclusive test
        // would therefore refuse to draw in exactly the case an SSB operator is in.
        if (dialHz >= lo && dialHz <= hi && (mx == null || Math.abs(mx - dx) > 1)) {
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)'
          ctx.lineWidth = Math.max(1, scaleY)
          ctx.beginPath()
          ctx.moveTo(dx, 0)
          ctx.lineTo(dx, devH)
          ctx.stroke()
          ctx.fillStyle = 'rgba(255, 255, 255, 0.7)'
          ctx.font = `${Math.max(8, Math.round(10 * textPx))}px system-ui, sans-serif`
          ctx.textAlign = 'left'
          ctx.textBaseline = 'top'
          ctx.fillText(DIAL_PLATE, dx + 3 * textPx, 2 * textPx)
        }
      }

      // ---- Frequency scale: where a click will actually put you --------------------
      //
      // Operator, 2026-08-22: "can freq numbers be added to the phone waterfall? It's a bit
      // difficult to see where a mouse click will take you." The Operate waterfall has had an
      // axis all along; this one had the DIAL plate and nothing else, so a signal two thirds of
      // the way across was a guess.
      //
      // ABSOLUTE frequency, not offset from the dial. The question being answered is "where will
      // I land", and an operator reading "-3.2k" still has arithmetic to do mid-QSO.
      //
      // The axis model (`axisToRf`, the inverse of where the tags, the notch and the passband sit)
      // owns the one thing that is easy to get wrong here: what a point on THIS axis is in RF. A
      // native RF panadapter is absolute already; Phone's carrier-centred axis is the offset from
      // the dial; CW's audio window hears a signal ON the dial at the pitch in true CW, while the
      // soundcard keyer and plain SSB audio hear the carrier at 0, on the sideband's side. The
      // scale read `dial + audio` everywhere until 2026-10-04, so in true CW it stood a pitch high
      // under the very tags it labels (600 Hz by default), and ran backwards on the reverse
      // sideband. It returns null where there is no honest answer (an audio row with the dial
      // unknown, an AM/FM baseband, where a click does not tune either), and then nothing is
      // drawn — a wrong number on a scale someone tunes by is worse than a blank one.
      {
        const ticks = axisTicks(lo, hi, 6)
        // Ticks under a kilohertz apart (CW's 300–800 Hz window; Phone's at a 2.4 kHz width) put the
        // same kHz under several labels at three decimals: CW's default scale read `7.030` five times
        // (operator, 2026-10-04). There each label carries the 100 Hz digit too. A lone tick only
        // stands on a window narrow enough for the finest step.
        const fine = ticks.length < 2 || ticks[1] - ticks[0] < 1_000
        // Where the next label may start: past the last one drawn, by the room a label keeps from its tick.
        let clearFrom = -Infinity
        for (const t of ticks) {
          const abs = axisToRf(axis, t)
          if (abs == null) break
          const tx = Math.round(((t - lo) / (hi - lo)) * Wd)
          // Skip a tick sitting on the dial line — its plate is already there and the two
          // would overprint.
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.20)'
          ctx.lineWidth = Math.max(1, scaleY)
          ctx.beginPath()
          ctx.moveTo(tx, devH - 10 * scaleY)
          ctx.lineTo(tx, devH)
          ctx.stroke()
          ctx.fillStyle = 'rgba(255, 255, 255, 0.55)'
          ctx.font = `${Math.max(8, Math.round(9 * textPx))}px system-ui, sans-serif`
          ctx.textBaseline = 'bottom'
          // Three decimals of MHz is the kHz an operator dials; a fine scale adds the 100 Hz digit,
          // rounded in whole hertz first. `toFixed` alone rounds the binary double, so two ticks 100 Hz
          // apart that both end in 50 Hz can round toward each other and read alike. Nudged inward at
          // the edges so a label is never half-clipped — a truncated frequency is a misleading one.
          const label = fine ? (Math.round(abs / 100) / 1e4).toFixed(4) : (abs / 1e6).toFixed(3)
          const w = ctx.measureText(label).width
          ctx.textAlign = 'left'
          const lx = Math.min(Wd - w - 2 * textPx, Math.max(2 * textPx, tx + 3 * textPx))
          // A label that would stand on the one before it is left off, and its tick stays: two labels run
          // together read as one wrong number. Only a narrow scope does it, most often at the right edge,
          // where the nudge pushes the last label back over its neighbour (a phone-width CW scope at 20 m
          // or 2 m, 2026-10-05). A scale with room for every label draws them all, where it always has.
          if (lx < clearFrom) continue
          ctx.fillText(label, lx, devH - 2 * textPx)
          clearFrom = lx + w + 3 * textPx
        }
      }

      // ---- Pitch marker (CW): tune a carrier onto the hairline = zero-beat ----
      // (on a native RF row scopeView puts the marker exactly ON the dial)
      const markerAt = view.markerAtHz
      if (markerAt != null && markerAt > lo && markerAt < hi) {
        const mx = Math.round(((markerAt - lo) / (hi - lo)) * Wd)
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.55)'
        ctx.setLineDash([4 * scaleY, 3 * scaleY])
        ctx.lineWidth = Math.max(1, scaleY)
        ctx.beginPath()
        ctx.moveTo(mx, 0)
        ctx.lineTo(mx, devH)
        ctx.stroke()
        ctx.setLineDash([])
      }
    }

    const measure =(entry?: ResizeObserverEntry): { dW: number; dH: number } => {
      const dpcb = entry?.devicePixelContentBoxSize?.[0]
      if (dpcb) return { dW: Math.max(1, dpcb.inlineSize), dH: Math.max(1, dpcb.blockSize) }
      const dpr = window.devicePixelRatio || 1
      return { dW: Math.max(1, Math.round(cssW * dpr)), dH: Math.max(1, Math.round(cssH * dpr)) }
    }
    // The overlay canvas and the renderer's host fill the same box (the canvas in flow, the host
    // absolutely over it), so one measurement sizes both.
    const resize = (entry?: ResizeObserverEntry) => {
      const rect = canvas.getBoundingClientRect()
      if ((canvas.offsetParent === null || rect.width < 2 || rect.height < 2) && devW > 0) return
      cssW = Math.max(1, rect.width)
      cssH = Math.max(1, rect.height)
      textScale = overlayTextScale(rect.width, canvas.offsetWidth)
      const { dW, dH } = measure(entry)
      scaleY = dH / cssH
      if (dW === devW && dH === devH) return
      canvas.width = dW
      canvas.height = dH
      if (overlays) {
        overlays.width = dW
        overlays.height = dH
      }
      devW = dW
      devH = dH
      // Redraw the history at the new geometry (smear-free), not a stretched old bitmap.
      renderer.resize(dW, dH)
      draw()
    }
    resize()
    const ro = new ResizeObserver((entries) => resize(entries[0]))
    try {
      ro.observe(canvas, { box: 'device-pixel-content-box' })
    } catch {
      ro.observe(canvas)
    }

    const poll = async (myGen: number) => {
      let wire: SpectrumFrameWire | null
      try {
        // Ask for the frame over the window this scope is DRAWING, not the whole 0-4000 Hz
        // capture. Same 512 bins, same bytes — 1.5625 Hz per bin on the CW cockpit's 300-1100
        // view instead of 7.8125.
        //
        // The view props are passed RAW even though they carry ABSOLUTE RF Hz when a native
        // panadapter is the source (the cockpit passes `rfSpan`). That is deliberate: the
        // backend already has to reject an insane span, so letting it own the one rule beats
        // duplicating the RF test here against a source we only learn from the PREVIOUS row.
        // An unhonourable request returns exactly what `getSpectrumRow` would have, and the
        // frame's own loHz/hiHz below is what everything downstream reads anyway.
        wire = rfOnly
          ? await getRfFrame(lastSeq)
          : await getScopeFrame(viewLoRef.current, viewHiRef.current, scaleRef.current.window, lastSeq)
      } catch {
        if ((!control || rfOnly) && latch.owns(myGen)) setScopeAvailable(false)
        return
      }
      // Superseded while awaiting — the watchdog gave this call up. Drawing now would put a
      // stale trace and a stale waterfall row on screen out of order.
      if (!latch.owns(myGen)) return
      if (wire) {
        // An empty frame: the source went quiet — or, for the RF pane, no panadapter is streaming.
        // The RF pane says so on every window; the rig scope only on a remote one, as before.
        if (!wire.bins || wire.bins.length === 0) {
          if (!control || rfOnly) setScopeAvailable(false)
          return
        }
        if (!control || rfOnly) setScopeAvailable(true)
        accept(wire)
      } else if (newest) {
        // The source has not swept since the last row. The view still follows the dial and the
        // props, so the pointer maps against what is on screen between two sweeps.
        project(newest.src, newest.rowLo, newest.rowHi)
        // SMOOTH SCROLL: the newest sweep again, as a REPEAT — its own number, which is what marks
        // it (a row carrying the number of the row before it), and its own time. ONE ROW PER SWEEP:
        // nothing; the waterfall moves when the source does.
        if (rowCadenceRef.current === 'smooth') commit({ ...newest.frame, tMs: Date.now() }, newest.range)
      } else {
        return
      }
      // PAUSED = review mode: history keeps filling (nothing is lost) but the scope is frozen;
      // the mouse wheel scrolls the band back through `rebuildRef`.
      if (pausedRef.current) return
      holdTrace()
      draw()
    }

    const loop = (now: number) => {
      if (!running) return
      if (!activeRef.current) {
        last = now
        acc = 0
        rafRef.current = requestAnimationFrame(loop)
        return
      }
      acc += now - last
      last = now
      const rowMs = reducedMotion() ? ROW_MS_REDUCED : ROW_MS
      // Give up on a fetch that never settles, so the scope resumes polling instead of sitting
      // frozen but mounted. Before the claim, so the freed latch is usable now.
      latch.abandonIfStuck(now)
      if (acc >= rowMs) {
        const myGen = latch.begin(now)
        if (myGen !== null) {
          acc = 0
          poll(myGen)
            .catch(() => {})
            .finally(() => latch.end(myGen))
        }
      }
      rafRef.current = requestAnimationFrame(loop)
    }
    rafRef.current = requestAnimationFrame(loop)

    return () => {
      running = false
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
      ro.disconnect()
      rebuildRef.current = null
      overlayRef.current = null
      overlaysSyncRef.current = null
      rendererRef.current = null
      // Hands a WebGL2 context back at once (browsers cap live contexts) and takes the canvas out.
      renderer.destroy()
    }
    // run once; live props read via refs
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The overlays' data: a new spot list, a privilege answer or a slot's decodes draws the overlays
  // again now, between two sweeps or while paused — and only them, never the picture.
  useEffect(() => {
    const d = overlayDataRef.current
    overlayDataRef.current = { v: d.v + 1, spots: spots ?? [], spans: privSpans, ft }
    overlaysSyncRef.current?.()
  }, [spots, privSpans, ft])

  // ---- Click-to-tune + drag-a-passband-box (Flex-style) ----------------------------
  // All handlers read refs (never stale) and hit-test against exactly the drawn window.
  const xToHz = (clientX: number): number | null => {
    const canvas = canvasRef.current
    const view = lastViewRef.current
    if (!canvas || !view) return null
    const rect = canvas.getBoundingClientRect()
    if (rect.width < 2 || !(view.hi > view.lo)) return null
    const frac = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    const axisHz = view.lo + frac * (view.hi - view.lo)
    // ROW Hz is what every caller wants (hit-testing the row, clickTuneTarget's audio
    // branch, the relative drag anchor) — so undo the axis mirror here, once, rather than
    // at each of them. On the empty half this is a negative audio Hz, and that is the right
    // answer: it says "this many Hz the other side of the dial", which is exactly how
    // clickTuneTarget's `dial + sign·(af − lowcut)` then moves the dial toward the click.
    return view.mirrored ? -axisHz : axisHz
  }
  /** The AXIS Hz under a client x — the drawn axis, never un-mirrored (`xToHz` answers row Hz),
   *  which is where the receiver marks are placed. */
  const axisAt = (clientX: number): number | null => {
    const canvas = canvasRef.current
    const view = lastViewRef.current
    if (!canvas || !view) return null
    const rect = canvas.getBoundingClientRect()
    if (rect.width < 2 || !(view.hi > view.lo)) return null
    const frac = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return view.lo + frac * (view.hi - view.lo)
  }
  /** The grabbable filter edge under a client x, with the passband and axis it was found on and
   *  the width it starts from; null where there is none to grab. */
  const edgeUnder = (clientX: number): { edge: Edge; p: AxisPassband; axis: AxisKind; hz: number } | null => {
    if (!canEditPassband()) return null
    const canvas = canvasRef.current
    const view = lastViewRef.current
    const width = shownWidth()
    if (!canvas || !view || width == null || !(view.hi > view.lo)) return null
    const axis = axisKind(view.rf)
    const p = passbandOnAxis(axis, width)
    if (!p) return null
    const rect = canvas.getBoundingClientRect()
    const xIn = clientX - rect.left
    const zone = scanZonePx(rect.width)
    if (xIn <= zone || xIn >= rect.width - zone) return null // the edge scan's band, as it always was
    const px = (hz: number) => ((hz - view.lo) / (view.hi - view.lo)) * rect.width
    const edge = edgeNear(xIn, px(p.lo), px(p.hi), edgesOnAxis(axis), EDGE_TOL_PX)
    return edge ? { edge, p, axis, hz: width } : null
  }
  /** Is a client point on the Sub's marker as last drawn: within EDGE_TOL_PX of its line, top to
   *  bottom, or on its plate? */
  const subUnder = (clientX: number, clientY: number): boolean => {
    const m = subMarkRef.current
    const canvas = canvasRef.current
    if (!m || !canvas) return false
    const rect = canvas.getBoundingClientRect()
    if (rect.width < 2 || rect.height < 2) return false
    const x = ((clientX - rect.left) / rect.width) * m.w
    const y = ((clientY - rect.top) / rect.height) * m.h
    return Math.abs(x - m.x) <= EDGE_TOL_PX * (m.w / rect.width) || (x >= m.x && x <= m.plateR && y <= m.plateB)
  }
  /** The spot whose tag is under a client point, where a tag can be clicked now; null elsewhere. */
  const tagUnder = (clientX: number, clientY: number): SpotRow | null => {
    if (!spotsClickable()) return null
    const canvas = canvasRef.current
    const { w, h, hits } = hitsRef.current
    if (!canvas || hits.length === 0) return null
    const rect = canvas.getBoundingClientRect()
    if (rect.width < 2 || rect.height < 2) return null
    const x = ((clientX - rect.left) / rect.width) * w
    const y = ((clientY - rect.top) / rect.height) * h
    return hits.find((t) => x >= t.l && x <= t.r && y >= t.t && y <= t.b)?.spot ?? null
  }
  /** Review mode's scrollback: `step` rows older (+) or newer (−), held inside the ring. The wheel
   *  and the ↑/↓ keys both land here. */
  const scrollBack = (step: number) => {
    const cur = offsetRef.current
    const next = Math.max(0, Math.min(Math.max(0, (rendererRef.current?.rows ?? 0) - 1), cur + step))
    if (next !== cur) {
      offsetRef.current = next
      rebuildRef.current?.()
    }
  }
  // Imperative box positioning — pointer-rate (60 fps), decoupled from the 20 Hz canvas.
  const positionBox = (centerHz: number, widthHz: number) => {
    const canvas = canvasRef.current
    const box = boxRef.current
    const view = lastViewRef.current
    if (!canvas || !box || !view) return
    const rect = canvas.getBoundingClientRect()
    const span = view.hi - view.lo
    if (span <= 0 || rect.width < 2) return
    const leftPx = ((centerHz - widthHz / 2 - view.lo) / span) * rect.width
    const widthPx = (widthHz / span) * rect.width
    const l = Math.max(0, leftPx)
    const r = Math.min(rect.width, leftPx + widthPx)
    box.style.left = `${l}px`
    box.style.width = `${Math.max(2, r - l)}px`
    box.style.display = 'block'
  }
  // Box pinned at the cursor's PIXEL position — used while edge-scanning, where the view
  // is sliding under the cursor and an Hz-derived position would spring back to center.
  const positionBoxAtCursor = (clientX: number, widthHz: number) => {
    const canvas = canvasRef.current
    const box = boxRef.current
    const view = lastViewRef.current
    if (!canvas || !box || !view) return
    const rect = canvas.getBoundingClientRect()
    const span = view.hi - view.lo
    if (span <= 0 || rect.width < 2) return
    const widthPx = Math.max(2, (widthHz / span) * rect.width)
    const cx = Math.min(rect.width, Math.max(0, clientX - rect.left))
    const l = Math.max(0, Math.min(rect.width - widthPx, cx - widthPx / 2))
    box.style.left = `${l}px`
    box.style.width = `${widthPx}px`
    box.style.display = 'block'
  }
  const endGesture = () => {
    if (boxRef.current) boxRef.current.style.display = 'none'
    if (canvasRef.current) canvasRef.current.style.cursor = ''
    dragRef.current = null
    scanRef.current = { dir: 0, depth: 0 }
    if (scanRafRef.current != null) {
      cancelAnimationFrame(scanRafRef.current)
      scanRafRef.current = null
    }
    scanTsRef.current = 0
  }
  const clickOnly = onBeginClick != null
  useEffect(() => {
    if (!clickOnly) return
    const cancel = () => endGesture()
    const escape = (e: KeyboardEvent) => { if (e.key === 'Escape') cancel() }
    window.addEventListener('blur', cancel); window.addEventListener('keydown', escape)
    return () => { window.removeEventListener('blur', cancel); window.removeEventListener('keydown', escape); cancel() }
  }, [clickOnly])
  useEffect(() => { if (clickOnly && (!interactive || !scopeAvailable)) endGesture() }, [clickOnly, interactive, scopeAvailable])
  // Edge-scan tuning curve: cubic in depth, so most of the zone gives FINE speed control
  // and the last few pixels ramp hard; at the extreme edge the dial moves ~3 visible
  // spans per second. The dial advances rate×dt per tick from wherever it IS (never
  // "jump to the view edge"), so the band scrolls smoothly instead of lurching half a
  // view per CAT flush. Self-limiting: at a band edge the CAT write stops (useScopeTune's
  // band check) and the runaway guard keeps the optimistic target within a span of the
  // rig's real dial, so a stalled link can't build a backlog.
  const SCAN_MAX_SPANS_PER_S = 3.0
  const scanTick = (now: number) => {
    scanRafRef.current = null
    const g = dragRef.current
    const s = scanRef.current
    const view = lastViewRef.current
    if (!g || !g.dragging || s.dir === 0 || !view) {
      scanTsRef.current = 0
      if (dragRef.current) dragRef.current.scanDialHz = null
      return
    }
    const dt = scanTsRef.current > 0 ? Math.min(100, now - scanTsRef.current) : 16
    scanTsRef.current = now
    const span = view.hi - view.lo
    const W = boxWidthFor(sidebandRef.current, filterWidthRef.current ?? null)
    const off = cwDragOffHz()
    // Seed the optimistic scan dial from the last commanded target; a drag that began
    // straight in the edge zone has no target yet — seed from the rig's real dial.
    if (g.scanDialHz == null) {
      g.scanDialHz =
        g.centerHz > 0
          ? dialFromBoxCenter(g.centerHz, sidebandRef.current, W) - off
          : (dialRef.current ?? 0)
      if (!(g.scanDialHz > 0)) {
        g.scanDialHz = null // dial unknown — re-seed next tick rather than tuning to nonsense
        scanRafRef.current = requestAnimationFrame(scanTick)
        return
      }
    }
    const rate = span * SCAN_MAX_SPANS_PER_S * s.depth * s.depth * s.depth // Hz/s, cubic
    g.scanDialHz += s.dir * rate * (dt / 1000)
    // Runaway guard: never run more than one span ahead of the rig's real dial.
    const realDial = dialRef.current
    if (realDial != null) {
      g.scanDialHz = Math.min(realDial + span, Math.max(realDial - span, g.scanDialHz))
    }
    // Keep the logical box center consistent (release/final report reads it back).
    const edges = boxEdges(g.scanDialHz + off, sidebandRef.current, W)
    g.centerHz = (edges.loHz + edges.hiHz) / 2
    positionBoxAtCursor(g.cursorX, W) // pinned under the hand — the band moves, not the box
    onTuneRef.current?.({ dialHz: Math.round(g.scanDialHz), kind: 'drag' })
    scanRafRef.current = requestAnimationFrame(scanTick)
  }
  const ensureScanLoop = () => {
    if (scanRafRef.current == null && scanRef.current.dir !== 0) {
      scanTsRef.current = 0
      scanRafRef.current = requestAnimationFrame(scanTick)
    }
  }
  // Soundcard-keyed CW rides SSB, so a dropped box must leave the dial sign×pitch off
  // the signal (tone at the pitch). True-CW rigs (and every other mode): 0.
  const cwDragOffHz = () => {
    const m = sidebandRef.current.trim().toUpperCase()
    if (!m.startsWith('CW') || cwPitchRefRef.current !== false) return 0
    return sidebandSign(sidebandRef.current) * pitchRef.current
  }
  const clickContext = () => {
    const rect = canvasRef.current?.getBoundingClientRect()
    return JSON.stringify([sourceRef.current, lastViewRef.current, dialRef.current, sidebandRef.current,
      pitchRef.current, cwPitchRefRef.current, filterWidthRef.current, rect?.left, rect?.top, rect?.width, rect?.height])
  }
  const tunable = interactive && (onTune != null || clickOnly) && (!clickOnly || scopeAvailable)
  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!interactiveRef.current || (!onTuneRef.current && !onBeginClickRef.current) || e.button !== 0) return
    if (onBeginClickRef.current && dragRef.current) return
    const view = lastViewRef.current
    if (!view || !lastRowRef.current) return // pre-first-draw — nothing to hit-test
    const click = onBeginClickRef.current?.()
    if (onBeginClickRef.current && !click) return
    e.currentTarget.setPointerCapture(e.pointerId)
    // On a grabbable filter edge, a move resizes the passband instead of sliding the box. The edge
    // keeps the press over a spot's tag: a tag is asked for only where no edge is.
    const edge = click ? undefined : (edgeUnder(e.clientX) ?? undefined)
    // rf captured at press so a mid-gesture feed swap can't change the semantics.
    dragRef.current = {
      click: click ?? undefined,
      clickContext: click ? clickContext() : undefined,
      pointerId: e.pointerId,
      x0: e.clientX,
      y0: e.clientY,
      rf: view.rf,
      moved: false,
      dragging: false,
      centerHz: 0,
      cursorX: e.clientX,
      scanDialHz: null,
      grabAfHz: xToHz(e.clientX) ?? 0,
      grabDialHz: dialRef.current,
      edge,
      spot: edge ? undefined : (tagUnder(e.clientX, e.clientY) ?? undefined),
      sub: subUnder(e.clientX, e.clientY),
    }
  }
  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const g = dragRef.current
    if (!g) {
      // Hovering: say where a filter edge can be grabbed, that a click on the Sub's marker tunes
      // nothing, and where a spot's tag can be clicked.
      if (canvasRef.current)
        canvasRef.current.style.cursor = edgeUnder(e.clientX)
          ? 'ew-resize'
          : subUnder(e.clientX, e.clientY)
            ? 'default'
            : tagUnder(e.clientX, e.clientY)
              ? 'pointer'
              : ''
      return
    }
    if (g.click && e.pointerId !== g.pointerId) return
    if (!g.moved && Math.hypot(e.clientX - g.x0, e.clientY - g.y0) <= 6) return // click wobble
    g.moved = true
    if (g.click) return // click-only Remote cannot start a native drag or edge scan
    if (g.edge) {
      // A grabbed FILTER EDGE: the width follows the hand, legal for this cockpit, and each new
      // width goes to the host's coalescer (one write per flush, the last width wins). The
      // tuning gate is asked on every move, so a transmitter keying mid-drag ends it here.
      if (!canEditPassband()) {
        endGesture()
        overlayRef.current?.()
        return
      }
      const hz = axisAt(e.clientX)
      // The RF pane has no filter to drag: it is display only (2026-10-04), so it has no limits either.
      const limits = cockpitRef.current === 'rfpan' ? null : PASSBAND_LIMITS[cockpitRef.current]
      if (hz == null || limits == null) return
      g.dragging = true
      if (canvasRef.current) canvasRef.current.style.cursor = 'ew-resize'
      const w = clampPassband(widthForEdge(g.edge.axis, g.edge.p, g.edge.edge, hz), limits)
      if (w !== g.edge.hz) {
        g.edge.hz = w
        onPassbandRef.current?.(w)
      }
      overlayRef.current?.()
      return
    }
    const view = lastViewRef.current
    const rect = canvasRef.current?.getBoundingClientRect()
    if (!view || !rect || rect.width < 4) return
    const W = boxWidthFor(sidebandRef.current, filterWidthRef.current ?? null)
    g.dragging = true
    g.cursorX = e.clientX
    if (canvasRef.current) canvasRef.current.style.cursor = 'grabbing'
    // Edge-scan zone: holding within the outer band keeps scrolling — cubic speed
    // (fine control through most of the zone, ramping hard at the very edge; pointer
    // capture means past-the-edge counts as full depth). Center region = normal drag.
    // Runs for BOTH row sources: scanTick works in absolute dial Hz seeded from the
    // rig's real dial, so the audio scope (Yaesu — no native panadapter) scans the
    // band exactly like the RF scopes do.
    const EDGE = scanZonePx(rect.width)
    const xIn = e.clientX - rect.left
    if (xIn <= EDGE) scanRef.current = { dir: -1, depth: Math.min(1, (EDGE - xIn) / EDGE) }
    else if (xIn >= rect.width - EDGE)
      scanRef.current = { dir: 1, depth: Math.min(1, (xIn - (rect.width - EDGE)) / EDGE) }
    else scanRef.current = { dir: 0, depth: 0 }
    if (scanRef.current.dir !== 0) {
      // Scanning: the box pins under the cursor and the rAF loop owns the tuning —
      // reporting cursor-Hz here would leap to the view edge and fight the smooth scan.
      // The audio-drag anchor is invalidated so a return to mid-view re-seeds from the
      // scanned-to dial instead of springing back to the original grab point.
      g.grabDialHz = null
      positionBoxAtCursor(e.clientX, W)
      ensureScanLoop()
      return
    }
    if (!g.rf) {
      // Mid-view drag on the AUDIO scope (Yaesu — no native panadapter): a RELATIVE
      // band drag. The audio window is anchored to the dial (af tracks RF − dial), so
      // moving the hand by Δaf retunes the dial by −sign·Δaf — the grabbed signal
      // follows the cursor, with the passband box riding under the hand. (The RF
      // scopes' absolute box placement below is meaningless here — no RF map.)
      const af = xToHz(e.clientX)
      if (af == null) return
      if (g.grabDialHz == null) {
        // Fresh anchor (first move, or just left an edge-scan): re-seed from the
        // optimistic scan dial when there is one, else the rig's real dial.
        const seed = g.scanDialHz ?? dialRef.current
        if (seed == null) return
        g.grabDialHz = seed
        g.grabAfHz = af
      }
      const target = g.grabDialHz - sidebandSign(sidebandRef.current) * (af - g.grabAfHz)
      g.scanDialHz = target
      // Same bookkeeping as scanTick: keep centerHz consistent so the release path
      // (dialFromBoxCenter − off) round-trips to exactly this target.
      const off = cwDragOffHz()
      const edges = boxEdges(target + off, sidebandRef.current, W)
      g.centerHz = (edges.loHz + edges.hiHz) / 2
      positionBoxAtCursor(e.clientX, W)
      onTuneRef.current?.({ dialHz: Math.round(target), kind: 'drag' })
      return
    }
    // Normal drag (mid-view): the box follows the cursor's frequency directly.
    g.scanDialHz = null
    const hz = xToHz(e.clientX)
    if (hz == null) return
    const center = clampBoxCenterHz(hz, W, view.lo, view.hi)
    g.centerHz = center
    positionBox(center, W)
    onTuneRef.current?.({
      dialHz: Math.round(dialFromBoxCenter(center, sidebandRef.current, W) - cwDragOffHz()),
      kind: 'drag',
    })
  }
  const onPointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const g = dragRef.current
    if (!g) return
    if (g.click && e.pointerId !== g.pointerId) return
    const wasDragging = g.dragging
    const centerHz = g.centerHz
    endGesture()
    if (g.click && (g.moved || !interactiveRef.current || g.clickContext !== clickContext())) return
    // ⛔ THE SUB'S MARKER IS DISPLAY ONLY. A press on it that does not move tunes nothing: not the Sub,
    // not Main onto the signal under it, and no spot whose tag it crosses. A drag from it is the drag.
    if (g.sub && !g.moved) return
    if (g.spot && !g.moved) {
      // A tag clicked: work that spot, BandMap's action (the host QSYs to its frequency and prefills
      // the log), rather than snapping to the signal under it. Asked again at release, so a
      // transmitter keyed mid-press works nothing.
      if (spotsClickable()) onSpotRef.current?.(g.spot)
      return
    }
    if (g.edge && wasDragging) {
      // The last width rides the coalescer's pending flush, and stands on screen until the rig's
      // read-back agrees. An edge pressed and released WITHOUT moving falls through to the click.
      pendingWidthRef.current = { hz: g.edge.hz, until: performance.now() + PENDING_WIDTH_MS }
      if (canEditPassband()) onPassbandRef.current?.(g.edge.hz)
      overlayRef.current?.()
      return
    }
    if (wasDragging) {
      // Final position rides the coalescer's pending timer — latest target wins.
      // centerHz 0 = an audio-row drag that never reached an edge zone (no tune
      // was ever commanded): release quietly rather than tuning to nonsense.
      if (centerHz > 0) {
        const W = boxWidthFor(sidebandRef.current, filterWidthRef.current ?? null)
        onTuneRef.current?.({
          dialHz: Math.round(dialFromBoxCenter(centerHz, sidebandRef.current, W) - cwDragOffHz()),
          kind: 'drag',
        })
      }
      return
    }
    // Click: snap-detect the signal under the cursor and tune to work it.
    const hz = xToHz(e.clientX)
    const data = lastRowRef.current
    const view = lastViewRef.current
    const dial = dialRef.current
    if (hz == null || !data || !view || dial == null) return
    // A demodulated FM/AM baseband has no click→RF mapping — leave the dial alone.
    if (!view.rf && isSymmetricMode(sidebandRef.current)) return
    const r = clickTuneTarget({
      row: data.row,
      rowLoHz: data.rowLo,
      rowHiHz: data.rowHi,
      source: sourceRef.current,
      clickHz: hz,
      dialHz: dial,
      sideband: sidebandRef.current,
      pitchHz: pitchRef.current,
      cwPitchRefDial: cwPitchRefRef.current,
    })
    if (g.click) g.click(Math.round(r.dialHz))
    else onTuneRef.current?.({ dialHz: Math.round(r.dialHz), kind: 'click' })
  }

  // ---- The keys: one for every gesture, on the focused scope -----------------------------------
  //
  // ←/→ tune down/up, the box drag's path (a drag report, so the host coalesces it), a hundredth
  // of the view per press and ten with Shift — the edge scan's; Enter snaps onto the signal in the
  // passband, the click's; [ and ] narrow and widen the filter, the edge's; ↑/↓ scroll back while
  // paused, the wheel's. ⛔ NOT Esc (the shared stop listener's: this handler lets it pass and
  // never cancels it) and NOT Space (Phone's PTT); nor PageUp/PageDown, which are CW's speed
  // keys. Chords with Ctrl, Alt or ⌘ belong to the app.
  const onKeyDown = (e: React.KeyboardEvent<HTMLCanvasElement>) => {
    const chord = e.ctrlKey || e.altKey || e.metaKey
    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !chord) {
      if (!pausedRef.current) return
      e.preventDefault()
      // The wheel's own direction: ↓ is a wheel turned toward you.
      const back = newestAtTopRef.current ? e.key === 'ArrowDown' : e.key === 'ArrowUp'
      scrollBack(back ? 3 : -3)
      return
    }
    const view = lastViewRef.current
    if (!interactiveRef.current || !view) return
    if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !chord) {
      if (clickOnly || !onTuneRef.current) return // the Remote scope is click-only
      const now = performance.now()
      const k = keyDialRef.current
      const from = k && now - k.at < KEY_IDLE_MS ? k.hz : dialRef.current
      if (from == null || !(from > 0)) return
      e.preventDefault()
      const step = keyStepHz(view.hi - view.lo) * (e.shiftKey ? 10 : 1)
      // On the step's grid, the way a radio's own tuning step moves: off the grid, the first press
      // lands on the next grid point in its direction.
      const next = e.key === 'ArrowRight' ? Math.floor(from / step) * step + step : Math.ceil(from / step) * step - step
      if (!(next > 0)) return
      keyDialRef.current = { hz: next, at: now }
      onTuneRef.current({ dialHz: next, kind: 'drag' })
      return
    }
    if (e.key === 'Enter' && !chord) {
      // The click at the passband's centre: what the box is sitting on, snapped onto.
      const data = lastRowRef.current
      const dial = dialRef.current
      if (!data || dial == null) return
      if (!view.rf && isSymmetricMode(sidebandRef.current)) return
      const p = passbandOnAxis(axisKind(view.rf), boxWidthFor(sidebandRef.current, filterWidthRef.current ?? null))
      if (!p) return
      e.preventDefault()
      const centre = (p.lo + p.hi) / 2
      const r = clickTuneTarget({
        row: data.row,
        rowLoHz: data.rowLo,
        rowHiHz: data.rowHi,
        source: sourceRef.current,
        clickHz: view.mirrored ? -centre : centre,
        dialHz: dial,
        sideband: sidebandRef.current,
        pitchHz: pitchRef.current,
        cwPitchRefDial: cwPitchRefRef.current,
      })
      if (clickOnly) {
        if (!scopeAvailable) return
        onBeginClickRef.current?.()?.(Math.round(r.dialHz))
      } else onTuneRef.current?.({ dialHz: Math.round(r.dialHz), kind: 'click' })
      return
    }
    // The character, not the key position: [ and ] sit behind AltGr on many layouts.
    if (e.key === '[' || e.key === ']') {
      const width = shownWidth()
      const limits = cockpitRef.current === 'rfpan' ? null : PASSBAND_LIMITS[cockpitRef.current]
      if (!canEditPassband() || width == null || limits == null) return
      e.preventDefault()
      const next = stepPassband(width, e.key === ']' ? 1 : -1, limits)
      if (next === width) return
      pendingWidthRef.current = { hz: next, until: performance.now() + PENDING_WIDTH_MS }
      onPassbandRef.current?.(next)
      overlayRef.current?.()
    }
  }
  /** Keys belong to a scope the host made a tuning surface (native or click-only). */
  const keyable = onTune != null || clickOnly
  const passbandGrabbable = tunable && !clickOnly && onPassband != null && (passbandHz ?? 0) > 0

  // A mark's props changing between two sweeps (or while paused) redraws the marks now.
  useEffect(() => {
    overlayRef.current?.()
  }, [passbandHz, notchHz, subReceiver, onPassband, interactive])

  // Real CAT S-meter (dB rel S9). Absent when the rig doesn't report STRENGTH, or during
  // TX (STRENGTH is RX-only) → the meter reads "—" rather than faking a level.
  const sm = smeterDb != null && !transmitting ? sMeterDisplay(smeterDb) : null
  const smColor =
    sm == null
      ? undefined
      : sm.zone === 'hot'
        ? 'var(--state-weak)'
        : sm.zone === 'warn'
          ? // --alert-warning, not --state-weak: the latter is the sheet's RED, so the warn
            // band painted the same colour as hot (see the note on TxMeters' ZONE_COLOR).
            'var(--alert-warning)'
          : 'var(--state-good)'
  return (
    <div className="ph-scope">
      <div
        className="ph-scope-smeter"
        title={
          sm
            ? t('scope.smeter.title', { reading: sm.label, db: smeterDb ?? 0 })
            : transmitting
              ? t('scope.smeter.title.tx')
              : t('scope.smeter.title.none')
        }
      >
        {/* ⚠️ ONLY THESE THREE ARE THE S-METER. The strip they sit in is `ph-scope-smeter` but
            it also carries the RF-source badge, the dynamic-range readout and the G/Z gain
            control — hiding the strip would take those with it, which is a different and much
            worse change than the one asked for. Phone hides the reading because its analog
            meter shows the same number on the same screen; CW keeps it. */}
        {!hideSmeter && (
          <>
            <span className="ph-scope-smeter-label">S</span>
            {/* A display well (styles.css DISPLAY WELLS): the bar only — the strip around it
                carries controls and stays on the frame. */}
            <div className="ph-scope-smeter-track well">
              <div
                className="ph-scope-smeter-fill"
                style={{ width: sm ? `${Math.round(sm.frac * 100)}%` : '0%', background: smColor }}
              />
            </div>
            <span className="ph-scope-smeter-label ph-scope-smeter-value">{sm ? sm.label : '—'}</span>
          </>
        )}
        {(source === 'flex' || source === 'civ') && (
          <span
            className="ph-scope-src"
            title={
              source === 'flex' ? t('scope.source.flex.title') : t('scope.source.civ.title')
            }
          >
            {source === 'flex' ? FLEX_RF : CIV_RF}
          </span>
        )}
        {spanDb != null && (
          <span
            className="ph-scope-dyn"
            title={t('scope.dynamic.title')}
          >
            ▲{spanDb} {DB}
          </span>
        )}
        <label className="ph-scope-gz" title={t('scope.gain.title')}>
          G
          <WheelRange
            wheelStep={wheelSliders ? 0.05 : undefined}
            type="range"
            min={-1}
            max={1}
            step={0.05}
            value={scale.gain}
            onChange={(e) => setScale({ gain: Number(e.target.value) })}
            aria-label={t('scope.gain.aria')}
          />
        </label>
        <label className="ph-scope-gz" title={t('scope.zero.title')}>
          Z
          <WheelRange
            wheelStep={wheelSliders ? 0.05 : undefined}
            type="range"
            min={-1}
            max={1}
            step={0.05}
            value={scale.zero}
            onChange={(e) => setScale({ zero: Number(e.target.value) })}
            aria-label={t('scope.zero.aria')}
          />
        </label>
        {/* The ⚙ strip's toggle: the analysis window, averaging, detector and slow-scope look are
            set once and left, so they sit one click away instead of crowding this row. */}
        <button
          type="button"
          className={`ph-scope-btn${gear ? ' on' : ''}`}
          aria-expanded={gear}
          aria-controls={gearId}
          aria-label={t('scope.gear.aria')}
          title={t('scope.gear.title')}
          onClick={() => setGear(!gear)}
        >
          ⚙
        </button>
        <button
          type="button"
          className={`ph-scope-btn${dss ? ' on' : ''}`}
          aria-pressed={dss}
          title={dss ? t('scope.dss.on.title') : t('scope.dss.off.title')}
          onClick={() => {
            const next = !dss
            setDss(next)
            dssRef.current = next
            surfaceSet(PHSCOPE_DSS_KEY, next ? '1' : '0')
            rebuildRef.current?.()
          }}
        >
          {dss ? '▤' : '◭'}
        </button>
        <button
          type="button"
          className={`ph-scope-btn${paused ? ' on' : ''}`}
          aria-pressed={paused}
          title={paused ? t('scope.pause.resume.title') : t('scope.pause.title')}
          onClick={() => {
            const next = !paused
            setPaused(next)
            pausedRef.current = next
            if (!next) offsetRef.current = 0
            rebuildRef.current?.()
          }}
        >
          {paused ? '▶' : '⏸'}
        </button>
        <button
          type="button"
          className={`ph-scope-btn${!newestAtTop ? ' on' : ''}`}
          aria-pressed={!newestAtTop}
          title={newestAtTop ? t('scope.flow.down.title') : t('scope.flow.up.title')}
          onClick={() => {
            const next = !newestAtTop
            setNewestAtTop(next)
            newestAtTopRef.current = next
            surfaceSet(PHSCOPE_FLOW_KEY, next ? 'down' : 'up')
            rebuildRef.current?.()
          }}
        >
          {newestAtTop ? t('scope.flow.down.label') : t('scope.flow.up.label')}
        </button>
      </div>
      {gear && (
        <div className="ph-scope-gear" id={gearId}>
          {/* No analysis window on the RF pane: its rows are the radio's own sweep, not an FFT here. */}
          <ScaleStrip settings={scale} onChange={setScale} windowControl={!rfOnly} control={control} />
          <button
            type="button"
            className={`ph-scope-btn${rowCadence === 'sweep' ? ' on' : ''}`}
            aria-pressed={rowCadence === 'sweep'}
            title={rowCadence === 'sweep' ? t('scope.rows.sweep.title') : t('scope.rows.smooth.title')}
            onClick={() => {
              const next: RowCadence = rowCadence === 'sweep' ? 'smooth' : 'sweep'
              setRowCadence(next)
              rowCadenceRef.current = next
              surfaceSet(PHSCOPE_ROWS_KEY, next)
            }}
          >
            {rowCadence === 'sweep' ? t('scope.rows.sweep.label') : t('scope.rows.smooth.label')}
          </button>
        </div>
      )}
      <div className="ph-scope-canvas-wrap">
        {/* The renderer's canvas goes here, under the overlay; it never takes a pointer event. */}
        <div
          ref={renderHostRef}
          className="ph-scope-render"
          style={{ visibility: scopeAvailable ? undefined : 'hidden' }}
          aria-hidden="true"
        />
        {/* The overlays: data-only layers over the picture and under the marks, taking no pointer
            event. A display well, so what it draws on the dark floor takes the dark palette's inks. */}
        <canvas
          ref={overlaysRef}
          className="ph-scope-overlays well"
          style={{ visibility: scopeAvailable ? undefined : 'hidden' }}
          aria-hidden="true"
        />
        <canvas
          ref={canvasRef}
          className={`ph-scope-canvas${tunable ? ' tunable' : ''}`}
          style={{ visibility: scopeAvailable ? undefined : 'hidden' }}
          title={
            tunable
              ? clickOnly
                ? t('remote.scopeClick')
                : passbandGrabbable
                  ? t('scope.canvas.edges.title')
                  : t('scope.canvas.title')
              : undefined
          }
          // An instrument the keys can drive: `application`, so a screen reader hands the arrows to
          // it rather than reading the page with them.
          tabIndex={keyable ? 0 : undefined}
          role={keyable ? 'application' : undefined}
          aria-label={keyable ? t('scope.canvas.keys.aria') : undefined}
          aria-keyshortcuts={keyable ? SCOPE_KEYS : undefined}
          onKeyDown={keyable ? onKeyDown : undefined}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={endGesture}
          onLostPointerCapture={clickOnly ? endGesture : undefined}
          onWheel={(e) => {
            // Only in pause/review mode: wheel up = back in time, down = toward live.
            if (!pausedRef.current) return
            // Wheel-back follows the scroll direction, exactly as the FT8 waterfall's does.
            const back = newestAtTopRef.current ? e.deltaY > 0 : e.deltaY < 0
            scrollBack(back ? 3 : -3)
          }}
        />
        {!scopeAvailable && (
          <div className="ph-scope-paused" role="status">
            {rfOnly ? t('scope.rf.none') : t('remote.scopeUnavailable')}
          </div>
        )}
        {paused && <div className="ph-scope-paused">{t('scope.paused.badge')}</div>}
        {/* The drag passband box — imperatively positioned (60 fps), never intercepts events. */}
        <div ref={boxRef} className="ph-scope-box" aria-hidden="true" />
      </div>
    </div>
  )
}

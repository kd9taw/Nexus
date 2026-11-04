// THE ROTOR BOX IN THE DASHBOARD RAIL GROWS INTO ITS BOX. A short box keeps the compact layout
// (styles.css `.dash-boxes … .rotor-*`: a small rose beside the bearing, the boxes and ■ STOP, so all of
// them show without scrolling); a taller or wider one draws the rose and the readings as big as the box
// holds, centred, the way the Clock box's digits fill theirs. The rose is the pane's SVG, whose viewBox
// scales the drawing (ClockPane's mechanism); what the stylesheet cannot know is the box's HEIGHT.
//
// A MEASURED SIZE, NEVER A QUERY (useRegionCols' reasons: no size-based @media, and no container
// queries). The box's body is measured — its size is the rail's split and width, never this pane's
// content, so a stamp cannot feed back into the measure — and so are the parts that keep their size: the
// boxes and STOP, line 2, the hint, and the compact layout's own rose, bearing and elevation, whose sizes
// stay the floors. The pane is stamped with the arrangement (`data-fit`), the rose's size and the readings'
// font sizes, and styles.css draws them. What is drawn is then measured: an arrangement that comes out
// taller than its room and than the compact one, or wider than the box, is taken back.
//
// TWO ARRANGEMENTS, whichever draws the bearing bigger (then the bigger rose):
//   · 'beside' — the compact layout grown: the rose beside the bearing, the elevation, the boxes and STOP.
//     It wins in a box that is short for its width.
//   · 'under' — the rose beside the bearing and the elevation, the boxes and STOP under them. In a box
//     about as tall as it is wide the boxes beside the rose would cap it at the width they leave.
//
// ■ STOP STAYS PUT. Under the arrangement, room is held for line 2 and the hint whether or not a call is in
// the entry: line 2 as tall as the tallest this box has shown at this width and text size (STOP's height
// before any), so a call coming or going moves nothing, and only a line 2 taller than any before (a longer
// call, a narrower rail) takes the room once. A slew's "→" line is not held for: it goes under the hint,
// and the body holds its scrollbar's gutter while the pane is grown, so nothing centred moves sideways
// when the box starts to scroll. A box that holds no bigger rose than the compact one is left compact.
import { useLayoutEffect, type RefObject } from 'react'

/** What the arrangement is computed from, in CSS px: the box's content area, and the parts that keep
 *  their size whatever the rose's. */
export interface RotorRoom {
  /** The body's content width and height. */
  w: number
  h: number
  /** The bearing box, the elevation box (an az/el rotator) and ■ STOP on one line, and its height. */
  controlsW: number
  controlsH: number
  /** The height held for line 2, and the hint's, under the arrangement. */
  line2H: number
  hintH: number
  /** The compact layout's bearing and elevation font sizes (0 with no elevation axis): the floors. */
  azMin: number
  elMin: number
  /** The pane grid's gaps. */
  colGap: number
  rowGap: number
}

export interface RotorFit {
  shape: 'beside' | 'under'
  /** The height above line 2 and the hint, which the arrangement is centred in. */
  room: number
  /** The rose's diameter. */
  rose: number
  /** The bearing's and the elevation's font sizes. */
  az: number
  el: number
}

/** The bearing grows with the rose: a fifth of its diameter. "123°T" is five monospace characters, three
 *  of the font size wide, so it never takes more than the column beside the rose leaves (3.3, with room). */
const AZ_PER_ROSE = 0.2
const AZ_FITS = 3.3
/** The elevation a step smaller; "EL 180°" is seven characters, 4.2 of the font size (4.4, with room). */
const EL_PER_ROSE = 0.14
const EL_FITS = 4.4
/** With the boxes and STOP under it, the rose takes at most this share of the width; the readings the rest. */
const UNDER_ROSE_SHARE = 0.6

/** The arrangement this box holds, or null when it holds nothing (a box that has not been laid out). */
export function rotorFit(r: RotorRoom): RotorFit | null {
  if (r.w <= 0 || r.h <= 0) return null
  const room = r.h - (2 * r.rowGap + r.line2H + r.hintH)
  // The narrowest column the readings take beside the rose, at their floors.
  const readings = Math.max(AZ_FITS * r.azMin, EL_FITS * r.elMin)
  const shape = (s: RotorFit['shape'], most: number): RotorFit => {
    const rose = Math.max(0, Math.floor(most))
    const col = r.w - r.colGap - rose
    return {
      shape: s,
      room,
      rose,
      az: Math.floor(Math.max(r.azMin, Math.min(rose * AZ_PER_ROSE, col / AZ_FITS))),
      el: Math.floor(Math.max(r.elMin, Math.min(rose * EL_PER_ROSE, col / EL_FITS))),
    }
  }
  const beside = shape('beside', Math.min(r.w - r.colGap - Math.max(r.controlsW, readings), room))
  const under = shape(
    'under',
    Math.min((r.w - r.colGap) * UNDER_ROSE_SHARE, r.w - r.colGap - readings, room - r.rowGap - r.controlsH),
  )
  return under.az > beside.az || (under.az === beside.az && under.rose > beside.rose) ? under : beside
}

const VARS = ['--rotor-rose', '--rotor-az', '--rotor-el', '--rotor-top'] as const

function unstamp(pane: HTMLElement): void {
  pane.removeAttribute('data-fit')
  for (const v of VARS) pane.style.removeProperty(v)
}

/** The tallest line 2 each pane has shown, and the width and text size it was shown at. */
const LINE2 = new WeakMap<HTMLElement, { at: string; h: number }>()

const px = (v: string) => parseFloat(v) || 0

/** Measure the box and stamp the pane, or leave it compact. */
export function fitRotor(pane: HTMLElement): void {
  // The compact layout is what is measured first, so a stamp is never measured as room.
  unstamp(pane)
  const body = pane.closest<HTMLElement>('.dash-boxes .pane-body')
  const row = pane.querySelector<HTMLElement>('.rotor-row')
  const rose = pane.querySelector<SVGSVGElement>('.rotor-rose')
  const az = pane.querySelector<HTMLElement>('.rotor-az')
  const entry = pane.querySelector<HTMLElement>('.rotor-entry')
  const hint = pane.querySelector<HTMLElement>('.rotor-hint')
  if (!body || !row || !rose || !az || !entry || !hint || body.offsetHeight === 0) return
  const el = pane.querySelector<HTMLElement>('.rotor-el')
  const b = getComputedStyle(body)
  const g = getComputedStyle(row)
  const controls = [...entry.children] as HTMLElement[]
  // A client rect is in the window's px under the app's zoom, the rest in the box's own CSS px.
  const zoom = body.getBoundingClientRect().height / body.offsetHeight || 1
  const tallOf = () => {
    const parts = [rose, az, ...(el ? [el] : []), entry].map((p) => p.getBoundingClientRect())
    return (Math.max(...parts.map((p) => p.bottom)) - Math.min(...parts.map((p) => p.top))) / zoom
  }
  const floor = rose.getBoundingClientRect().width / zoom
  const compactTall = tallOf()
  const fixed = {
    controlsW:
      controls.reduce((w, c) => w + c.offsetWidth, 0) + px(getComputedStyle(entry).columnGap) * (controls.length - 1),
    controlsH: Math.max(...controls.map((c) => c.offsetHeight)),
    azMin: px(getComputedStyle(az).fontSize),
    elMin: el ? px(getComputedStyle(el).fontSize) : 0,
    colGap: px(g.columnGap),
    rowGap: px(g.rowGap),
  }
  // What the box holds: its content area as it stands (compact first, then with the scrollbar gutter a
  // grown pane's body holds), and the lines that wrap to that width.
  const measure = (): RotorFit | null => {
    const w = body.clientWidth - px(b.paddingLeft) - px(b.paddingRight)
    const aim = pane.querySelector<HTMLElement>('.rotor-aim')
    // Keyed by the body's own width, the same with the gutter or without, and the text size.
    const at = `${body.offsetWidth}:${fixed.azMin}`
    const seen = LINE2.get(pane)
    const line2 = Math.max(seen?.at === at ? seen.h : 0, aim?.offsetHeight ?? 0)
    if (aim) LINE2.set(pane, { at, h: line2 })
    return rotorFit({
      ...fixed,
      w,
      h: body.clientHeight - px(b.paddingTop) - px(b.paddingBottom),
      line2H: Math.max(fixed.controlsH, line2),
      // The Moon line (the Rotor pane's ☾) stands under the controls with the hint, so it is room the
      // rose cannot have either; it is there only while the station's grid places the Moon.
      hintH: hint.offsetHeight + (pane.querySelector<HTMLElement>('.rotor-moon')?.offsetHeight ?? 0),
    })
  }
  const first = measure()
  // No bigger than the compact rose: the compact layout is the one that fits a short box.
  if (!first || first.rose <= floor) return
  pane.setAttribute('data-fit', first.shape)
  const fit = measure()
  if (!fit || fit.rose <= floor) return unstamp(pane)
  pane.setAttribute('data-fit', fit.shape)
  pane.style.setProperty('--rotor-rose', `${fit.rose}px`)
  pane.style.setProperty('--rotor-az', `${fit.az}px`)
  pane.style.setProperty('--rotor-el', `${fit.el}px`)
  // Drawn: taken back if it is taller than its room and than the compact layout, or wider than the box.
  const tall = tallOf()
  if ((tall > fit.room + 0.5 && tall > compactTall + 0.5) || row.scrollWidth > row.clientWidth + 0.5) return unstamp(pane)
  // Centred in the room above line 2 and the hint.
  pane.style.setProperty('--rotor-top', `${Math.max(0, Math.floor((fit.room - tall) / 2))}px`)
}

/** Keep the pane's arrangement in step with its box. `shown` is whether the pane's controls are rendered. */
export function useRotorFit(pane: RefObject<HTMLElement | null>, shown: boolean): void {
  // After every render: a reading, the elevation axis or line 2 may have changed the parts.
  useLayoutEffect(() => {
    if (pane.current) fitRotor(pane.current)
  })
  // And on every resize, which renders nothing: of the box (its divider, the rail's width), and of the
  // hint and STOP, whose sizes follow Text size and the box's own A−/A+. Only in the dashboard rail.
  useLayoutEffect(() => {
    const el = pane.current
    const body = el?.closest<HTMLElement>('.dash-boxes .pane-body')
    if (!el || !body || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => fitRotor(el))
    for (const t of [body, el.querySelector('.rotor-hint'), el.querySelector('.rotor-stop')]) if (t) ro.observe(t)
    return () => ro.disconnect()
  }, [pane, shown])
}

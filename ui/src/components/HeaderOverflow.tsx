// THE HEADER'S ⋯ (the 1080p pass, 2026-11-01): the CW and Phone headers' occasional controls —
// memories, the rotator, split and REC — behind one button at md and lg (features/compactChrome), so
// the header is one row there. At xl, and below md, the cockpits draw those controls inline as they
// always did and never mount this.
//
// A DISCLOSURE, NOT A MENU. What it holds are live controls with their own state and their own
// popups (a memory menu, the split offset, the rotor's heading), not commands, so it is a button with
// `aria-expanded` that shows a group of them under it; the controls inside keep their own names and
// their own keyboard. The group stays MOUNTED while closed (`hidden`), so opening it never resets a
// split offset or a rotor reading, and a hidden control is out of the tab order.
//
// The keyboard: Enter or Space on the button opens it and moves focus to the first control inside
// (`preventScroll`, so the cockpit does not jump); Escape inside closes it and puts focus back on
// the button. ⛔ ESCAPE IS NOT SWALLOWED: CW's Esc is a stop (the window's keydown → abort), and it
// must still reach the window from inside this group. A press outside closes it, except inside a
// popup one of the controls opened (it portals out of the group).
//
// WHICH WAY IT OPENS (a beta tester's report): on the side with room, RIGHTWARD FIRST. Always right-aligned
// to the button, the group opened off the window's left edge (−441 px at 1920x1080 with the Dashboard rail)
// whenever the header wrapped and put ⋯ at the start of its second line. On open, and on a window resize
// while open, `placeOverflow` compares the button's box, the group's width and the cockpit's own box
// (`main`, so the mode bar and the window's edges both bound it): rightward if it fits there, else
// leftward, else slid back inside from its start. The sides themselves are cockpit-panes.css's
// (`.ch-overflow-start`); this only picks one, and the one offset it writes is converted to the group's
// own (zoomed) units from its measured box, so `--ui-zoom` needs no arithmetic here.
//
// ⛔ NOTHING THAT KEYS OR STOPS A TRANSMISSION MAY BE PASSED HERE. PTT, Stop TX, Tune, the TX latch,
// Send and the macro keys live in the dock and the TX strip, outside every fold (THE STOP LINE,
// cockpit-panes.css); split is a configuration control (SplitControl's header says why it is not a
// stop), and REC records audio.
import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'

const MORE = '⋯'

/** A horizontal extent on screen, in viewport px. */
export interface Span {
  left: number
  right: number
}

/**
 * Where the group opens, from the button's box, the group's width and the box it must stay inside (all
 * viewport px): `start` (its left edge on the button's, opening rightward) when that fits, else `end`
 * (its right edge on the button's, as before) when that fits, else `start` slid back by `offset` px so
 * its far edge meets the bound's, never past the bound's start.
 */
export function placeOverflow(anchor: Span, panelW: number, bound: Span): { side: 'start' | 'end'; offset: number } {
  if (anchor.left + panelW <= bound.right) return { side: 'start', offset: 0 }
  if (anchor.right - panelW >= bound.left) return { side: 'end', offset: 0 }
  return { side: 'start', offset: Math.max(bound.left, bound.right - panelW) - anchor.left }
}

export function HeaderOverflow({
  label,
  title,
  badges,
  children,
}: {
  /** The button's accessible name and the group's. */
  label: string
  title: string
  /** What is ON behind the button (REC while recording, SPLIT while split): shown on the button, so a
   *  state the operator would otherwise see in the header is never folded out of sight. */
  badges?: ReactNode
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const id = useId()
  const button = useRef<HTMLButtonElement>(null)
  const group = useRef<HTMLDivElement>(null)
  const focusFirst = useRef(false)
  const wrap = useRef<HTMLDivElement>(null)
  /** The side it opens on, and the slide in the group's own CSS px (null: not yet measured, `end`). */
  const [place, setPlace] = useState<{ side: 'start' | 'end'; offset: number } | null>(null)

  useLayoutEffect(() => {
    if (!open) return
    const measure = () => {
      const panel = group.current
      const box = wrap.current
      if (!panel || !box) return
      const a = box.getBoundingClientRect()
      const p = panel.getBoundingClientRect()
      if (p.width === 0) return
      const main = box.closest('main')?.getBoundingClientRect()
      const bound = { left: Math.max(0, main?.left ?? 0), right: Math.min(window.innerWidth, main?.right ?? window.innerWidth) }
      const next = placeOverflow(a, p.width, bound)
      // The slide is written in the group's own units: its laid-out width against its width on screen.
      const cs = getComputedStyle(panel)
      const px = (v: string) => parseFloat(v) || 0
      const laidOut = px(cs.width) + px(cs.paddingLeft) + px(cs.paddingRight) + px(cs.borderLeftWidth) + px(cs.borderRightWidth)
      const offset = next.offset && laidOut > 0 ? (next.offset * laidOut) / p.width : 0
      setPlace((old) => (old && old.side === next.side && Math.abs(old.offset - offset) < 0.5 ? old : { side: next.side, offset }))
    }
    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  }, [open])

  useEffect(() => {
    if (!open) return
    if (focusFirst.current) {
      focusFirst.current = false
      const first = group.current?.querySelector<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      )
      first?.focus({ preventScroll: true })
    }
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null
      if (!target) return
      if (group.current?.contains(target) || button.current?.contains(target)) return
      // A popup a control in the group opened (a Radix menu, a dialog) portals out of the group.
      if (target.closest?.('[data-radix-popper-content-wrapper], [role="menu"], .ui-dialog')) return
      setOpen(false)
    }
    document.addEventListener('pointerdown', onDown, true)
    return () => document.removeEventListener('pointerdown', onDown, true)
  }, [open])

  return (
    <div ref={wrap} className="ch-overflow">
      <button
        ref={button}
        type="button"
        className={`ch-overflow-btn${open ? ' on' : ''}`}
        aria-expanded={open}
        aria-controls={id}
        aria-label={label}
        title={title}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          // Esc on the button itself closes an open group too (opened by the mouse, focus stays here).
          // Not swallowed, for the reason the group's own Esc is not.
          if (open && e.key === 'Escape') {
            setOpen(false)
            return
          }
          // A keyboard open goes INTO the group; a click leaves focus where the pointer put it.
          if (!open && (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown')) {
            focusFirst.current = true
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setOpen(true)
            }
          }
        }}
      >
        <span aria-hidden="true">{MORE}</span>
        {badges}
      </button>
      <div
        ref={group}
        id={id}
        role="group"
        aria-label={label}
        className={`ch-overflow-panel${open ? ' ch-overflow-open' : ''}${place?.side === 'start' ? ' ch-overflow-start' : ''}`}
        style={place?.offset ? ({ '--ch-overflow-offset': `${place.offset}px` } as CSSProperties) : undefined}
        hidden={!open}
        onKeyDown={(e) => {
          if (e.key !== 'Escape') return
          setOpen(false)
          button.current?.focus({ preventScroll: true })
        }}
      >
        {children}
      </div>
    </div>
  )
}

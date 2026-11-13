// THE RECALL CARD, OPENED IN FULL — the callsign card's ⤢ (tester report, 2026-11-13: "I want to see the
// whole card without scrolling"). In the Operate rail the card is capped (cockpit-panes.css `.cockpit-recall`)
// so it cannot take the rail from the feeds beside it; this is where the WHOLE card goes instead: the photo
// larger, the name, call and place, the badges, the note and every prior contact, in one floating box over
// the pane. RecallPanel renders its own content in here (`full`); this file only places and closes the box.
//
// ⛔ IT NEVER MOVES, COVERS OR RESIZES A TRANSMIT CONTROL. It is placed by the station card's own rule
// (CallCard `placeCallCard` + `txBoxes`, the same pair the Best times card uses), which keeps it off every
// visible TX strip, TX dock, FT QSO strip and Stop button (features/txSurfaces `TX_AVOID`), and gives it a
// shorter, scrolling box where the room is short. It is portaled to <body> and position:fixed, so it is out
// of every layout: nothing in the cockpit moves when it opens. Geometry, so jsdom cannot see it:
// `scripts/browser-probe --desktop recallcard` opens it in the real app and asks `elementFromPoint` at
// Stop TX and across the dock.
//
// CLOSING: Esc, its own ✕, a press anywhere outside it, or the ⤢ again. Esc is also the stop key on every
// operating screen (useEscStop.ts, capture phase): this listener never prevents or stops the key, so the
// same press that closes the card still halts TX, exactly as with the station card and every dialog.
//
// ONE FLOATING CARD AT A TIME (features/floatingCard): opening this closes the station card and the Best
// times card, and opening either of them closes this.
//
// Its content re-applies `--ui-zoom` (the box is placed in real window pixels), as the station card's does.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): its one word, the close button's name, is the catalog's.
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { CSSProperties, ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { placeCallCard, txBoxes, type Placement } from './CallCard'
import { registerFloatingCard } from '../features/floatingCard'
import { planReturn, giveBack } from '../focusReturn'
import { t } from '../i18n'

/** The open card's close, if one is open: the floating-card registry closes it when another card opens. */
let closeOpen: (() => void) | null = null
const closeOtherCards = registerFloatingCard(() => closeOpen?.())

export function RecallCardPopover({
  anchor,
  keyboard,
  label,
  onClose,
  children,
}: {
  /** The ⤢ it was opened from: the card stands beside it, and a press on it is not "outside". */
  anchor: HTMLElement
  /** Opened from the keyboard: the card takes the keyboard (without scrolling anything) and gives it back. */
  keyboard: boolean
  /** The dialog's accessible name. */
  label: string
  onClose: () => void
  children: ReactNode
}) {
  const box = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  const [place, setPlace] = useState<Placement | null>(null)
  const focusInside = useRef(false)

  useEffect(() => {
    closeOtherCards()
    closeOpen = onClose
    return () => {
      if (closeOpen === onClose) closeOpen = null
    }
  }, [onClose])

  useLayoutEffect(() => {
    const replace = () => {
      if (!anchor.isConnected) return onClose()
      const a = anchor.getBoundingClientRect()
      const c = inner.current?.getBoundingClientRect()
      if (!c || !box.current) return
      const b = getComputedStyle(box.current)
      const bw = (parseFloat(b.borderLeftWidth) || 0) + (parseFloat(b.borderRightWidth) || 0)
      const bh = (parseFloat(b.borderTopWidth) || 0) + (parseFloat(b.borderBottomWidth) || 0)
      const p = placeCallCard(
        { left: a.left, top: a.top, right: a.right, bottom: a.bottom },
        { w: c.width + bw, h: c.height + bh },
        { w: window.innerWidth, h: window.innerHeight },
        txBoxes(),
      )
      setPlace((old) => (old && p && old.left === p.left && old.top === p.top && old.width === p.width && old.maxHeight === p.maxHeight ? old : p))
    }
    replace()
    window.addEventListener('resize', replace)
    window.addEventListener('scroll', replace, true)
    const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(replace) : null
    if (inner.current) ro?.observe(inner.current)
    return () => {
      window.removeEventListener('resize', replace)
      window.removeEventListener('scroll', replace, true)
      ro?.disconnect()
    }
  }, [anchor, onClose])

  const placed = !!place
  useEffect(() => {
    if (keyboard && placed) box.current?.focus({ preventScroll: true })
  }, [keyboard, placed])
  useEffect(() => {
    return () => {
      if (focusInside.current && anchor.isConnected) giveBack(planReturn(anchor, false))
    }
  }, [anchor])
  // Esc closes it (never prevented: it is the stop key too); a press outside it closes it. A press on the
  // ⤢ is the ⤢'s own (it toggles), so it is not "outside".
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    const onDown = (e: PointerEvent) => {
      const at = e.target as Node | null
      if (at && (box.current?.contains(at) || anchor.contains(at))) return
      onClose()
    }
    window.addEventListener('keydown', onKey)
    document.addEventListener('pointerdown', onDown, true)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('pointerdown', onDown, true)
    }
  }, [anchor, onClose])

  const style: CSSProperties = place
    ? { left: place.left, top: place.top, maxWidth: place.width, maxHeight: place.maxHeight }
    : { left: 0, top: 0, visibility: 'hidden' }
  return createPortal(
    <div
      ref={box}
      className="callcard recall-full"
      role="dialog"
      aria-modal="false"
      aria-label={label}
      data-placed={place ? 'yes' : 'no'}
      tabIndex={-1}
      style={style}
      onFocus={() => {
        focusInside.current = true
      }}
      onBlur={(e) => {
        if (!box.current?.contains(e.relatedTarget as Node | null)) focusInside.current = false
      }}
    >
      <div ref={inner} className="recall-full-body" style={{ zoom: 'var(--ui-zoom, 1)' }}>
        <button type="button" className="recall-full-close" aria-label={t('recall.full.close')} title={t('recall.full.close')} onClick={onClose}>
          ✕
        </button>
        {children}
      </div>
    </div>,
    document.body,
  )
}

//! A double-click acts on the row its FIRST click landed on.
//!
//! The operator's report (1.25.0): a double-click on the Call Roster worked the row above or below.
//! The first click selected the row; something then moved the list (the roster's filter bar
//! re-wrapping, a period's decodes landing in a feed that follows its newest line, a selected card
//! growing its path line), so the second click, and the double-click, landed on whatever had slid
//! under the pointer, and that station was worked. A double-click is ONE gesture, aimed at what was
//! under the pointer when it began. So the gesture's first press is recorded and the gesture acts on
//! it: a later click of the same gesture (the browser's click count, `detail`, above 1) on another
//! row does not re-select, and the double-click takes the recorded row, or does nothing if it has
//! left the list in between. Acting on it rather than dropping the gesture: it is the station the
//! operator aimed at, the one the first click already selected, so the selection and the work agree.
//!
//! EVERY GESTURE STARTS AFRESH, ROW OR NOT, WHEREVER IT BEGINS. Every first press (detail 1) anywhere in
//! the document clears the record, in the capture phase, before anything else sees it: a gesture begun
//! outside the list (the roster's filter bar, the Stations header) whose second click lands on a row
//! that moved under it then acts on nothing. The record is then taken by the LIST, in the capture
//! phase, on the press and the click of every gesture's first click anywhere inside it, before any
//! row handler runs and whatever a child does with the event afterwards. A first click that landed on
//! no row (blank space, a gap between rows) or on a control inside a row (the QRZ link, a menu) records
//! NO row, and the double-click that follows it does nothing: otherwise a record from an earlier,
//! finished gesture would be acted on (a review finding against the first version of this rule). A
//! row says it is one with `firstClickRow(key)`; a control inside a row that IS the row for this purpose
//! (a card's own open button) carries the same props. A card's Work button does not: it works on its
//! own first click, so a double-click begun on it has nothing left to do.
//!
//! Used by every list that works a station on a double-click: the Call Roster, Band Activity and
//! Rx Frequency (keyed by decode entry, since one call can have many rows), and the Stations cards.
import { useEffect, useRef, type MouseEvent } from 'react'

/** The attribute a row (or a control that stands for its row) carries its key in. */
const ATTR = 'data-first-click'
/** What a first press can land on: a row, or a control that is not one (and so begins no row). */
const HIT = `[${ATTR}], button, a, input, select, textarea, [role=menu]`

export interface FirstClick<T> {
  /** Spread on the list's container: records each gesture's first press, row or not. */
  containerProps: {
    onMouseDownCapture: (e: MouseEvent) => void
    onClickCapture: (e: MouseEvent) => void
  }
  /** A row's click. True when it should act (select); false for a later click of a gesture that did
   *  not begin on this row. `detail` is the event's click count (1 begins a gesture; jsdom's 0 too). */
  click: (key: string, detail: number) => boolean
  /** The row a double-click on `key` acts on: the gesture's first row, looked up in the list as it
   *  stands now; undefined when the gesture began on no row or that row has left the list. */
  double: (key: string) => T | undefined
}

/** Spread on a row, and on a control inside a row that stands for that row. A plain function, so a
 *  memoised row that never sees the hook can carry it. */
export function firstClickRow(key: string): { [ATTR]: string } {
  return { [ATTR]: key }
}

/** `lookup` finds a row by key in the list as currently drawn. */
export function useFirstClick<T>(lookup: (key: string) => T | undefined): FirstClick<T> {
  // undefined: no press seen yet (only a test's bare double-click); null: the gesture began on no row.
  const first = useRef<string | null | undefined>(undefined)
  useEffect(() => {
    const clear = (e: globalThis.MouseEvent) => {
      if (e.detail <= 1) first.current = null
    }
    document.addEventListener('mousedown', clear, true)
    return () => document.removeEventListener('mousedown', clear, true)
  }, [])
  const begin = (e: MouseEvent) => {
    if (e.detail > 1) return
    const hit = (e.target as Element | null)?.closest?.(HIT)
    first.current = hit?.getAttribute(ATTR) ?? null
  }
  return {
    containerProps: { onMouseDownCapture: begin, onClickCapture: begin },
    click: (key, detail) => {
      if (detail <= 1) first.current = key
      return first.current === key
    },
    double: (key) => {
      const f = first.current === undefined ? key : first.current
      return f == null ? undefined : lookup(f)
    },
  }
}

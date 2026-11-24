//! A double-click acts on the row its FIRST click landed on.
//!
//! The operator's report (1.25.0): a double-click on the Call Roster worked the row above or below.
//! The first click selected the row; something then moved the list (the roster's filter bar
//! re-wrapping, a period's decodes landing in a feed that follows its newest line, a selected card
//! growing its path line), so the second click, and the double-click, landed on whatever had slid
//! under the pointer, and that station was worked. A double-click is ONE gesture, aimed at what was
//! under the pointer when it began. So the first click's row is recorded and the gesture acts on it:
//! a later click of the same gesture (the browser's click count, `detail`, above 1) on another row
//! does not re-select, and the double-click takes the recorded row, or does nothing if it has left the
//! list in between. Acting on it rather than dropping the gesture: it is the station the operator aimed
//! at, the one the first click already selected, so the selection and the work agree.
//!
//! Used by every list that works a station on a double-click: the Call Roster, Band Activity and
//! Rx Frequency (keyed by decode entry, since one call can have many rows), and the Stations cards.
import { useRef } from 'react'

export interface FirstClick<T> {
  /** A row's click. True when it should act (select); false for a later click of a gesture begun
   *  on another row. `detail` is the event's click count (1 begins a gesture; jsdom's 0 does too). */
  click: (key: string, detail: number) => boolean
  /** The row a double-click on `key` acts on: the gesture's first row, looked up in the list as it
   *  stands now; undefined when that row has left it. */
  double: (key: string) => T | undefined
}

/** `lookup` finds a row by key in the list as currently drawn. */
export function useFirstClick<T>(lookup: (key: string) => T | undefined): FirstClick<T> {
  const first = useRef<string | null>(null)
  return {
    click: (key, detail) => {
      if (detail <= 1 || first.current == null) first.current = key
      return first.current === key
    },
    double: (key) => lookup(first.current ?? key),
  }
}

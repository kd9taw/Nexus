// The Logbook's Park name cell (#437): the name of each POTA park the Park column shows, read from
// the station's park list (`lookupPark`, the list the log form names a park from) as the cell is
// drawn. Nothing is stored on the contact; a park the list does not hold, or a summit, prints the
// dash. The engine sorts the column from the same list (`query.rs` `park_name`).
import { useEffect, useState } from 'react'
import { lookupPark } from '../api'
import type { LoggedQso } from '../types'
import { parkNameRefs, parkNameText } from '../features/logColumns'

/** The names found, by reference, for this window's life: a page of contacts in one park asks the
 *  station once. A reference not found is asked again next time, so a list downloaded later fills
 *  the column in. */
const known = new Map<string, string>()

export function ParkNameCell({ q }: { q: LoggedQso }) {
  const refs = parkNameRefs(q)
  const key = refs.join(',')
  const fromKnown = () => parkNameText(q, (ref) => known.get(ref) ?? null)
  const [text, setText] = useState<string | null>(fromKnown)
  useEffect(() => {
    let live = true
    setText(fromKnown())
    const missing = refs.filter((ref) => !known.has(ref))
    if (missing.length) {
      void Promise.all(
        missing.map((ref) =>
          lookupPark(ref)
            .then((p) => {
              if (p?.name?.trim()) known.set(ref, p.name.trim())
            })
            .catch(() => {}),
        ),
      ).then(() => {
        if (live) setText(fromKnown())
      })
    }
    return () => {
      live = false
    }
    // The references are everything the name depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return (
    <span className="log-cell" title={text ?? ''}>
      {text ?? '—'}
    </span>
  )
}

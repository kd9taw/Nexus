// The Logbook's Park name cell (#437): the name of each POTA park the Park column shows, read from
// the station's park list as the cell is drawn (features/parkNames: one lookup per park for the
// window, misses kept too, forgotten when the list is downloaded or imported). Nothing is stored
// on the contact; a park the list does not hold, or a summit, prints the dash. The engine sorts the
// column from the same list (`query.rs` `park_name`).
import { useEffect, useState } from 'react'
import type { LoggedQso } from '../types'
import { parkNameRefs, parkNameText } from '../features/logColumns'
import { keptParkName, onParksChanged, resolveParkNames } from '../features/parkNames'

export function ParkNameCell({ q }: { q: LoggedQso }) {
  const refs = parkNameRefs(q)
  const key = refs.join(',')
  const fromKept = () => parkNameText(q, (ref) => keptParkName(ref) ?? null)
  const [text, setText] = useState<string | null>(fromKept)
  const [listRev, setListRev] = useState(0)
  useEffect(() => onParksChanged(() => setListRev((n) => n + 1)), [])
  useEffect(() => {
    let live = true
    setText(fromKept())
    if (refs.some((ref) => keptParkName(ref) === undefined)) {
      void resolveParkNames(refs).then(() => {
        if (live) setText(fromKept())
      })
    }
    return () => {
      live = false
    }
    // The references and the list are everything the name depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, listRev])
  return (
    <span className="log-cell" title={text ?? ''}>
      {text ?? '—'}
    </span>
  )
}

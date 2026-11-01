// The Logbook's Path cell: the route a contact most likely took, how sure, and the polar cap — a
// glyph and a word, everything else in the hover (features/pathGrade). A grade modelled afterwards
// is drawn dimmer than one stamped when the contact was logged.
import type { LoggedQso } from '../../types'
import { gradeCell, usePathGrade } from '../../features/pathGrade'

export function PathGradeCell({ q }: { q: LoggedQso }) {
  const v = gradeCell(usePathGrade(q))
  return (
    <span className={`log-cell log-path${v.dim ? ' modelled' : ''}`} title={v.title} aria-label={v.title}>
      {v.text}
    </span>
  )
}

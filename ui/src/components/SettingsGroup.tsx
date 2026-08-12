import { createContext, useContext, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'

/**
 * The section id a deep link is currently pointing at, or `null`.
 *
 * A collapsed disclosure that stays collapsed has not been "found" — the operator lands on a
 * heading with their setting hidden under it, which is exactly the Issue #62 failure (an
 * operator filed a feature request for a toggle that already shipped, inside the collapsed
 * Advanced group). So a group whose own id is the target opens itself on arrival.
 */
export const SettingsOpenTarget = createContext<string | null>(null)

interface Props {
  title: string
  children: ReactNode
  /** Registry section id. Becomes the DOM anchor (`settings-<id>`) a deep link scrolls to,
   * and the value this group watches for on {@link SettingsOpenTarget}. */
  id?: string
  /** Start expanded (the default). Advanced/optional disclosures pass false. */
  defaultOpen?: boolean
}

/**
 * Collapsible Settings sub-section. Renders the same visual as
 * `.settings-featgroup` (an uppercase title over a grid of fields), but the
 * title is a native <button> toggle with `aria-expanded`, so it is keyboard
 * accessible and shows/hides its fields. Reused by Phase-3 "Advanced"
 * disclosures.
 */
export function SettingsGroup({ title, children, id, defaultOpen = true }: Props) {
  const target = useContext(SettingsOpenTarget)
  const [open, setOpen] = useState(defaultOpen)
  // Deliberately one-way: arriving via a deep link opens the group, but a later target change
  // never re-collapses it — the operator may have opened others by hand on the way.
  useEffect(() => {
    if (id && target === id) setOpen(true)
  }, [target, id])
  return (
    <div className="settings-group" id={id ? `settings-${id}` : undefined}>
      <button
        type="button"
        className="settings-featgroup-title settings-group-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <ChevronRight size={13} className="settings-group-chevron" aria-hidden="true" />
        {title}
      </button>
      {open && <div className="settings-grid">{children}</div>}
    </div>
  )
}

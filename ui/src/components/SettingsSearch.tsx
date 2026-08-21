import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { t } from '../i18n'
import { searchSettings, type SettingsHit } from '../settings/registry'

/**
 * Find a setting by name — the answer to "I know Nexus can do this, I cannot find where".
 *
 * This is item B6 of the approved 0.17 settings plan, three batches late. The audit that
 * produced that plan called 230 controls without search "disqualifying"; it is 280-odd now, and
 * what shipped instead was two reorganisations, each of which decayed. Rearranging asks the
 * operator to guess the same shape the author did. Search lets them say what they want.
 *
 * It queries the registry's `keywords`, not just headings, and THAT is the point: the words
 * operators type live in hint text, never in the heading they would have to guess. Nothing in
 * "Rig & CAT" or "Audio" contains "COM port" or "sound card". Issue #62 was an operator who
 * could not find a toggle that had already shipped, folded inside a collapsed group — and it
 * was closed with a hint rewrite, leaving the finding problem exactly where it was.
 */
export function SettingsSearch({ onPick }: { onPick: (sectionId: string) => void }) {
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const boxRef = useRef<HTMLDivElement | null>(null)

  const hits = useMemo(() => searchSettings(q), [q])

  // Clamp the highlight whenever the result set shrinks, so Enter can never fire a stale index.
  useEffect(() => setActive(0), [q])

  // A click anywhere else closes the list. Pointerdown, not click, so choosing a result still
  // lands (the list would otherwise unmount before the click completed).
  useEffect(() => {
    if (!open) return
    const away = (e: PointerEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', away)
    return () => document.removeEventListener('pointerdown', away)
  }, [open])

  const choose = (h: SettingsHit) => {
    onPick(h.section.id)
    setOpen(false)
    setQ('')
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      // First Escape closes the list, a second clears the box — so a stray keystroke never
      // discards what was typed.
      if (open) setOpen(false)
      else setQ('')
      return
    }
    if (!hits.length) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setOpen(true)
      setActive((i) => (i + 1) % hits.length)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setOpen(true)
      setActive((i) => (i - 1 + hits.length) % hits.length)
    } else if (e.key === 'Enter') {
      e.preventDefault() // never submit the settings form from the search box
      choose(hits[active] ?? hits[0])
    }
  }

  const listId = 'settings-search-results'
  return (
    <div className="settings-search" ref={boxRef}>
      <Search size={14} className="settings-search-icon" aria-hidden="true" />
      <input
        type="search"
        className="settings-search-input"
        placeholder={t('settings.search.placeholder')}
        aria-label={t('settings.search.label')}
        role="combobox"
        aria-expanded={open && hits.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        value={q}
        onChange={(e) => {
          setQ(e.target.value)
          setOpen(true)
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
      />
      {open && q.trim() !== '' && (
        <ul className="settings-search-results" id={listId} role="listbox">
          {hits.length === 0 ? (
            // Never a bare empty box: say what was searched, so "no results" reads as an answer
            // rather than as the control being broken.
            <li className="settings-search-empty" role="presentation">
              {t('settings.search.empty', { query: q.trim() })}
            </li>
          ) : (
            hits.map((h, i) => (
              <li key={h.section.id} role="option" aria-selected={i === active}>
                <button
                  type="button"
                  className={`settings-search-hit${i === active ? ' active' : ''}`}
                  // pointerdown fires before the outside-click handler closes the list.
                  onPointerDown={(e) => {
                    e.preventDefault()
                    choose(h)
                  }}
                  onMouseEnter={() => setActive(i)}
                >
                  <span className="settings-search-hit-label">{h.section.label}</span>
                  {/* Why this result is here. A search that cannot explain a hit trains the
                      operator to distrust it. */}
                  {h.matched.toLowerCase() !== h.section.label.toLowerCase() && (
                    <span className="settings-search-hit-why">
                      {t('settings.search.matched', { term: h.matched })}
                    </span>
                  )}
                </button>
              </li>
            ))
          )}
        </ul>
      )}
    </div>
  )
}

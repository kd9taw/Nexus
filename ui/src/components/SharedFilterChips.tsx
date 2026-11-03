// The filter controls the Spots board and the Needed board share — the same chip, drawn by the same
// code, on both boards (their rules are `spotFilters.ts`). Each board keeps its own state and its
// own record of it; these only draw it and report a click.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word comes from
// the catalog. The chip labels are the shared ones (`spots.filter.*`), so the two boards' chips
// read alike in every language; a tooltip that has to say "spots" or "needs" is the host's.
import { t } from '../i18n'

/** The head's search box (`.np-search`). Escape or ✕ clears it. */
export function FilterSearch({
  query,
  onQuery,
  placeholder,
  label,
}: {
  query: string
  onQuery: (q: string) => void
  placeholder: string
  label: string
}) {
  return (
    <span className="np-search">
      <input
        type="search"
        value={query}
        placeholder={placeholder}
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onQuery('')
        }}
        aria-label={label}
      />
      {query && (
        <button type="button" className="np-search-clear" onClick={() => onQuery('')} title={t('spots.search.clear')}>
          ✕
        </button>
      )}
    </span>
  )
}

/** "My privileges": a toggle chip (`spotFilters.privilegeOk`). */
export function PrivilegesChip({ on, onToggle, title }: { on: boolean; onToggle: () => void; title: string }) {
  return (
    <button type="button" className={`np-chip${on ? ' active' : ''}`} aria-pressed={on} onClick={onToggle} title={title}>
      {t('spots.filter.privileges.label')}
    </button>
  )
}

/** The US-state chips — only the states present (`spotFilters.presentStates`), behind a separator.
 *  Nothing at all when none is. */
export function StateChips({
  available,
  picked,
  onToggle,
  ariaLabel,
  titleFor,
}: {
  available: readonly string[]
  picked: readonly string[]
  onToggle: (state: string) => void
  ariaLabel: string
  titleFor: (state: string) => string
}) {
  if (available.length === 0) return null
  return (
    <>
      <div className="np-filter-sep" aria-hidden="true" />
      <div className="np-filter-group" role="group" aria-label={ariaLabel}>
        {available.map((st) => (
          <button
            key={st}
            type="button"
            className={`np-chip${picked.includes(st) ? ' active' : ''}`}
            aria-pressed={picked.includes(st)}
            onClick={() => onToggle(st)}
            title={titleFor(st)}
          >
            {st}
          </button>
        ))}
      </div>
    </>
  )
}

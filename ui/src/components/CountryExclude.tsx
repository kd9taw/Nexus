// The country-exclusion controls: the picker that ticks countries, and the chip that says
// how many are hidden. Both are presentational — the list itself lives in
// features/countryExclude.ts and each pane subscribes once, so a pane and its controls can
// never disagree about what is ticked.
//
// The picker PORTALS (Radix DropdownMenu, the Menu.tsx precedent). That is not a style
// choice: `.cockpit-side` and `.cockpit-panes` clip overflow rather than scroll it — see
// the `.od-filters` note in styles.css — so an absolutely-positioned popover anchored in
// the chip bar would be painted outside the clip and be unreachable in the narrow rail.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Its own prose comes
// from the catalog; the country NAMES it lists do not — those are DXCC entity names, data that
// arrives from `features/countryExclude.ts` and the backend's entity table. Continent CODES
// (EU, NA…) are cty.dat tokens; only the continent NAMES beside them are prose.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useState } from 'react'
import { CONTINENT_CODES, EXCLUDABLE_COUNTRIES, countryLabel } from '../features/countryExclude'
import { getDxccEntityNames } from '../api'
import { t } from '../i18n'

/** A continent code's catalog name (#229). One literal `t()` per key, so the catalog guard can
 *  see every entry used; an unknown code shows as itself. */
function continentName(code: string): string {
  switch (code) {
    case 'NA':
      return t('hideCountries.continent.na')
    case 'SA':
      return t('hideCountries.continent.sa')
    case 'EU':
      return t('hideCountries.continent.eu')
    case 'AF':
      return t('hideCountries.continent.af')
    case 'AS':
      return t('hideCountries.continent.as')
    case 'OC':
      return t('hideCountries.continent.oc')
    default:
      return code
  }
}

interface PickerProps {
  /** The ticked catalog keys. */
  keys: ReadonlySet<string>
  /** Tick or untick one country. */
  onToggle: (key: string) => void
  /** Paused: ticks kept, nothing hidden. */
  paused?: boolean
  /** Pause/resume without losing the ticks. */
  onPauseChange?: (paused: boolean) => void
  /** Arbitrarily-picked entity NAMES beyond the curated 18 (F4MQS). */
  entities?: ReadonlySet<string>
  /** Add/remove one entity by NAME. */
  onToggleEntity?: (entity: string) => void
  /** Whole continents ticked (#229). */
  continents?: ReadonlySet<string>
  /** Tick or untick one whole continent. */
  onToggleContinent?: (code: string) => void
}

/** The Band Activity chip-bar control: 18 quick-pick checkboxes, a whole-continent submenu, and
 *  an "Other country…" search over the full DXCC table, multi-tick, stays open. */
export function CountryExcludePicker({
  keys,
  onToggle,
  paused = false,
  onPauseChange,
  entities,
  onToggleEntity,
  continents,
  onToggleContinent,
}: PickerProps) {
  const n = keys.size + (entities?.size ?? 0) + (continents?.size ?? 0)
  const active = n > 0 && !paused
  const [allEntities, setAllEntities] = useState<string[]>([])
  const [q, setQ] = useState('')
  // Load the full table lazily on first open (small, cached after).
  const loadAll = () => {
    if (allEntities.length === 0 && onToggleEntity) {
      void getDxccEntityNames().then(setAllEntities).catch(() => {})
    }
  }
  // The curated 18's entity names, so the search doesn't offer a duplicate of a quick pick.
  const curatedEntities = new Set(EXCLUDABLE_COUNTRIES.map((c) => c.entity))
  const query = q.trim().toLowerCase()
  const matches =
    query.length < 2
      ? []
      : allEntities
          .filter((e) => e.toLowerCase().includes(query) && !curatedEntities.has(e))
          .slice(0, 12)
  return (
    // NOT modal: a modal menu marks the rest of the page aria-hidden and locks body
    // scroll, which would hide the very thing the operator is judging — they tick a
    // country and watch the pane behind thin out.
    <RM.Root modal={false}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`od-chip${active ? ' active' : ''}`}
          title={t('hideCountries.chip.title')}
        >
          {t('hideCountries.chip.label')}
          {n > 0 ? ` · ${paused ? t('hideCountries.chip.paused') : n}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* Same portal-zoom re-application as Menu/Dialog/Tooltip: the portal escapes
              `.app`'s zoom:var(--ui-zoom), so the content must re-apply it. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">{t('hideCountries.head')}</div>
            {onPauseChange && n > 0 && (
              <RM.CheckboxItem
                className="ui-menu-item country-item"
                checked={paused}
                onSelect={(e) => e.preventDefault()}
                onCheckedChange={() => onPauseChange(!paused)}
              >
                {t('hideCountries.pause')}
              </RM.CheckboxItem>
            )}
            {EXCLUDABLE_COUNTRIES.map((c) => (
              <RM.CheckboxItem
                key={c.key}
                className="ui-menu-item country-item"
                checked={keys.has(c.key)}
                // Ticking must not dismiss the menu — the operator is choosing a SET.
                onSelect={(e) => e.preventDefault()}
                onCheckedChange={() => onToggle(c.key)}
              >
                {/* The tick is drawn from `data-state` in CSS rather than rendered as text,
                    so a checkbox's accessible name stays exactly its label. `aria-checked`
                    (Radix) is what actually conveys the state. */}
                {countryLabel(c)}
              </RM.CheckboxItem>
            ))}
            {/* #229 (pa0kgb): a whole continent at once — hide-by-country does not scale to
                "show only Europe". A SUBMENU, not six more rows: the operator's rule for this
                list is "under 20". Same set, same protections — a continent tick expands into
                the entity names a row is matched on. It portals like the menu, for the same
                clipping reason, and re-applies the zoom the same way. */}
            {onToggleContinent && (
              <RM.Sub>
                <RM.SubTrigger className="ui-menu-item country-item">
                  {`${t('hideCountries.continents.head')} ▸`}
                </RM.SubTrigger>
                <RM.Portal>
                  <RM.SubContent className="ui-menu country-menu" sideOffset={2} collisionPadding={8}>
                    <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
                      {CONTINENT_CODES.map((code) => (
                        <RM.CheckboxItem
                          key={`cont-${code}`}
                          className="ui-menu-item country-item"
                          checked={continents?.has(code) ?? false}
                          onSelect={(e) => e.preventDefault()}
                          onCheckedChange={() => onToggleContinent(code)}
                        >
                          {`${continentName(code)} (${code})`}
                        </RM.CheckboxItem>
                      ))}
                    </div>
                  </RM.SubContent>
                </RM.Portal>
              </RM.Sub>
            )}
            {onToggleEntity && (
              <>
                {/* Any-entity picks already made, so they can be un-ticked without searching. */}
                {[...(entities ?? [])].sort().map((e) => (
                  <RM.CheckboxItem
                    key={`ent-${e}`}
                    className="ui-menu-item country-item"
                    checked
                    onSelect={(ev) => ev.preventDefault()}
                    onCheckedChange={() => onToggleEntity(e)}
                  >
                    {e}
                  </RM.CheckboxItem>
                ))}
                <div className="country-menu-head">{t('hideCountries.other.head')}</div>
                <div style={{ padding: '0.3rem 0.6rem' }}>
                  <input
                    className="settings-input"
                    type="text"
                    value={q}
                    placeholder={t('hideCountries.search.placeholder')}
                    autoComplete="off"
                    spellCheck={false}
                    onFocus={loadAll}
                    onChange={(e) => setQ(e.target.value)}
                    onKeyDownCapture={(e) => e.stopPropagation()}
                    style={{ width: '15rem', maxWidth: '70vw' }}
                  />
                </div>
                {matches.map((e) => (
                  <RM.CheckboxItem
                    key={`match-${e}`}
                    className="ui-menu-item country-item"
                    checked={entities?.has(e) ?? false}
                    onSelect={(ev) => ev.preventDefault()}
                    onCheckedChange={() => onToggleEntity(e)}
                  >
                    {e}
                  </RM.CheckboxItem>
                ))}
              </>
            )}
            <div className="country-menu-note">{t('hideCountries.note')}</div>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}

interface ChipProps {
  /** How many COUNTRIES are ticked — curated and picked by name — not how many rows vanished
   *  (the operator chose countries, and a row count would change every slot). */
  count: number
  /** How many whole CONTINENTS are ticked (#229). Counted apart: "1 country hidden" for all of
   *  Europe would be a lie, and without it a continent-only tick showed no chip at all. */
  continents?: number
  onClear: () => void
  /** Per-pane, so a test can tell the Band Activity chip from the roster's. */
  testId: string
}

/**
 * "N countries hidden — Clear". Rendered only while the filter is actually doing
 * something, so a pane that looks emptier than the band always says why: a quiet filter is
 * the why-is-the-band-empty trap this feature must not become.
 */
export function CountryHiddenChip({ count, continents = 0, onClear, testId }: ChipProps) {
  if (count === 0 && continents === 0) return null
  const said = [
    count > 0 ? t('hideCountries.hidden', { count }) : null,
    continents > 0 ? t('hideCountries.hiddenContinents', { count: continents }) : null,
  ]
    .filter(Boolean)
    .join(' · ')
  return (
    <span className="country-hidden-chip" data-testid={testId}>
      {said}
      <button
        type="button"
        className="country-hidden-clear"
        aria-label={t('hideCountries.clear.aria')}
        title={t('hideCountries.clear.title')}
        onClick={onClear}
      >
        {t('hideCountries.clear.label')}
      </button>
    </span>
  )
}

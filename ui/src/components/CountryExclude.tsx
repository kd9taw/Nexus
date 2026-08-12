// The country-exclusion controls: the picker that ticks countries, and the chip that says
// how many are hidden. Both are presentational — the list itself lives in
// features/countryExclude.ts and each pane subscribes once, so a pane and its controls can
// never disagree about what is ticked.
//
// The picker PORTALS (Radix DropdownMenu, the Menu.tsx precedent). That is not a style
// choice: `.cockpit-side` and `.cockpit-panes` clip overflow rather than scroll it — see
// the `.od-filters` note in styles.css — so an absolutely-positioned popover anchored in
// the chip bar would be painted outside the clip and be unreachable in the narrow rail.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useState } from 'react'
import { EXCLUDABLE_COUNTRIES, countryLabel } from '../features/countryExclude'
import { getDxccEntityNames } from '../api'

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
}

/** The Band Activity chip-bar control: 18 quick-pick checkboxes + an "Other country…"
 *  search over the full DXCC table, multi-tick, stays open. */
export function CountryExcludePicker({
  keys,
  onToggle,
  paused = false,
  onPauseChange,
  entities,
  onToggleEntity,
}: PickerProps) {
  const n = keys.size + (entities?.size ?? 0)
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
          title="Hide chosen countries from this pane (a display filter — decoding, logging and alerts are untouched)"
        >
          Countries{n > 0 ? ` · ${paused ? 'paused' : n}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* Same portal-zoom re-application as Menu/Dialog/Tooltip: the portal escapes
              `.app`'s zoom:var(--ui-zoom), so the content must re-apply it. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">Hide these countries</div>
            {onPauseChange && n > 0 && (
              <RM.CheckboxItem
                className="ui-menu-item country-item"
                checked={paused}
                onSelect={(e) => e.preventDefault()}
                onCheckedChange={() => onPauseChange(!paused)}
              >
                Pause (keep my ticks, show everything)
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
                <div className="country-menu-head">Other country…</div>
                <div style={{ padding: '0.3rem 0.6rem' }}>
                  <input
                    className="settings-input"
                    type="text"
                    value={q}
                    placeholder="search all entities…"
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
            <div className="country-menu-note">
              A view filter only — decoding, logging and alerts are untouched. Stations
              calling you, the one you are working, and new entities or band slots still
              show.
            </div>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}

interface ChipProps {
  /** How many COUNTRIES are ticked (not how many rows vanished — the operator chose
   *  countries, and a row count would change every slot). */
  count: number
  onClear: () => void
  /** Per-pane, so a test can tell the Band Activity chip from the roster's. */
  testId: string
}

/**
 * "N countries hidden — Clear". Rendered only while the filter is actually doing
 * something, so a pane that looks emptier than the band always says why: a quiet filter is
 * the why-is-the-band-empty trap this feature must not become.
 */
export function CountryHiddenChip({ count, onClear, testId }: ChipProps) {
  if (count === 0) return null
  return (
    <span className="country-hidden-chip" data-testid={testId}>
      {count} {count === 1 ? 'country' : 'countries'} hidden
      <button
        type="button"
        className="country-hidden-clear"
        aria-label="Clear country filter"
        title="Show every country again"
        onClick={onClear}
      >
        Clear
      </button>
    </span>
  )
}

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
import { EXCLUDABLE_COUNTRIES, countryLabel } from '../features/countryExclude'

interface PickerProps {
  /** The ticked catalog keys. */
  keys: ReadonlySet<string>
  /** Tick or untick one country. */
  onToggle: (key: string) => void
}

/** The Band Activity chip-bar control: 18 checkboxes, multi-tick, stays open. */
export function CountryExcludePicker({ keys, onToggle }: PickerProps) {
  const n = keys.size
  return (
    // NOT modal: a modal menu marks the rest of the page aria-hidden and locks body
    // scroll, which would hide the very thing the operator is judging — they tick a
    // country and watch the pane behind thin out.
    <RM.Root modal={false}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`od-chip${n > 0 ? ' active' : ''}`}
          title="Hide chosen countries from this pane (a display filter — decoding, logging and alerts are untouched)"
        >
          Countries{n > 0 ? ` · ${n}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* Same portal-zoom re-application as Menu/Dialog/Tooltip: the portal escapes
              `.app`'s zoom:var(--ui-zoom), so the content must re-apply it. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">Hide these countries</div>
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

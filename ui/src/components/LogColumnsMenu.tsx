// The Logbook's column chooser: every column the table can show, grouped, each a checkbox; and
// the way back to the default table. It replaces the one "More columns" switch, which could only
// show all eight extra columns or none.
//
// It edits a choice and hands it back (`onChange`); the Logbook saves it. Nothing here knows where
// a choice is kept, so the menu cannot write past the one writer (`setLogbookColumns`).
import * as RM from '@radix-ui/react-dropdown-menu'
import { t } from '../i18n'
import {
  columnName,
  groupName,
  LOG_COLUMNS,
  visibleColumns,
  withColumnShown,
  type LogColumnGroup,
  type LogColumnsSaved,
} from '../features/logColumns'

const GROUPS: readonly LogColumnGroup[] = ['contact', 'location', 'qsl', 'uploads', 'contest', 'station']

/** The mark beside a chosen column: a glyph, not a word. */
const CHECK = '✓'

interface Props {
  choice: LogColumnsSaved
  onChange: (next: LogColumnsSaved) => void
  onReset: () => void
}

export function LogColumnsMenu({ choice, onChange, onReset }: Props) {
  const shown = new Set(visibleColumns(choice).map((c) => c.id))
  const custom = choice.shown.length > 0 || choice.hidden.length > 0 || Object.keys(choice.widths).length > 0
  return (
    <RM.Root modal={false}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`log-filter-chip${custom ? ' active' : ''}`}
          title={t('logbook.columns.title')}
        >
          {t('logbook.columns.label')}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu log-columns-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* The portal escapes `.app`'s zoom, so the content re-applies it (as ui/Menu does). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <p className="log-columns-hint">{t('logbook.columns.hint')}</p>
            {GROUPS.map((g) => (
              <RM.Group key={g}>
                <RM.Label className="log-columns-group">{groupName(g)}</RM.Label>
                {LOG_COLUMNS.filter((c) => c.group === g && !c.fixed).map((c) => (
                  <RM.CheckboxItem
                    key={c.id}
                    className="ui-menu-item"
                    checked={shown.has(c.id)}
                    // The menu stays open, so several columns are chosen in one visit.
                    onSelect={(e) => e.preventDefault()}
                    onCheckedChange={(on) => onChange(withColumnShown(choice, c.id, on === true))}
                  >
                    <span className="log-columns-check" aria-hidden="true">
                      {shown.has(c.id) ? CHECK : ''}
                    </span>
                    {columnName(c)}
                  </RM.CheckboxItem>
                ))}
              </RM.Group>
            ))}
            <RM.Separator className="log-columns-sep" />
            <RM.Item className="ui-menu-item" disabled={!custom} onSelect={onReset}>
              {t('logbook.columns.reset')}
            </RM.Item>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}

// CLEAN UP — the Logbook's three clean-up tools in one dialog: Duplicates (find contacts logged
// more than once, merge each into one), Bulk edit (set or clear fields on many contacts at once)
// and Check (list problems, each with a proposed fix, and make only the ones ticked), Blank grids
// (fill a grid Nexus knows into contacts logged with none, only those ticked: LogGridFill.tsx) — and Update
// from callbook, which fills the empty fields of many contacts from callbook lookups
// (LogCallbookUpdate.tsx).
//
// Every change a tool makes is ONE change in the log's history: the toast's Undo takes the whole
// of it back, and so does Recently deleted for a merge's deleted copies. No tool uploads or sends a
// contact anywhere (Update from callbook only looks calls up), and none changes the worked call,
// so every confirmation stays. The changes are
// the station's own, as a purge is: driving the station from another computer the tools read and
// preview, and their buttons that change the log are off.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands, modes,
// field names and references are invariant tokens, shown as they are.
import { t } from '../i18n'
import type { LogQuery } from '../features/logQuery'
import { drivesAnotherStation } from '../features/durableStore'
import { Dialog } from './ui/Dialog'
import { LogDupes } from './LogDupes'
import { LogBulkEdit } from './LogBulkEdit'
import { LogCheck } from './LogCheck'
import { LogCallbookUpdate } from './LogCallbookUpdate'
import { LogGridFill } from './LogGridFill'

export type LogTool = 'dupes' | 'bulk' | 'check' | 'grids' | 'callbook'

export function LogTools({
  open,
  tool,
  onTool,
  onClose,
  onChanged,
  onCallbookStarted,
  picked,
  query,
  shown,
}: {
  open: boolean
  tool: LogTool
  onTool: (tool: LogTool) => void
  onClose: () => void
  /** The log changed: the list reloads. */
  onChanged: () => void
  /** An update from the callbook started: the Logbook reports its end, open or closed. */
  onCallbookStarted: () => void
  /** The contacts ticked in the list, by id. */
  picked: ReadonlySet<string>
  /** The Logbook's search, filters and order: what "the contacts the list shows" means. */
  query: LogQuery
  /** How many contacts the list shows, for the bulk edit's choice. */
  shown: number
}) {
  const afar = drivesAnotherStation()
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('logbook.tools.title')}
      description={t('logbook.tools.intro')}
      className="log-tools-dialog"
    >
      <div className="log-tools-tabs" role="tablist" aria-label={t('logbook.tools.title')}>
        {(['dupes', 'bulk', 'check', 'grids', 'callbook'] as const).map((id) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tool === id}
            className={tool === id ? 'export-btn active' : 'export-btn'}
            onClick={() => onTool(id)}
          >
            {id === 'dupes' ? t('logbook.tools.tab.dupes')
              : id === 'bulk' ? t('logbook.tools.tab.bulk')
              : id === 'check' ? t('logbook.tools.tab.check')
              : id === 'grids' ? t('logbook.tools.tab.grids')
              : t('logbook.tools.tab.callbook')}
          </button>
        ))}
      </div>
      {afar && <p className="qso-detail-bare">{t('logbook.tools.atStation')}</p>}
      {open && tool === 'dupes' && <LogDupes onChanged={onChanged} afar={afar} />}
      {open && tool === 'bulk' && (
        <LogBulkEdit onChanged={onChanged} afar={afar} picked={picked} query={query} shown={shown} />
      )}
      {open && tool === 'check' && <LogCheck onChanged={onChanged} afar={afar} />}
      {open && tool === 'grids' && <LogGridFill onChanged={onChanged} afar={afar} />}
      {open && tool === 'callbook' && (
        <LogCallbookUpdate onStarted={onCallbookStarted} afar={afar} picked={picked} query={query} shown={shown} />
      )}
    </Dialog>
  )
}

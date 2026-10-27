// What the Logbook's clean-up tools (components/LogTools.tsx) share: the toast a change ends with,
// whose Undo takes the whole change back, and whether a press that changes the log is refused here.
import { undoHistoryRun, type ToolRun } from '../api'
import type { LoggedQso } from '../types'
import { t } from '../i18n'
import { failureText, pushToast } from '../toast'
import { drivesAnotherStation } from './durableStore'
import { isStreamInput } from '../remote-native/stream-input'
import { sayUndone } from '../components/QsoHistory'

/** What a tool's change made, said in a toast whose Undo takes the whole change back. */
export function sayRun(made: ToolRun, done: string, onChanged: () => void): void {
  const left = made.gone + made.changedSince + made.needsChoice + made.distinct
  const tail = left > 0 ? t('logbook.tools.leftAlone', { count: left }) : ''
  if (made.changed === 0) {
    pushToast(`${t('logbook.tools.nothing')}${tail}`, 'info', 8000)
    return
  }
  const run = made.run
  pushToast(`${done}${tail}`, 'success', 15_000, run ? {
    actionLabel: t('logbook.undo'),
    action: async () => {
      const answer = await undoHistoryRun(run).catch((err: unknown) => {
        throw new Error(failureText(t('qso.history.undo.failed'), err))
      })
      sayUndone(answer, t('logbook.tools.undone'), onChanged)
      onChanged()
    },
  } : {})
}

/** Whether a press that changes the log must be refused: one through the Remote stream, or any
 *  while this computer drives another station. Read before anything awaits. */
export function changesRefusedHere(): boolean {
  return isStreamInput() || drivesAnotherStation()
}

/** A contact as a tool's row names it: call, UTC date and time, band and mode — all tokens. */
export function contactLabel(q: Pick<LoggedQso, 'call' | 'whenUnix' | 'band' | 'mode'>, date: string): string {
  return `${q.call} ${date} ${q.band} ${q.mode}`
}

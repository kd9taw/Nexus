// CHECK — the log's problems, each with the fix it proposes: a grid that is no Maidenhead
// locator, a DXCC entity the call's prefix does not place it in, a frequency with no band or on
// another band, a contact in the future or one that ends before it starts, a park, summit or
// island reference not in its format. Nothing is fixed until the operator ticks it: only the
// ticked fixes are made, as one change the toast's Undo takes back. A fix whose field changed
// since the check is left out. A problem only the operator can settle (a date) is listed with no
// fix. Nothing is sent to any service.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, grids, bands,
// references and field names are invariant tokens, shown as they are.
import { useRef, useState } from 'react'
import { applyLogFixes, checkLogbook, type LogFix, type LogProblem } from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { utcDate, utcTime } from '../features/utcLog'
import { changesRefusedHere, sayRun } from '../features/logCleanup'

/** How many rows of one kind the list shows; "Tick all" still ticks every one of the kind. */
export const ROWS_SHOWN = 200

const KINDS: readonly LogProblem['kind'][] = [
  'grid', 'myGrid', 'dxcc', 'noBand', 'bandFreq', 'endBeforeStart', 'future', 'iota', 'pota', 'sota',
]

function kindLabel(kind: LogProblem['kind'], count: number): string {
  switch (kind) {
    case 'grid': return t('logbook.tools.check.kind.grid', { count })
    case 'myGrid': return t('logbook.tools.check.kind.myGrid', { count })
    case 'dxcc': return t('logbook.tools.check.kind.dxcc', { count })
    case 'noBand': return t('logbook.tools.check.kind.noBand', { count })
    case 'bandFreq': return t('logbook.tools.check.kind.bandFreq', { count })
    case 'endBeforeStart': return t('logbook.tools.check.kind.endBeforeStart', { count })
    case 'future': return t('logbook.tools.check.kind.future', { count })
    case 'iota': return t('logbook.tools.check.kind.iota', { count })
    case 'pota': return t('logbook.tools.check.kind.pota', { count })
    default: return t('logbook.tools.check.kind.sota', { count })
  }
}

/** A field's value as the list shows it: an end time as a UTC time, anything else as it is. */
function shown(p: LogProblem, v: string | null): string {
  if (v === null) return t('logbook.tools.check.empty')
  if (p.field === 'TIME_OFF') {
    const n = Number(v)
    return Number.isFinite(n) ? `${utcDate(n)} ${utcTime(n)}` : v
  }
  return v
}

// A contact can have two problems of one kind (a bad reference on each side): the field tells them apart.
const keyOf = (p: LogProblem) => `${p.id} ${p.kind} ${p.field ?? ''}`

export function LogCheck({ onChanged, afar }: { onChanged: () => void; afar: boolean }) {
  const [problems, setProblems] = useState<LogProblem[] | null>(null)
  const [checked, setChecked] = useState(0)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)

  const run = async () => {
    setBusy(true)
    setAsking(false)
    const found = await withErrorToast(() => checkLogbook(), t('logbook.tools.check.failed'))
    setBusy(false)
    if (!found) return
    setProblems(found.problems)
    setChecked(found.checked)
    setTicked(new Set())
  }

  const toggle = (keys: string[], on: boolean) => {
    setAsking(false)
    setTicked((was) => {
      const next = new Set(was)
      for (const k of keys) {
        if (on) next.add(k)
        else next.delete(k)
      }
      return next
    })
  }

  const apply = async () => {
    if (changesRefusedHere() || inFlight.current || !problems) return
    const fixes: LogFix[] = problems
      .filter((p) => ticked.has(keyOf(p)) && p.fix && p.field)
      .map((p) => ({ id: p.id, field: p.field!, expect: p.current, value: p.fix!.value }))
    inFlight.current = true
    setBusy(true)
    setAsking(false)
    const made = await withErrorToast(() => applyLogFixes(fixes), t('logbook.tools.check.applyFailed'))
    inFlight.current = false
    setBusy(false)
    if (!made) return
    sayRun(made, t('logbook.tools.check.done', { count: made.changed }), onChanged)
    onChanged()
    void run()
  }

  return (
    <div className="log-tools-pane">
      <div className="log-tools-row">
        <button type="button" className="export-btn" disabled={busy} onClick={() => void run()}>
          {busy && problems === null ? t('logbook.tools.working') : t('logbook.tools.check.run')}
        </button>
        {problems !== null && (
          <span className="qso-detail-bare">
            {t('logbook.tools.check.found', { count: problems.length, checked })}
          </span>
        )}
      </div>
      {problems !== null && problems.length > 0 && (
        <div className="log-tools-row">
          <button
            type="button"
            className="export-btn"
            disabled={busy || afar || ticked.size === 0}
            onClick={() => setAsking(true)}
          >
            {t('logbook.tools.check.apply', { count: ticked.size })}
          </button>
          {asking && (
            <span className="log-tools-confirm">
              <span>{t('logbook.tools.check.ask', { count: ticked.size })}</span>
              <button type="button" className="export-btn danger" disabled={busy} onClick={() => void apply()}>
                {t('logbook.tools.check.confirm', { count: ticked.size })}
              </button>
              <button type="button" className="export-btn" onClick={() => setAsking(false)}>
                {t('logbook.tools.cancel')}
              </button>
            </span>
          )}
        </div>
      )}
      {problems !== null && KINDS.map((kind) => {
        const of = problems.filter((p) => p.kind === kind)
        if (of.length === 0) return null
        const fixable = of.filter((p) => p.fix).map(keyOf)
        const all = fixable.length > 0 && fixable.every((k) => ticked.has(k))
        return (
          <section key={kind} className="log-tools-group">
            <div className="log-tools-row">
              <strong>{kindLabel(kind, of.length)}</strong>
              {fixable.length > 0 && (
                <label>
                  <input type="checkbox" checked={all} onChange={(e) => toggle(fixable, e.target.checked)} />{' '}
                  {t('logbook.tools.check.tickAll', { count: fixable.length })}
                </label>
              )}
            </div>
            <ul className="log-tools-list">
              {of.slice(0, ROWS_SHOWN).map((p) => (
                <li key={keyOf(p)} className="log-tools-copy">
                  <label>
                    <input
                      type="checkbox"
                      disabled={!p.fix}
                      checked={ticked.has(keyOf(p))}
                      onChange={(e) => toggle([keyOf(p)], e.target.checked)}
                    />{' '}
                    <span className="mono">{`${p.call} ${utcDate(p.whenUnix)} ${utcTime(p.whenUnix)}`}</span>
                  </label>
                  {p.field && <span className="mono">{`${p.field} ${shown(p, p.current)}`}</span>}
                  <span className="log-tools-note">
                    {p.fix ? t('logbook.tools.check.fix', { value: shown(p, p.fix.value) }) : t('logbook.tools.check.noFix')}
                  </span>
                </li>
              ))}
            </ul>
            {of.length > ROWS_SHOWN && (
              <p className="qso-detail-bare">{t('logbook.tools.check.more', { count: of.length - ROWS_SHOWN })}</p>
            )}
          </section>
        )
      })}
    </div>
  )
}

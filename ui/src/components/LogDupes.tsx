// DUPLICATES — contacts logged more than once, found by the log's own duplicate window (the same
// call, band and mode within 5 minutes) or, wider, within an hour; a group spans no more than its
// window. The call is the FULL logged call, and copies under two station calls, two operators,
// two grids, two activations or two propagation modes are two contacts: never one group, and the
// station refuses to merge them. Each group shows its copies; one is kept (the fullest is
// suggested) and takes the union of the others: every field one copy holds and the kept one
// lacks, and every confirmation. Its upload marks stay its own. A field the copies disagree on is
// shown with each value to choose from; one a confirmed copy holds, the QSL-sent mark, a comment
// or notes and a contest exchange must be chosen before that group can merge, and "Merge all"
// takes only the groups with nothing left to choose. The copies not kept are deleted — whole, into
// Recently deleted — and the merge is one change the toast's Undo takes back.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import { useRef, useState } from 'react'
import { findLogDuplicates, mergeLogDuplicates, type DupeGroup, type MergeAsk } from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { utcDate, utcTime } from '../features/utcLog'
import { changesRefusedHere, contactLabel, sayRun } from '../features/logCleanup'

type Tier = 'rule' | 'possible'

/** What the operator picked for one group: the copy kept, and the copy each field is taken from. */
interface Pick {
  keep: number
  choices: Record<string, string>
}

/** The copy a conflicting field takes: the operator's pick; else, unguarded, the kept copy's value
 *  where it holds one (the merge's own default), else the first. A guarded field has no default. */
function chosenFor(g: DupeGroup, p: Pick, tag: string): string | null {
  if (p.choices[tag]) return p.choices[tag]
  const c = g.conflicts.find((x) => x.tag === tag)
  if (!c || c.guarded) return null
  const keepId = g.copies[p.keep]?.id
  return c.values.some((v) => v.id === keepId) ? keepId ?? null : c.values[0]?.id ?? null
}

function ask(g: DupeGroup, p: Pick): MergeAsk {
  const choices: Record<string, string> = {}
  for (const c of g.conflicts) {
    const id = chosenFor(g, p, c.tag)
    if (id) choices[c.tag] = id
  }
  return {
    keep: g.copies[p.keep].id ?? '',
    copies: g.copies.map((q, i) => ({ id: q.id ?? '', revision: g.revisions[i] })),
    choices,
  }
}

function ready(g: DupeGroup, p: Pick): boolean {
  return g.conflicts.every((c) => !c.guarded || Boolean(p.choices[c.tag]))
}

export function LogDupes({ onChanged, afar }: { onChanged: () => void; afar: boolean }) {
  const [tier, setTier] = useState<Tier>('rule')
  const [groups, setGroups] = useState<DupeGroup[] | null>(null)
  const [scanned, setScanned] = useState(0)
  const [picks, setPicks] = useState<Pick[]>([])
  const [busy, setBusy] = useState(false)
  // "Merge all" asked, waiting for the operator's yes in the dialog.
  const [askingAll, setAskingAll] = useState(false)
  const inFlight = useRef(false)

  const find = async (which: Tier = tier) => {
    setBusy(true)
    setAskingAll(false)
    const scan = await withErrorToast(() => findLogDuplicates(which), t('logbook.tools.dupes.failed'))
    setBusy(false)
    if (!scan) return
    setGroups(scan.groups)
    setScanned(scan.scanned)
    setPicks(scan.groups.map((g) => ({ keep: g.keep, choices: {} })))
  }

  const merge = async (which: number[]) => {
    if (changesRefusedHere() || inFlight.current || !groups) return
    inFlight.current = true
    setBusy(true)
    setAskingAll(false)
    const made = await withErrorToast(
      () => mergeLogDuplicates(which.map((i) => ask(groups[i], picks[i]))),
      t('logbook.tools.dupes.mergeFailed'),
    )
    inFlight.current = false
    setBusy(false)
    if (!made) return
    sayRun(made, t('logbook.tools.dupes.merged', { count: which.length }), onChanged)
    onChanged()
    void find()
  }

  const setPick = (i: number, p: Pick) => setPicks((all) => all.map((x, k) => (k === i ? p : x)))
  const readyAll = groups ? groups.map((_, i) => i).filter((i) => ready(groups[i], picks[i])) : []
  const copiesAll = readyAll.reduce((n, i) => n + (groups?.[i].copies.length ?? 1) - 1, 0)

  return (
    <div className="log-tools-pane">
      <div className="log-tools-row">
        <label>
          {t('logbook.tools.dupes.tierLabel')}{' '}
          <select
            value={tier}
            onChange={(e) => { const v = e.target.value as Tier; setTier(v); setGroups(null) }}
          >
            <option value="rule">{t('logbook.tools.dupes.tier.rule')}</option>
            <option value="possible">{t('logbook.tools.dupes.tier.possible')}</option>
          </select>
        </label>
        <button type="button" className="export-btn" disabled={busy} onClick={() => void find()}>
          {busy && groups === null ? t('logbook.tools.working') : t('logbook.tools.dupes.find')}
        </button>
      </div>
      {groups !== null && (
        <p className="qso-detail-bare">
          {t('logbook.tools.dupes.found', { count: groups.length, scanned })}
        </p>
      )}
      {groups !== null && groups.length > 0 && tier === 'rule' && (
        <div className="log-tools-row">
          <button
            type="button"
            className="export-btn"
            disabled={busy || afar || readyAll.length === 0}
            onClick={() => setAskingAll(true)}
          >
            {t('logbook.tools.dupes.mergeAll', { count: readyAll.length })}
          </button>
          {askingAll && (
            <span className="log-tools-confirm">
              <span>{t('logbook.tools.dupes.mergeAllAsk', { count: readyAll.length, copies: copiesAll })}</span>
              <button type="button" className="export-btn danger" disabled={busy} onClick={() => void merge(readyAll)}>
                {t('logbook.tools.dupes.mergeAllConfirm', { count: readyAll.length })}
              </button>
              <button type="button" className="export-btn" onClick={() => setAskingAll(false)}>
                {t('logbook.tools.cancel')}
              </button>
            </span>
          )}
        </div>
      )}
      {groups !== null && groups.length > 0 && (
        <ol className="log-tools-list">
          {groups.map((g, i) => {
            const p = picks[i]
            return (
              <li key={g.copies.map((q) => q.id).join(' ')} className="log-tools-group">
                <ul className="log-tools-copies">
                  {g.copies.map((q, k) => (
                    <li key={q.id ?? k} className="log-tools-copy">
                      <label>
                        <input
                          type="radio"
                          name={`keep-${i}`}
                          checked={p.keep === k}
                          onChange={() => setPick(i, { ...p, keep: k })}
                        />{' '}
                        <span className="mono">{contactLabel(q, `${utcDate(q.whenUnix)} ${utcTime(q.whenUnix)}`)}</span>
                      </label>
                      {p.keep === k && <span className="log-tools-note">{t('logbook.tools.dupes.kept')}</span>}
                      {q.confirmed && <span className="log-tools-note">{t('logbook.tools.dupes.confirmed')}</span>}
                    </li>
                  ))}
                </ul>
                {g.conflicts.length > 0 && (
                  <ul className="log-tools-conflicts">
                    {g.conflicts.map((c) => {
                      const chosen = chosenFor(g, p, c.tag)
                      return (
                        <li key={c.tag}>
                          <span className="mono">{c.tag}</span>
                          {c.guarded && !p.choices[c.tag] && (
                            <span className="log-tools-note danger">{t('logbook.tools.dupes.choose')}</span>
                          )}
                          {c.values.map((v) => (
                            <label key={v.id} className="log-tools-choice">
                              <input
                                type="radio"
                                name={`${i}-${c.tag}`}
                                checked={chosen === v.id}
                                onChange={() => setPick(i, { ...p, choices: { ...p.choices, [c.tag]: v.id } })}
                              />{' '}
                              <span className="mono">{v.value}</span>
                            </label>
                          ))}
                        </li>
                      )
                    })}
                  </ul>
                )}
                <button
                  type="button"
                  className="export-btn"
                  disabled={busy || afar || !ready(g, p)}
                  onClick={() => void merge([i])}
                >
                  {t('logbook.tools.dupes.merge', { count: g.copies.length })}
                </button>
              </li>
            )
          })}
        </ol>
      )}
    </div>
  )
}

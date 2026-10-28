// BLANK GRIDS — contacts logged with no grid where Nexus knows the station's grid: the one it SENT
// ON THE AIR (heard by Nexus, in ALL.TXT or this session, within 24 hours of the contact), else its
// home QTH from a callbook lookup
// already made (a call with no stroke and no park only, from the last 12 months). Nothing is
// changed until the operator ticks a contact and confirms: only the ticked ones are filled, as one
// change the toast's Undo takes back. A heard grid counts as any logged grid; a callbook's is
// marked as the callbook's, for the distance and the map only — it counts toward no grid award,
// fills no GRID need and is sent to no service. A contact already uploaded keeps its upload marks,
// as for every change here: nothing is sent again.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, grids and bands
// are invariant tokens, shown as they are.
import { useRef, useState } from 'react'
import { fillLogGrids, findLogGridOffers, type LogGridOffer } from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { changesRefusedHere, sayRun } from '../features/logCleanup'

/** How many rows the list shows. "Tick all" ticks these and no other: nothing is filled that the
 *  operator has not seen. */
export const ROWS_SHOWN = 200

export function LogGridFill({ onChanged, afar }: { onChanged: () => void; afar: boolean }) {
  const [offers, setOffers] = useState<LogGridOffer[] | null>(null)
  const [blank, setBlank] = useState(0)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)

  const run = async () => {
    setBusy(true)
    setAsking(false)
    const found = await withErrorToast(() => findLogGridOffers(), t('logbook.tools.grids.failed'))
    setBusy(false)
    if (!found) return
    setOffers(found.offers)
    setBlank(found.blank)
    setTicked(new Set())
  }

  const toggle = (ids: string[], on: boolean) => {
    setAsking(false)
    setTicked((was) => {
      const next = new Set(was)
      for (const id of ids) {
        if (on) next.add(id)
        else next.delete(id)
      }
      return next
    })
  }

  const apply = async () => {
    if (changesRefusedHere() || inFlight.current || !offers) return
    const picks = offers
      .filter((o) => ticked.has(o.id))
      .map((o) => ({ id: o.id, grid: o.grid, source: o.source }))
    inFlight.current = true
    setBusy(true)
    setAsking(false)
    const made = await withErrorToast(() => fillLogGrids(picks), t('logbook.tools.grids.applyFailed'))
    inFlight.current = false
    setBusy(false)
    if (!made) return
    sayRun(made, t('logbook.tools.grids.done', { count: made.changed }), onChanged)
    onChanged()
    void run()
  }

  const shown = offers?.slice(0, ROWS_SHOWN) ?? []
  const all = shown.length > 0 && shown.every((o) => ticked.has(o.id))
  // The confirm says how many of the ticked grids count (heard) and how many are for the map only
  // (callbook).
  const tickedHeard = offers?.filter((o) => ticked.has(o.id) && o.source === 'heard').length ?? 0
  return (
    <div className="log-tools-pane">
      <p className="qso-detail-bare">{t('logbook.tools.grids.intro')}</p>
      <div className="log-tools-row">
        <button type="button" className="export-btn" disabled={busy} onClick={() => void run()}>
          {busy && offers === null ? t('logbook.tools.working') : t('logbook.tools.grids.run')}
        </button>
        {offers !== null && (
          <span className="qso-detail-bare">
            {t('logbook.tools.grids.found', { count: offers.length, blank })}
          </span>
        )}
      </div>
      {offers !== null && offers.length > 0 && (
        <>
          <div className="log-tools-row">
            <label>
              <input
                type="checkbox"
                checked={all}
                onChange={(e) => toggle(shown.map((o) => o.id), e.target.checked)}
              />{' '}
              {t('logbook.tools.grids.tickAll', { count: shown.length })}
            </label>
            <button
              type="button"
              className="export-btn"
              disabled={busy || afar || ticked.size === 0}
              onClick={() => setAsking(true)}
            >
              {t('logbook.tools.grids.apply', { count: ticked.size })}
            </button>
            {asking && (
              <span className="log-tools-confirm">
                <span>
                  {t('logbook.tools.grids.ask', {
                    count: ticked.size,
                    heard: tickedHeard,
                    callbook: ticked.size - tickedHeard,
                  })}
                </span>
                <button type="button" className="export-btn danger" disabled={busy} onClick={() => void apply()}>
                  {t('logbook.tools.grids.confirm')}
                </button>
                <button type="button" className="export-btn" onClick={() => setAsking(false)}>
                  {t('logbook.tools.cancel')}
                </button>
              </span>
            )}
          </div>
          <ul className="log-tools-list">
            {shown.map((o) => (
              <li key={o.id} className="log-tools-copy">
                <label>
                  <input
                    type="checkbox"
                    checked={ticked.has(o.id)}
                    onChange={(e) => toggle([o.id], e.target.checked)}
                  />{' '}
                  <span className="mono">{`${o.call} ${utcDate(o.whenUnix)} ${o.band} ${o.mode}`}</span>
                </label>
                <span className="mono">{o.grid}</span>
                <span className="log-tools-note">
                  {o.source === 'heard' ? t('logbook.tools.grids.heard') : t('logbook.tools.grids.callbook')}
                  {o.uploaded ? ` · ${t('logbook.tools.grids.uploaded')}` : ''}
                </span>
              </li>
            ))}
          </ul>
          {offers.length > ROWS_SHOWN && (
            <p className="qso-detail-bare">{t('logbook.tools.grids.more', { count: offers.length - ROWS_SHOWN })}</p>
          )}
        </>
      )}
    </div>
  )
}

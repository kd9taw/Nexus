// THE GETTING OUT LIST — every receiver the station reports (`propagation::getting_out`: the latest report
// per receiver over its window), under a SORT BAR: Distance (the station's own order, the default), Call
// A–Z, Newest and SNR (features/getoutSort). Each row selects its station on the map and says how long
// ago it heard you.
//
// THE BAR IS PART OF THE BOX'S CONTENT, in the body the frame gives it (Conditions' PaneFrame, the
// dashboard rail's, a cockpit's CockpitPaneFrame with the shared list's `fill` role): it sizes nothing,
// and its chips wrap onto a second line in a narrow box. It is a radio group of native radios, named,
// one per box (`useId`), so the browser gives it one Tab stop and the arrows choose — the input itself is
// visually hidden and its label is the chip.
//
// THE CHOICE IS THE BOX'S (`instance`, its place: a Conditions or rail slot, or a cockpit's box), kept
// across a restart and read back validated. A live update re-sorts the new answer in the chosen order
// with stable ties; the list does not auto-follow, so the body's scroll stays where the operator left it.
//
// ⚠️ ON THE MIGRATED LIST with panes.tsx (i18n/hardcoded-strings.test.ts): every word is the catalog's.
import { useId, useMemo, useState } from 'react'
import { t } from '../../i18n'
import type { HeardMe } from '../../types'
import type { Units } from '../../units'
import { fmtDistanceKm } from '../../units'
import { ageLabel } from '../SpotsPanel'
import { GETOUT_SORTS, loadGetoutSort, saveGetoutSort, sortGetout, type GetoutSort } from '../../features/getoutSort'

/** A unit symbol printed beside a reading: a token, the same in every language. */
const DB_UNIT = 'dB'

/** Each order's chip. Literal keys, resolved at render (a change of language re-renders the box). */
const SORT_LABEL: Record<GetoutSort, () => string> = {
  distance: () => t('connect.getout.sort.distance'),
  call: () => t('connect.getout.sort.call'),
  newest: () => t('connect.getout.sort.newest'),
  snr: () => t('connect.getout.sort.snr'),
}

export function GetoutList({
  reports,
  units,
  instance,
  onSelectCall,
}: {
  reports: readonly HeardMe[]
  units: Units
  /** The box's place, which its choice is kept under. */
  instance: string
  onSelectCall: (call: string) => void
}) {
  const name = useId()
  const [sort, setSort] = useState<GetoutSort>(() => loadGetoutSort(instance))
  const rows = useMemo(() => sortGetout(reports, sort), [reports, sort])
  const choose = (s: GetoutSort) => {
    setSort(s)
    saveGetoutSort(instance, s)
  }
  return (
    <>
      <div className="getout-sort" role="radiogroup" aria-label={t('connect.getout.sort.aria')}>
        {GETOUT_SORTS.map((s) => (
          <label key={s} className={`getout-sort-opt${sort === s ? ' active' : ''}`}>
            <input
              type="radio"
              className="sr-only"
              name={name}
              value={s}
              checked={sort === s}
              onChange={() => choose(s)}
            />
            {SORT_LABEL[s]()}
          </label>
        ))}
      </div>
      <ul className="getout-list">
        {rows.map((r) => (
          <li
            key={r.call}
            className="go-clickable"
            onClick={() => onSelectCall(r.call)}
            title={t('connect.getout.select.title', { call: r.call })}
          >
            <span className="go-call">{r.call}</span>
            <span className="go-where">
              {r.octant} {fmtDistanceKm(r.km, units)}
            </span>
            <span className="go-band">{r.band}</span>
            <span className="go-snr">{r.snr != null ? `${r.snr} ${DB_UNIT}` : ''}</span>
            <span className="go-age">{ageLabel(r.ageSecs)}</span>
          </li>
        ))}
      </ul>
    </>
  )
}

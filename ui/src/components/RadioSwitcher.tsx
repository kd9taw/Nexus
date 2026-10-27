// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the radio's own profile name, its band label and
// its dial frequency — all three are interpolated as the tokens they are.
//
// The peg lock pins which radio band selection may move. It is not a transmit control and is on
// no cockpit's stop-line census; `transmitting` is read here only to colour the pill.
import type { RadioSummary } from '../types'
import { t } from '../i18n'

interface Props {
  radios: RadioSummary[]
  pegged: boolean
  onSwitch: (id: number) => void
  onTogglePeg: (on: boolean) => void
}

/** Dual-radio switcher: one pill per configured radio (active highlighted, others show their
 * last-known band), plus a peg-lock toggle that pins the active radio so band selection can't
 * auto-switch it (P4). Rendered ONLY when there's more than one radio — a single-radio station
 * never sees this (the whole multi-radio surface stays invisible until a 2nd radio is added). */
export function RadioSwitcher({ radios, pegged, onSwitch, onTogglePeg }: Props) {
  if (radios.length < 2) return null
  return (
    <div className="radio-switcher" role="group" aria-label={t('radios.switcher.aria')}>
      {radios.map((r) => {
        const freq = r.dialMhz > 0 ? `${r.dialMhz.toFixed(3)}` : '—'
        // A background (monitored) radio whose CAT probe is failing: surface it on the pill so a dead
        // 2nd rig is visible at a glance (Test CAT only ever checks the ACTIVE radio). The active
        // radio's own CAT trouble already shows in its cockpit's "no rig control" badge.
        const catDead = !r.isActive && r.catOk === false
        return (
          <button
            key={r.id}
            type="button"
            className={`radio-pill${r.isActive ? ' active' : ''}${r.transmitting ? ' tx' : ''}${catDead ? ' cat-dead' : ''}`}
            aria-pressed={r.isActive}
            onClick={() => !r.isActive && onSwitch(r.id)}
            title={
              r.isActive
                ? t('radios.switcher.active.title', { name: r.name, band: r.band, freq })
                : catDead
                  ? t('radios.switcher.catDead.title', { name: r.name })
                  : t('radios.switcher.switch.title', {
                      name: r.name,
                      band: r.band || '—',
                      freq,
                    })
            }
          >
            <span className="radio-pill-name">
              {r.name}
              {catDead && (
                <span className="radio-pill-warn" aria-label={t('radios.switcher.catDead.aria')}>
                  {' '}
                  ⚠
                </span>
              )}
            </span>
            <span className="radio-pill-band">
              {catDead ? t('radios.switcher.catDead.band') : r.band || '—'}
            </span>
          </button>
        )
      })}
      <button
        type="button"
        className={`radio-peg${pegged ? ' on' : ''}`}
        aria-pressed={pegged}
        onClick={() => onTogglePeg(!pegged)}
        title={pegged ? t('radios.peg.on.title') : t('radios.peg.off.title')}
      >
        {pegged ? t('radios.peg.on.label') : t('radios.peg.off.label')}
      </button>
    </div>
  )
}

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The verdict words
// and the row labels come from the catalog; every reading beside them is data, printed by an
// invariant formatter (`toFixed`, `Math.round`) and never locale-aware.
import { t } from '../i18n'
import type { LinkState, RadioStatus } from '../types'

interface Props {
  link: LinkState
  radio: RadioStatus
}

/**
 * The invariant tokens this pill prints, gathered so the guard can prove they never became
 * catalog entries. `RV` and `dT` are the link report's own FIELD NAMES — `dT` is WSJT-X's,
 * and a translated one names nothing — and `MHz` / `Hz` are unit symbols: the same characters
 * in every language (the rule is in `i18n/index.ts`).
 */
const LINK_TOKENS = { rv: 'RV', dt: 'dT', mhz: 'MHz', hz: 'Hz' } as const

function quality(link: LinkState): 'solid' | 'marginal' | 'weak' {
  if (link.quality > 0.6) return 'solid'
  if (link.quality > 0.35) return 'marginal'
  return 'weak'
}

export function LinkPill({ link, radio }: Props) {
  const q = quality(link)
  const label =
    q === 'solid'
      ? t('link.quality.solid', { snr: fmt(link.snrDb) })
      : q === 'marginal'
        ? t('link.quality.marginal', { rv: link.rv })
        : t('link.quality.weak', { snr: fmt(link.snrDb) })

  return (
    <div className="telemetry">
      <div className={`link-pill ${q}`}>
        <span className="link-dot" />
        <span className="link-label">{label}</span>
      </div>
      <dl className="telemetry-grid">
        <div>
          <dt>{t('link.dial.label')}</dt>
          <dd>
            {radio.dialMhz.toFixed(3)} {LINK_TOKENS.mhz}
          </dd>
        </div>
        <div>
          <dt>{t('link.band.label')}</dt>
          <dd>{radio.band}</dd>
        </div>
        <div>
          <dt>{t('link.tier.label')}</dt>
          <dd>{link.tier}</dd>
        </div>
        <div>
          <dt>{LINK_TOKENS.rv}</dt>
          <dd>{link.rv}</dd>
        </div>
        <div>
          <dt>{LINK_TOKENS.dt}</dt>
          <dd>{link.dtSec.toFixed(1)}s</dd>
        </div>
        <div>
          <dt>{t('link.audioFreq.label')}</dt>
          <dd>
            {Math.round(link.freqHz)} {LINK_TOKENS.hz}
          </dd>
        </div>
      </dl>
    </div>
  )
}

function fmt(v: number): string {
  return `${v > 0 ? '+' : ''}${Math.round(v)}`
}

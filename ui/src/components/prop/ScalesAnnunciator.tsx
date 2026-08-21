// SWPC R/S/G scale chips + the latest space-weather alert headline (B3). Live external
// data (desktop-only); degrades to a plain "no live scales" line when the feed is absent.
import type { NoaaScalesView, AlertView } from '../../types'
import { t, type MessageKey } from '../../i18n'

// R, S and G are the NOAA scales' own letters — technical tokens, not prose. Only what
// each one MEANS is a catalog entry, and it is looked up when the chip renders.
const RSG: { key: 'r' | 's' | 'g'; label: string; titleKey: MessageKey }[] = [
  { key: 'r', label: 'R', titleKey: 'prop.scales.r.title' },
  { key: 's', label: 'S', titleKey: 'prop.scales.s.title' },
  { key: 'g', label: 'G', titleKey: 'prop.scales.g.title' },
]

function sev(n: number): string {
  return n <= 0 ? 'quiet' : n <= 2 ? 'minor' : 'major'
}

export function ScalesAnnunciator({
  scales,
  alerts,
}: {
  scales: NoaaScalesView | null
  alerts: AlertView[]
}) {
  // asOf is stamped only on a REAL fetch — an all-zero default from a cold/
  // offline feed must read as "no data", never as a genuinely quiet sun.
  if (!scales || !scales.asOf) return <p className="pane-basic">{t('prop.scales.none')}</p>
  const top = alerts[0]
  return (
    <div className="swsc">
      <div className="swsc-scales">
        {RSG.map((x) => (
          <span key={x.key} className={`swsc-chip swsc-${sev(scales[x.key])}`} title={t(x.titleKey)}>
            {x.label}
            {scales[x.key]}
          </span>
        ))}
        {scales.gTomorrow > 0 && (
          <span className="swsc-fc" title={t('prop.scales.tomorrow.title')}>
            {t('prop.scales.tomorrow', { level: scales.gTomorrow })}
          </span>
        )}
      </div>
      {top && (
        <p className="swsc-alert" title={top.message}>
          <span className="swsc-kind">{top.kind}</span> {top.message.replace(/\s+/g, ' ').slice(0, 90)}
        </p>
      )}
    </div>
  )
}

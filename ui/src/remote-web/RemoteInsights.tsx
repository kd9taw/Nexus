import { useContext, useEffect, useState } from 'react'
import { AwardsJourney } from '../components/AwardsJourney'
import { StatsView } from '../components/StatsView'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { INSIGHTS_COMMAND, type InsightCollection } from './application-query-protocol'
import { parseInsights, type Insights } from './insights'

export function RemoteInsights({ kind, showGamification = true }: { kind: InsightCollection; showGamification?: boolean }) {
  const source = useContext(RemoteCollectionsContext)
  const available = useStationData()
  const supported = source?.client.supports(INSIGHTS_COMMAND) ?? false
  const [capture, setCapture] = useState<{ value: Insights; at: number } | null>(null)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'unavailable'>('loading')
  const [refresh, setRefresh] = useState(0)
  const [now, setNow] = useState(() => performance.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(performance.now()), 1000)
    return () => clearInterval(timer)
  }, [])
  useEffect(() => {
    if (!available || !supported || !source) { setCapture(null); setPhase('unavailable'); return }
    let live = true
    setPhase('loading')
    const started = performance.now()
    void source.page({ collection: kind, cursor: null, search: '', unconfirmed: false, after: null })
      .then(page => {
        const value = parseInsights(page, kind)
        if (live) { setCapture({ value, at: started - value.capturedAgeMs }); setPhase('ready'); setNow(performance.now()) }
      })
      .catch(() => { if (live) { setCapture(null); setPhase('unavailable') } })
    return () => { live = false }
  }, [source, available, supported, kind, refresh])
  const age = capture ? Math.max(0, now - capture.at) : 0
  const expired = age >= 60_000
  const value = available && supported && !expired && capture?.value.kind === kind ? capture.value : null
  return <div className="remote-insights-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : expired ? t('remote.insightsExpired') : value
        ? t('remote.insightsSnapshot', { count: value.logCount, seconds: Math.floor(age / 1000) }) : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n => n + 1)}>{t('remote.insightsRefresh')}</button>
      {kind === 'awards' && <span>{t('remote.awardsObserver')}</span>}
    </div>
    {value?.kind === 'awards' && <AwardsJourney showGamification={showGamification} observation={value.awards} />}
    {value?.kind === 'statistics' && <StatsView observation={value} />}
  </div>
}

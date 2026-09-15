import { useContext, useEffect, useState } from 'react'
import { AwardsJourney } from '../components/AwardsJourney'
import { StatsView } from '../components/StatsView'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { CONFIRMATIONS_COMMAND, INSIGHTS_COMMAND, type InsightCollection } from './application-query-protocol'
import { parseInsights, type Insights } from './insights'
import { parseConfirmations, type Confirmations } from './confirmations'

export function RemoteInsights({ kind, showGamification = true }: { kind: InsightCollection; showGamification?: boolean }) {
  const source = useContext(RemoteCollectionsContext)
  const available = useStationData()
  const supported = source?.client.supports(INSIGHTS_COMMAND) ?? false
  // An older station offers the summary without diagnostics; the Awards view then says so.
  const diagnosable = kind === 'awards' && (source?.client.supports(CONFIRMATIONS_COMMAND) ?? false)
  const [capture, setCapture] = useState<{ value: Insights; at: number } | null>(null)
  const [diagnostics, setDiagnostics] = useState<{ value: Confirmations; at: number } | null>(null)
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
  useEffect(() => {
    setDiagnostics(null)
    if (!available || !diagnosable || !source) return
    let live = true
    const started = performance.now()
    // Best effort, as on the desktop: a failed diagnosis never holds back the award summary.
    void source.page({ collection: 'confirmations', cursor: null, search: '', unconfirmed: false, after: null })
      .then(page => {
        const value = parseConfirmations(page)
        if (live) setDiagnostics({ value, at: started - value.capturedAgeMs })
      })
      .catch(() => { if (live) setDiagnostics(null) })
    return () => { live = false }
  }, [source, available, diagnosable, refresh])
  const age = capture ? Math.max(0, now - capture.at) : 0
  const expired = age >= 60_000
  const value = available && supported && !expired && capture?.value.kind === kind ? capture.value : null
  const report = available && diagnostics && now - diagnostics.at < 60_000 ? diagnostics.value.report : undefined
  return <div className="remote-insights-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : expired ? t('remote.insightsExpired') : value
        ? t('remote.insightsSnapshot', { count: value.logCount, seconds: Math.floor(age / 1000) }) : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n => n + 1)}>{t('remote.insightsRefresh')}</button>
      {kind === 'awards' && <span>{diagnosable ? t('remote.b3.awardsObserver') : t('remote.awardsObserver')}</span>}
    </div>
    {value?.kind === 'awards' && <AwardsJourney showGamification={showGamification} observation={value.awards} diagnostics={report} />}
    {value?.kind === 'statistics' && <StatsView observation={value} />}
  </div>
}

import { useContext, useEffect, useMemo, useState } from 'react'
import { DxpeditionsView } from '../components/DxpeditionsView'
import { dxpedLink } from '../components/prop/dxpedLink'
import type { CalendarEntry } from '../types'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { DXPEDITIONS_COMMAND } from './application-query-protocol'
import { parseDxpeditions, type Dxpeditions } from './dxpeditions'

function openPage(entry: CalendarEntry) {
  const link = dxpedLink(entry)
  if (link) window.open(link.url, '_blank', 'noopener,noreferrer')
}
export function RemoteDxpeditions() {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const supported = source?.client.supports(DXPEDITIONS_COMMAND) ?? false
  const [capture, setCapture] = useState<{ value: Dxpeditions; at: number } | null>(null)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'unavailable'>('loading')
  const [refresh, setRefresh] = useState(0), [now, setNow] = useState(() => performance.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(performance.now()), 1000)
    return () => clearInterval(timer)
  }, [])
  useEffect(() => {
    setCapture(null)
    if (!available || !supported || !source) { setPhase('unavailable'); return }
    let live = true
    setPhase('loading')
    const started = performance.now()
    void source.page({ collection: 'dxpeditions', cursor: null, search: '', unconfirmed: false, after: null })
      .then(page => {
        const value = parseDxpeditions(page)
        if (live) { setCapture({ value, at: started - value.capturedAgeMs }); setPhase('ready'); setNow(performance.now()) }
      }).catch(() => { if (live) { setCapture(null); setPhase('unavailable') } })
    return () => { live = false }
  }, [source, available, supported, refresh])
  const elapsed = capture ? Math.max(0, now - capture.at) : 0
  const age = (capture?.value.sourceAgeMs ?? 0) + elapsed
  const expired = age >= 300_000 || elapsed >= 60_000
  const value = available && supported && !expired ? capture?.value : null
  const forecastsCurrent = value?.windowValidForMs != null && elapsed < value.windowValidForMs
  const observation = useMemo(() => ({ windows: forecastsCurrent ? value?.windows ?? null : null, openPage }), [value?.windows, forecastsCurrent])
  return <div className="remote-insights-view remote-dxpeditions-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : expired ? t('remote.dxpedExpired') : value
        ? t('remote.dxpedSnapshot', { seconds: Math.floor(age / 1000) }) : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n => n + 1)}>{t('remote.dxpedRefresh')}</button>
      <span>{t('remote.dxpedObserver')}</span>
      {value && <span>{!forecastsCurrent ? t('remote.dxpedNoWindows')
        : t('remote.dxpedWindowsAge', { minutes: Math.floor(((value.windowAgeMs ?? 0) + elapsed) / 60_000) })}</span>}
    </div>
    {value && <DxpeditionsView snap={value} observation={observation} />}
  </div>
}

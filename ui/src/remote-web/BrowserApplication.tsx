import { useEffect, useState, useSyncExternalStore } from 'react'
import App from '../App'
import type { AppSnapshot, BandChannel, Settings } from '../types'
import { installApplicationTransport } from '../applicationTransport'
import { ErrorBoundary } from '../components/ErrorBoundary'
import { t } from '../i18n'
import { StationControlContext, StationDataContext } from '../stationAccess'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { HostedConnection } from './client'
import '../cockpit-panes.css'
import './application.css'

type Bootstrap = { snapshot: AppSnapshot; settings: Settings; bandPlan: BandChannel[] }
export function BrowserApplication({ connection, disconnect }: { connection: HostedConnection; disconnect: () => void }) {
  const client = connection.application
  const phase = useSyncExternalStore(client.subscribe, client.getPhase)
  const [boot, setBoot] = useState<Bootstrap | null>(null)
  const [error, setError] = useState(false)
  const [, setTick] = useState(0)
  useEffect(() => installApplicationTransport(client), [client])
  useEffect(() => {
    const timer = setInterval(() => setTick(value => value + 1), 500)
    return () => clearInterval(timer)
  }, [])
  useEffect(() => {
    if (phase !== 'ready') return
    let live = true, timer: ReturnType<typeof setTimeout>
    async function load() {
      try {
        const [snapshot, settings, bandPlan] = await Promise.all([
          client.invoke<AppSnapshot>('get_snapshot'), client.invoke<Settings>('get_settings'), client.invoke<BandChannel[]>('get_band_plan'),
        ])
        if (live) { setBoot({ snapshot, settings, bandPlan }); setError(false); timer = setTimeout(() => void load(), 2000) }
      } catch {
        if (live) { setError(true); timer = setTimeout(() => void load(), 1000) }
      }
    }
    void load()
    return () => { live = false; clearTimeout(timer) }
  }, [client, phase])
  const stale = phase !== 'ready' || client.age('get_snapshot') >= APPLICATION_TIMEOUT_MS
  const status = <div className="remote-application-status" role="status">
    <strong>{t('remote.browserWorkspace')}</strong>
    <span>{stale ? t('remote.applicationUnavailable') : t('remote.applicationObserver')}</span>
    <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
  </div>
  if (!boot) return <div className="app remote-monitor-app remote-service-app">
    {status}
    <main className="rm-scroll"><div className="rm-content">
      <h1>{t('remote.browserWorkspace')}</h1>
      <p>{phase === 'updateRequired' ? t('remote.desktopUpdateRequired') : error || phase === 'unavailable'
        ? t('remote.applicationUnavailable') : t('monitor.connecting')}</p>
    </div></main>
  </div>
  return <ErrorBoundary label={t('remote.browserWorkspace')} action={{ label: t('remote.disconnect'), onClick: disconnect }}>
    <StationControlContext.Provider value={false}>
      <StationDataContext.Provider value={!stale}>
        <App remote={{ ...boot, status, stale }} />
      </StationDataContext.Provider>
    </StationControlContext.Provider>
  </ErrorBoundary>
}

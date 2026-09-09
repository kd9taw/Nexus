import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'
import App from '../App'
import { RemoteCollections, RemoteCollectionsContext, RemoteHistoryContext } from './collections'
import type { HistoryRow, RemoteHistory } from './collections'
import { INSIGHTS_COMMAND, QUERY_COMMAND } from './application-query-protocol'
import type { AppSnapshot, BandChannel, Settings } from '../types'
import { installApplicationTransport } from '../applicationTransport'
import { ErrorBoundary } from '../components/ErrorBoundary'
import { t } from '../i18n'
import { StationControlContext, StationDataContext } from '../stationAccess'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { HostedConnection } from './client'
import '../cockpit-panes.css'
import './application.css'

type Bootstrap = { snapshot: AppSnapshot; settings: Settings; bandPlan: BandChannel[]; cwPhone: boolean; keyboard: boolean; collections: boolean; insights: boolean }
export function BrowserApplication({ connection, disconnect }: { connection: HostedConnection; disconnect: () => void }) {
  const client = connection.application
  const collections = useMemo(() => new RemoteCollections(client), [client])
  const [history, setHistory] = useState<RemoteHistory | null>(null)
  const phase = useSyncExternalStore(client.subscribe, client.getPhase)
  const [boot, setBoot] = useState<Bootstrap | null>(null)
  const [error, setError] = useState(false)
  const [, setTick] = useState(0)
  useEffect(() => {
    collections.activate()
    const uninstall = installApplicationTransport(collections)
    return () => { uninstall(); collections.dispose() }
  }, [collections])
  useEffect(() => {
    if (phase !== 'ready' || !client.supports(QUERY_COMMAND)) { setHistory(null); collections.invalidate(); return }
    let signature = ''
    let live = true, after: number | null = null, timer: ReturnType<typeof setTimeout>
    async function loadHistory() {
      try {
        const snapshot = await client.invoke<AppSnapshot>('get_snapshot')
        const nextSignature = JSON.stringify([snapshot.radio.band, snapshot.link.tier, snapshot.radio.slot, snapshot.recentDecodes])
        if (nextSignature === signature) { if (live) timer = setTimeout(() => void loadHistory(), 1000); return }
        const result = await collections.read('decodes', after)
        const meta = (result.meta as { source: Omit<RemoteHistory, 'rows'> & { latestSequence: number } }).source
        if (!live) return
        const rows = result.rows as unknown as HistoryRow[]
        setHistory(previous => {
          const merged = new Map((previous?.generation === meta.generation ? previous.rows : []).map(row => [row.firstSequence, row]))
          for (const row of rows) merged.set(row.firstSequence, row)
          return { ...meta, rows: [...merged.values()].sort((a, b) => a.firstSequence - b.firstSequence).slice(-3000) }
        })
        after = meta.latestSequence
        signature = nextSignature
      } catch { if (live) { setHistory(null); after = null; signature = '' } }
      if (live) timer = setTimeout(() => void loadHistory(), 1000)
    }
    void loadHistory()
    return () => { live = false; clearTimeout(timer) }
  }, [client, collections, phase])
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
        if (live) {
          setBoot({ snapshot, settings, bandPlan, cwPhone: client.supports('get_cw_state') && client.supports('get_scope_snapshot'),
            keyboard: client.supports('get_rtty_state') && client.supports('get_psk_state'), collections: client.supports(QUERY_COMMAND), insights: client.supports(INSIGHTS_COMMAND) })
          setError(false); timer = setTimeout(() => void load(), 2000)
        }
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
        <RemoteCollectionsContext.Provider value={boot.collections ? collections : null}>
          <RemoteHistoryContext.Provider value={boot.collections ? history : null}>
            <App remote={{ ...boot, status, stale }} />
          </RemoteHistoryContext.Provider>
        </RemoteCollectionsContext.Provider>
      </StationDataContext.Provider>
    </StationControlContext.Provider>
  </ErrorBoundary>
}

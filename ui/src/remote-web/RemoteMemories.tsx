import { useContext, useEffect, useState } from 'react'
import { emptyBank } from '../features/memories'
import { MemoriesView } from '../components/MemoriesView'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { MEMORIES_COMMAND } from './application-query-protocol'
import { parseMemories, type RemoteMemoryBank } from './memories'
import { MEMORY_BANK_TTL_MS } from '../remote-native/memoryBank'

const EMPTY_BANK = emptyBank()

export function RemoteMemories({ myGrid }: { myGrid: string }) {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const supported = source?.client.supports(MEMORIES_COMMAND) ?? false
  const [capture, setCapture] = useState<{ value: RemoteMemoryBank; at: number } | null>(null)
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
    void source.page({ collection: 'memories', cursor: null, search: '', unconfirmed: false, after: null })
      .then(page => {
        const value = parseMemories(page)
        if (live) { setCapture({ value, at: started - value.capturedAgeMs }); setPhase('ready'); setNow(performance.now()) }
      }).catch(() => { if (live) { setCapture(null); setPhase('unavailable') } })
    return () => { live = false }
  }, [source, available, supported, refresh])
  const age = capture ? capture.value.sourceAgeMs + Math.max(0, now - capture.at) : 0
  const expired = age >= MEMORY_BANK_TTL_MS
  const value = available && supported && !expired ? capture?.value : null
  return <div className="remote-insights-view remote-memories-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : expired ? t('remote.memoriesExpired') : value
        ? t('remote.memoriesSnapshot', { seconds: Math.floor(age / 1000) }) : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n => n + 1)}>{t('remote.memoriesRefresh')}</button>
      <span>{t('remote.memoriesObserver')}</span>
    </div>
    {/* Keep view choices while clearing all station values during refresh/loss. */}
    <div className="remote-memory-bank" hidden={!value}>
      <MemoriesView observation={value?.bank ?? EMPTY_BANK} dialMhz={0} dialMode="" myGrid={myGrid} />
    </div>
  </div>
}

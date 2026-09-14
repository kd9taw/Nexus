import { useContext, useEffect, useState } from 'react'
import { PotaSotaView, type OtaRemote, type OtaSpotClickArg } from '../components/PotaSotaView'
import type { ObservedOta } from '../otaHunt'
import { sendLogChange, useLogChange, useRemoteOperations } from './operations'
import { confirmDialog } from '../confirm'
import { pushToast } from '../toast'
import type { AppSnapshot } from '../types'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { OTA_COMMAND } from './application-query-protocol'
import { parseOta, OTA_CAPTURE_TTL_MS, OTA_SOURCE_TTL_MS } from './ota'

const EMPTY: ObservedOta = { feeds: [
  { program: 'POTA', status: 'unavailable', sourceAgeMs: null, spots: [] },
  { program: 'SOTA', status: 'unavailable', sourceAgeMs: null, spots: [] },
], activation: { program: null, reference: null, qsoCount: 0 }, hunt: null, parkCount: 0, huntedCount: 0 }

export function RemoteOta({ snap, onHunt }: { snap: AppSnapshot; onHunt?: (arg: OtaSpotClickArg) => void }) {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const client = useRemoteOperations(), canHunt = useLogChange('otaHunt'), canActivate = useLogChange('otaActivation')
  const canSelfSpot = useLogChange('selfSpot')
  const supported = source?.client.supports(OTA_COMMAND) ?? false
  const [capture, setCapture] = useState<{ value: ObservedOta; at: number } | null>(null)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'unavailable'>('loading')
  const [refresh, setRefresh] = useState(0), [now, setNow] = useState(() => performance.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(performance.now()), 1000)
    return () => clearInterval(timer)
  }, [])
  useEffect(() => {
    setCapture(null)
    if (!source || !available || !supported) { setPhase('unavailable'); return }
    let live = true
    const started = performance.now()
    setPhase('loading')
    void source.page({ collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null }).then(page => {
      const value = parseOta(page)
      if (live) { setCapture({ value, at: started - value.capturedAgeMs }); setPhase('ready'); setNow(performance.now()) }
    }).catch(() => { if (live) { setCapture(null); setPhase('unavailable') } })
    return () => { live = false }
  }, [source, available, supported, refresh])
  const age = capture ? Math.max(0, now - capture.at) : 0
  const value = available && supported && capture && age < OTA_CAPTURE_TTL_MS ? capture.value : null
  const observed: ObservedOta = value ? { ...value, feeds: value.feeds.map(feed => {
    const sourceAgeMs = feed.sourceAgeMs === null ? null : feed.sourceAgeMs + age
    return feed.status === 'ready' && Number(sourceAgeMs) >= OTA_SOURCE_TTL_MS
      ? { ...feed, status: 'expired', sourceAgeMs, spots: [] } : { ...feed, sourceAgeMs }
  }) } : EMPTY
  // A hunt is a station change under the logging grant. Only once the station has tagged it does the
  // spot go on to App's own tune path, which keeps its own permission; a refused hunt never tunes.
  // Each action is present only while the station offers it; your activation is station context too.
  const reread = (outcome: { outcome: string } | null) => { if (outcome?.outcome === 'applied') setRefresh(n => n + 1) }
  const remote: OtaRemote | undefined = client ? {
    ...(canHunt ? {
      hunt: (arg: OtaSpotClickArg) => void sendLogChange(client, { kind: 'hunt', call: arg.call, program: arg.program as 'POTA' | 'SOTA', reference: arg.reference })
        .then(outcome => { if (outcome?.outcome === 'applied') { setRefresh(n => n + 1); onHunt?.(arg) } }),
      clearHunt: () => void sendLogChange(client, { kind: 'clearHunt' }).then(reread),
    } : {}),
    ...(canActivate ? {
      startActivation: (program: string, reference: string) =>
        void sendLogChange(client, { kind: 'activation', program: program as 'POTA' | 'SOTA', reference }).then(reread),
      stopActivation: () => void sendLogChange(client, { kind: 'clearActivation' }).then(reread),
    } : {}),
    ...(canSelfSpot && observed.activation.reference ? {
      // A public post from the station's own call: ask on every click, never remember the answer,
      // and send exactly the reference and dial the question showed.
      selfSpot: () => void (async () => {
        const reference = observed.activation.reference ?? '', dialHz = Math.round(snap.radio.dialMhz * 1e6)
        if (!(await confirmDialog({ title: t('remote.selfSpotConfirm.title'), confirmLabel: t('remote.selfSpotConfirm.post'),
          body: t('remote.selfSpotConfirm.body', { reference, freq: (dialHz / 1e6).toFixed(4) }) }))) return
        const outcome = await sendLogChange(client, { kind: 'selfSpot', reference, dialHz })
        if (outcome?.outcome === 'applied') pushToast(t('remote.selfSpotPosted'), 'success')
      })(),
    } : {}),
  } : undefined
  return <div className="remote-insights-view remote-ota-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : value
        ? t('remote.otaSnapshot', { seconds: Math.floor(age / 1000) }) : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n => n + 1)}>{t('remote.otaRefresh')}</button>
      <span>{t('remote.otaObserver')}</span>
    </div>
    {value && <div className="remote-ota-feeds" role="status">{observed.feeds.map(feed => <span key={feed.program}>
      {feed.program}: {feed.status === 'ready' ? t('remote.otaFeedAge', { seconds: Math.floor(Number(feed.sourceAgeMs) / 1000) })
        : feed.status === 'expired' ? t('remote.otaFeedExpired') : t('remote.otaFeedUnavailable')}
    </span>)}</div>}
    <div className="remote-ota-bank" hidden={!value}>
      <PotaSotaView snap={snap} observation={observed} remote={remote} />
    </div>
  </div>
}

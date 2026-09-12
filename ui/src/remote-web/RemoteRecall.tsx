import { RemoteLogEntry, useRemoteOperations } from './operations'
import { useContext, useEffect, useState } from 'react'
import type { AppSnapshot } from '../types'
import { RecallPanel } from '../components/RecallPanel'
import { bandKey, modeKey } from '../features/callHistory'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { RECALL_COMMAND } from './application-query-protocol'
import { parseRecall } from './recall'
import type { Recall } from './recall'

export type RecallProps = { snap: AppSnapshot; call: string; mode: string; bounded?: boolean; onOpenLog?: (call: string) => void; context?: { band: string; freqMhz: number; mode: string } }
export type RemoteRecallEntryProps = Omit<RecallProps, 'call' | 'bounded'> & { selectedCall?: string; pendingWork?: { call: string; ts: number } | null; onConsumeWork?: () => void }
export function RemoteRecall({ snap, call, mode, bounded, onOpenLog, context }: RecallProps) {
  const source = useContext(RemoteCollectionsContext)
  const available = useStationData()
  const cu = call.trim().toUpperCase()
  const [result, setResult] = useState<Recall | null>(null)
  const [error, setError] = useState(false)
  const [refresh, setRefresh] = useState(0)
  const supported = source?.client.supports(RECALL_COMMAND) ?? false
  useEffect(() => {
    setResult(null); setError(false)
    if (!source || !supported || !available || !/^[A-Z0-9/]{3,32}$/.test(cu)) return
    let live = true
    // Selecting/typing is local. Only a settled call spends a bounded read.
    const timer = setTimeout(() => {
      void source.page({ collection: 'recall', cursor: null, search: cu, unconfirmed: false, after: null })
        .then(page => { const value = parseRecall(page, cu); if (live) setResult(value) })
        .catch(() => { if (live) setError(true) })
    }, 300)
    return () => { live = false; clearTimeout(timer) }
  }, [source, supported, available, cu, refresh])
  if (!supported) return <p className="dim" role="status">{t('remote.recallUnsupported')}</p>
  if (cu.length < 3) return null
  const retry = <button type="button" className="le-qrz le-lookup" disabled={!available} onClick={() => setRefresh(n => n + 1)}>{t('remote.recallRefresh')}</button>
  const value = available && !error && result?.call === cu ? result : null
  if (!value) return <div className={`recall-card${bounded ? ' cockpit-recall' : ''}`}>
    <strong>{cu}</strong><p className="dim" role="status">{t(error || !available || !/^[A-Z0-9/]{3,32}$/.test(cu) ? 'remote.collectionUnavailable' : 'remote.collectionLoading')}</p>{retry}
  </div>
  const band = context?.band ?? snap.radio.band, logMode = context?.mode ?? mode
  const liveBand = bandKey({ band, freqMhz: context?.freqMhz ?? snap.radio.dialMhz })
  const dupe = value.workedBandModes.some(([b, m]) => b === band.trim().toLowerCase() && b !== '' && (!snap.b4MatchMode || m === logMode.trim().toUpperCase()))
  const newBandSlot = value.slots.workedEver && !value.slots.bandUnknown && liveBand !== null && !value.slots.bandsWorked.includes(liveBand)
  const station = snap.stations.find(s => s.call.trim().toUpperCase() === cu)
  const latest = value.rows[0]
  return <RecallPanel call={cu} band={band} myGrid={snap.mygrid}
    name={latest?.name} qth={latest?.qth} grid={station?.grid || latest?.grid} country={value.entity}
    hist={{ ...value.history, qsos: value.rows, dupeThisBand: dupe }}
    newEntity={Boolean(value.entity?.trim()) && !value.slots.workedEver}
    newBandSlot={newBandSlot} newModeSlot={value.slots.workedEver && !newBandSlot && !value.slots.modesWorked.includes(modeKey(logMode))}
    latestNote={value.latestNote} hasLookup={false} bounded={bounded} onOpenLog={onOpenLog}
    historyNotice={<div className="dim" role="status"><p>{t('remote.recallSnapshot')}</p>
      {value.rows.length < value.history.count && <p>{t('remote.collectionCapped', { count: value.rows.length, total: value.history.count })}</p>}{retry}</div>} />
}

/** The existing log pane hosts an observer's local callsign and the same recall card. */
export function ObserverRecallEntry({ snap, mode, onOpenLog, selectedCall }: Omit<RecallProps, 'call' | 'bounded'> & { selectedCall?: string }) {
  const [call, setCall] = useState('')
  useEffect(() => { if (selectedCall) setCall(selectedCall) }, [selectedCall])
  return <div className="log-entry">
    <div className="le-row"><input className="settings-input mono le-call" value={call} maxLength={32}
      aria-label={t('logEntry.call.placeholder')} placeholder={t('logEntry.call.placeholder')}
      autoComplete="off" spellCheck={false} onChange={e => setCall(e.target.value.toUpperCase())} /></div>
    <p className="dim">{t('remote.recallObserver')}</p>
    <RemoteRecall snap={snap} call={call} mode={mode} onOpenLog={onOpenLog} />
  </div>
}

export function RemoteRecallEntry(props:RemoteRecallEntryProps) {
  const operations=useRemoteOperations()
  return operations?.enabled?<RemoteLogEntry {...props} client={operations}/>:<ObserverRecallEntry {...props}/>
}

import { useContext, useEffect, useState } from 'react'
import { FieldDayView } from '../components/FieldDayView'
import type { FdDisplaySettings, FieldDayObservation } from '../fieldDayObservation'
import { useStationData } from '../stationAccess'
import { t } from '../i18n'
import { RemoteCollectionsContext } from './collections'
import { FIELD_DAY_COMMAND } from './application-query-protocol'
import { parseFieldDay, FIELD_DAY_TTL_MS } from './field-day'

const EMPTY: FdDisplaySettings = { fdOperator: '', fdPowerMult: 1, fdBonuses: [], fdBonusesPlanned: [] }
export function RemoteFieldDay({ tier }: { tier: string }) {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const supported = source?.client.supports(FIELD_DAY_COMMAND) ?? false
  const [capture,setCapture] = useState<{value:FieldDayObservation;at:number}|null>(null)
  const [phase,setPhase] = useState<'loading'|'ready'|'unavailable'|'tooLarge'>('loading')
  const [refresh,setRefresh] = useState(0), [now,setNow] = useState(() => performance.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(performance.now()),1000)
    return () => clearInterval(timer)
  },[])
  useEffect(() => {
    setCapture(null)
    if (!source || !available || !supported) { setPhase('unavailable'); return }
    let live = true
    const started = performance.now()
    setPhase('loading')
    void source.page({collection:'fieldDay',cursor:null,search:'',unconfirmed:false,after:null}).then(page => {
      const value = parseFieldDay(page)
      if (live) { setCapture({value,at:started-value.capturedAgeMs});setPhase('ready');setNow(performance.now()) }
    }).catch(error => { if(live) {setCapture(null);setPhase(error instanceof Error && error.message === 'applicationTooLarge' ? 'tooLarge' : 'unavailable')} })
    return () => {live=false}
  },[source,available,supported,refresh])
  const age = capture ? Math.max(0,now-capture.at) : 0
  const value = available && supported && capture && age < FIELD_DAY_TTL_MS ? capture.value : null
  const fieldDay = value?.fieldDay ? {...value.fieldDay,club:value.fieldDay.club ? {...value.fieldDay.club,
    board:value.fieldDay.club.board.map(row => ({...row,lastSeenSecs:row.lastSeenSecs+Math.floor(age/1000)}))} : value.fieldDay.club} : null
  return <div className="remote-insights-view remote-field-day-view">
    <div className="remote-insights-status" role="status">
      <span>{phase === 'loading' && available ? t('remote.collectionLoading') : value
        ? t('remote.fieldDaySnapshot',{seconds:Math.floor(age/1000)}) : phase === 'tooLarge' ? t('remote.fieldDayTooLarge') : t('remote.collectionUnavailable')}</span>
      <button type="button" className="le-qrz le-lookup" disabled={!available || !supported || phase === 'loading'}
        onClick={() => setRefresh(n=>n+1)}>{t('remote.fieldDayRefresh')}</button>
      <span>{t('remote.fieldDayObserver')}</span>
    </div>
    {value && !fieldDay && <p className="remote-view-unavailable">{t('remote.fieldDayInactive')}</p>}
    <main className="layout single remote-field-day-bank" hidden={!fieldDay}>
      <FieldDayView fieldDay={fieldDay} observation={value?.settings ?? EMPTY} fdActive={value?.active ?? false} fdRuleset={value?.ruleset ?? null} tier={tier}/>
    </main>
  </div>
}

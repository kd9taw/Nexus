import type { AprsHealth, AprsHeard, AprsIsStatus, AprsStation, AprsStationsView } from '../api'
import type { Settings } from '../types'
import type { RemoteCollections } from './collections'
import { bool, captureClock, finite, integer, nullableNumber, nullableTime, object, rows, text } from './display-validation'

export type AprsSettings = Pick<Settings,'mygrid'|'aprsChannelMhz'|'aprsComment'|'aprsPath'|'aprsSymbolTable'|'aprsSymbolCode'|'aprsIsEnabled'|'aprsIsRadiusKm'|'aprsIsWatchCalls'>
export type AprsLive = { health: AprsHealth; isStatus: AprsIsStatus; settings: AprsSettings }
export function parseAprsLive(raw: unknown, ageMs: number): AprsLive {
  const sample=object(raw,['health','isStatus','settings','capturedAtMs'])
  if (!integer(sample.capturedAtMs) || !finite(ageMs) || ageMs<0 || ageMs>=3000 || new TextEncoder().encode(JSON.stringify(raw)).length>32*1024) throw new Error('invalidAprs')
  const h=object(sample.health,['arm','audioPeak','lastAudioUnix','drains','framesSeen','framesDecoded','lastDecodeUnix','lastFrameSeenUnix','framePeak','maxFramePeak','frameClippedSamples','radioName','bandRadioCount'])
  if (!['off','auto','explicit'].includes(String(h.arm)) || ![h.audioPeak,h.framePeak,h.maxFramePeak].every(finite) ||
    ![h.lastAudioUnix,h.lastDecodeUnix,h.lastFrameSeenUnix].every(nullableTime) ||
    ![h.drains,h.framesSeen,h.framesDecoded,h.frameClippedSamples,h.bandRadioCount].every(integer) || !text(h.radioName,256)) throw new Error('invalidAprs')
  const i=object(sample.isStatus,['enabled','connected','verified','packets','lastPacketUnix','uplinkEnabled','uploaded','gateRejected','lastReject'])
  if (![i.enabled,i.connected,i.verified,i.uplinkEnabled].every(bool) || ![i.packets,i.uploaded,i.gateRejected].every(integer) ||
    !nullableTime(i.lastPacketUnix) || (i.lastReject!==null&&!text(i.lastReject))) throw new Error('invalidAprs')
  const s=object(sample.settings,['mygrid','aprsChannelMhz','aprsComment','aprsPath','aprsSymbolTable','aprsSymbolCode','aprsIsEnabled','aprsIsRadiusKm','aprsIsWatchCalls'])
  if (!text(s.mygrid,12) || !nullableNumber(s.aprsChannelMhz) || !text(s.aprsComment,256) ||
    !rows(s.aprsPath,16).every(p=>text(p,80)) || !text(s.aprsSymbolTable,4) || !text(s.aprsSymbolCode,4) ||
    !bool(s.aprsIsEnabled) || !finite(s.aprsIsRadiusKm) || !rows(s.aprsIsWatchCalls,100).every(p=>text(p,80))) throw new Error('invalidAprs')
  const value={health:sample.health,isStatus:sample.isStatus,settings:sample.settings} as AprsLive
  captureClock(value,sample.capturedAtMs,ageMs)
  return value
}
function weather(raw: unknown): void {
  if (raw===null)return
  const w=object(raw,['windDirDeg','windMph','gustMph','tempF','rain1hIn100','rain24hIn100','rainMidnightIn100','humidityPct','pressureTenthHpa'])
  if (!Object.values(w).every(nullableNumber)) throw new Error('invalidAprs')
}
const common=['lat','lon','symbolTable','symbolCode','kind','text','speedKnots','courseDeg','path','raw','sourceKind','wx']
function packetOrStation(raw: unknown, station: boolean): AprsHeard|AprsStation {
  const v=object(raw,[...common,...(station?['call','lastHeardUnix','lastRfUnix','lastInetUnix','packets','firstHeardUnix']:['source','dest','addressee','msgId','atUnix'])])
  if (!nullableNumber(v.lat) || (v.lat!==null&&Math.abs(Number(v.lat))>90) || !nullableNumber(v.lon) || (v.lon!==null&&Math.abs(Number(v.lon))>180) ||
    ![v.symbolTable,v.symbolCode].every(v=>text(v,4)&&[...String(v)].length===1) || !text(v.kind,32) || !text(v.text) || !text(v.raw,2048) ||
    !rows(v.path,16).every(v=>text(v,80)) || !['rf','inet','both'].includes(String(v.sourceKind)) || ![v.speedKnots,v.courseDeg].every(nullableNumber)) throw new Error('invalidAprs')
  weather(v.wx)
  if (station) {
    if (!text(v.call,80) || ![v.lastHeardUnix,v.firstHeardUnix,v.packets].every(integer) || ![v.lastRfUnix,v.lastInetUnix].every(nullableTime) ||
      v.sourceKind!==(v.lastRfUnix!==null?(v.lastInetUnix!==null?'both':'rf'):'inet') || (v.lastRfUnix===null&&v.lastInetUnix===null)) throw new Error('invalidAprs')
  } else if (![v.source,v.dest].every(v=>text(v,80)) || !integer(v.atUnix) || [v.addressee,v.msgId].some(v=>v!==null&&!text(v,80))) throw new Error('invalidAprs')
  return raw as AprsHeard|AprsStation
}
export type AprsRoster = { heard: AprsHeard[]; roster: AprsStationsView; capturedAgeMs: number }
export async function loadAprsRoster(source: RemoteCollections, current:()=>boolean): Promise<AprsRoster> {
  let cursor:string|null=null,snapshot='',offset=0,signature='',age=0
  let meta:Record<string,unknown>|null=null
  const heard:AprsHeard[]=[], stations:AprsStation[]=[],calls=new Set<string>(),started=performance.now()
  do {
    if(!current())throw new Error('applicationUnavailable')
    const page=await source.page({collection:'aprs',cursor,search:'',unconfirmed:false,after:null})
    if(!current())throw new Error('applicationUnavailable')
    const envelope=object(page.meta,['source','capturedAgeMs']),m=object(envelope.source,['capturedAtMs','ttlMin','fadeAfterMin','packets','stations'])
    if (!Object.values(m).every(integer) || !integer(envelope.capturedAgeMs) || envelope.capturedAgeMs>=60_000 || page.total!==page.retained || page.total!==Number(m.packets)+Number(m.stations) ||
      Number(m.packets)>300 || Number(m.stations)>2000 || page.offset!==offset || offset+page.rows.length>page.total ||
      (!page.rows.length&&page.nextCursor!==null) || (snapshot&&snapshot!==page.snapshotId) || (signature&&signature!==JSON.stringify(m))) throw new Error('invalidAprs')
    meta=m;snapshot=page.snapshotId;signature=JSON.stringify(m);age=envelope.capturedAgeMs+page.ageMs
    for (const row of page.rows) {
      const r=object(row,['kind','value'])
      const isStation=offset++>=Number(m.packets)
      if (r.kind!==(isStation?'station':'packet'))throw new Error('invalidAprs')
      if(isStation){const s=packetOrStation(r.value,true) as AprsStation;if(calls.has(s.call))throw new Error('invalidAprs');calls.add(s.call);stations.push(s)}
      else heard.push(packetOrStation(r.value,false) as AprsHeard)
    }
    cursor=page.nextCursor
  }while(cursor)
  if(!meta || heard.length!==meta.packets || stations.length!==meta.stations)throw new Error('invalidAprs')
  const value={heard,roster:{stations,ttlMin:Number(meta.ttlMin),fadeAfterMin:Number(meta.fadeAfterMin)},capturedAgeMs:age+performance.now()-started}
  captureClock(value,Number(meta.capturedAtMs),value.capturedAgeMs)
  return value
}

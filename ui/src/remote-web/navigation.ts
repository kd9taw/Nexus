import type { GettingOut, PathPrediction, PropagationSnapshot, NoaaScalesView, AlertView, MufStation, SatView, SatDetail, SatTrackStatus, SatTransponderHeld, SatVfoMap, PcaView, AuroraPoint, SatPass } from '../types'
import type { RemoteCollections } from './collections'
import type { NAVIGATION_COLLECTIONS } from './application-query-protocol'
import { captureClock, finite, integer, object, text } from './display-validation'
import { SAT_VFO_MAPS } from '../features/satVfo'
import { streamId } from './application-stream-protocol'

type Kind = typeof NAVIGATION_COLLECTIONS[number]
export type Feed<T> = { value:T; ageMs:number; validForMs:number }
export type ConnectData = {mycall:string;mygrid:string;prop:PropagationSnapshot;sourceAgeMs:number;sourceValidForMs:number;
  bandOutlook:PathPrediction|null;gettingOut:GettingOut;scales:Feed<[NoaaScalesView,AlertView[]]>|null;muf:Feed<MufStation[]>|null;
  aurora:Feed<AuroraPoint[]>|null;pca:Feed<PcaView>|null;declination:number|null;coverage:{grids:string[];zones:number[];logCount:number};xray:{flux:number;asOf:number}|null}
export type PathData = {mygrid:string;grid:string;prediction:PathPrediction|null;sourceAgeMs:number}
export type SatelliteData = {mygrid:string;view:SatView|null}
export type SatelliteDetailData = {mygrid:string;name:string;detail:SatDetail;schedule:SatPass[];logCount:number}
export type SatelliteSettings = {mygrid:string;rotatorConfigured:boolean;satDopplerOff:boolean;satVfoMap:SatVfoMap;radioPegged:boolean}
export type SatelliteLive = {track:SatTrackStatus|null;held:SatTransponderHeld|null;settings:SatelliteSettings}
export type NavigationDocument<T> = {value:T;ageMs:number;validForMs:number;capturedAtMs:number;stationContextId:string}
const encoder=new TextEncoder()
const bad=():never=>{throw new Error('invalidNavigation')}
/** Display payloads contain no executable values, prototype keys, deep trees or
 * unbounded strings/collections. Exact source DTO roots select the renderer. */
function boundedTree(raw:unknown):void {
  let nodes=0,bytes=0
  function visit(v:unknown,depth:number):void {
    if(++nodes>240_000||depth>16)bad()
    if(typeof v==='string'){if(!text(v,16*1024))bad();bytes+=encoder.encode(v).length;if(bytes>2*1024*1024)bad();return}
    if(v===null||typeof v==='boolean')return
    if(typeof v==='number'){if(!finite(v))bad();return}
    if(Array.isArray(v)){if(v.length>65_536)bad();for(const x of v)visit(x,depth+1);return}
    if(typeof v!=='object'||!v||Object.keys(v).length>128)bad()
    for(const [key,x]of Object.entries(v as object)){
      if(['__proto__','prototype','constructor'].includes(key)||!text(key,128))bad()
      visit(x,depth+1)
    }
  }
  visit(raw,0)
}
function record(v:unknown):Record<string,unknown>{if(!v||typeof v!=='object'||Array.isArray(v))bad();return v as Record<string,unknown>}
function array(v:unknown,max=5000):unknown[]{if(!Array.isArray(v)||v.length>max)bad();return v as unknown[]}
function prediction(v:unknown):void{
  if(v===null)return
  const p=object(v,['engine','bands','mufNow','mufHourly'])
  if(!text(p.engine,32)||!finite(p.mufNow)||!array(p.mufHourly,24).every(finite))bad()
  for(const raw of array(p.bands,32)){
    const b=record(raw);if(!text(b.band,16)||!text(b.workability,32)||!array(b.hourly,24).every(finite))bad()
  }
}
function prop(v:unknown):void{
  const p=record(v)
  if(!['live','partial','cached'].includes(String(p.source))||!integer(p.asOf))bad()
  record(p.advisory);record(p.spaceWx);const dx=record(p.dxpeditions)
  for(const key of ['workableNow','upcoming','active'])array(dx[key])
  for(const key of ['openings','spots','insights','bestToRegion','regionBand'])array(p[key])
  for(const spot of array(p.spots)){const s=record(spot);if(!text(s.call,64)||!finite(s.lat)||!finite(s.lon))bad()}
}
function pass(v:unknown):void{
  const p=record(v)
  if(!text(p.name,80)||!integer(p.aosUnix)||!integer(p.losUnix)||!finite(p.maxElDeg)||!finite(p.aosAzDeg)||!finite(p.losAzDeg))bad()
}
function satelliteView(v:unknown):void {
  if(v===null)return
  const s=object(v,['tleAgeDays','usableCount','agingCount','heldBackCount','tleFetchedAt','tleSource','birds','passes','excluded'])
  if(!finite(s.tleAgeDays)||![s.usableCount,s.agingCount,s.heldBackCount,s.tleFetchedAt].every(integer)||!text(s.tleSource,80))bad()
  for(const raw of array(s.birds,1024)){
    const b=record(raw);if(!text(b.name,80)||![b.lat,b.lon,b.altKm,b.footprintKm].every(finite))bad()
    for(const rawPoint of array(b.track,120)){const p=array(rawPoint,3);if(p.length!==3||!p.every(finite))bad()}
  }
  array(s.passes).forEach(pass)
  for(const raw of array(s.excluded,4096)){const e=record(raw);if(!text(e.name,80)||!text(e.reason,80))bad()}
}
function parse(raw:unknown,kind:Kind,search:string):ConnectData|PathData|SatelliteData|SatelliteDetailData{
  boundedTree(raw)
  if(kind==='connect'){
    const v=object(raw,['mycall','mygrid','prop','sourceAgeMs','sourceValidForMs','bandOutlook','gettingOut','scales','muf','aurora','pca','declination','coverage','xray'])
    if(!text(v.mycall,64)||!text(v.mygrid,16)||!integer(v.sourceAgeMs)||!integer(v.sourceValidForMs)||v.sourceValidForMs>300_000)bad()
    prop(v.prop);prediction(v.bandOutlook)
    const c=object(v.coverage,['grids','zones','logCount']);if(!integer(c.logCount)||!array(c.grids,65_536).every(x=>text(x,16))||!array(c.zones,40).every(x=>integer(x)&&x>=1&&x<=40)||(v.declination!==null&&!finite(v.declination)))bad()
    const g=object(v.gettingOut,['count','maxKm','reports']);if(!integer(g.count)||!integer(g.maxKm))bad();array(g.reports)
    for(const name of ['scales','muf','aurora','pca'])if(v[name]!==null){
      const f=object(v[name],['value','ageMs','validForMs']);if(!integer(f.ageMs)||!integer(f.validForMs)||f.validForMs>900_000)bad()
    }
    if(v.xray!==null){const x=object(v.xray,['flux','asOf']);if(!finite(x.flux)||!integer(x.asOf))bad()}
    return raw as ConnectData
  }
  if(kind==='path'){
    const v=object(raw,['mygrid','grid','prediction','sourceAgeMs'])
    if(!text(v.mygrid,16)||v.grid!==search||!integer(v.sourceAgeMs))bad();prediction(v.prediction);return raw as PathData
  }
  if(kind==='satellites'){
    const v=object(raw,['mygrid','view']);if(!text(v.mygrid,16))bad();satelliteView(v.view);return raw as SatelliteData
  }
  const v=object(raw,['mygrid','name','detail','schedule','logCount']);if(!text(v.mygrid,16)||v.name!==search)bad()
  const d=record(v.detail);if(d.name!==search||!Array.isArray(d.transmitters)||!Array.isArray(d.passTrack))bad()
  if(d.pass!==null)pass(d.pass)
  array(v.schedule,256).forEach(pass);if(!integer(v.logCount))bad()
  return raw as SatelliteDetailData
}
export function parseSatelliteLive(raw:unknown,ageMs:number):SatelliteLive{
  boundedTree(raw)
  if(!finite(ageMs)||ageMs<0||ageMs>=3000||encoder.encode(JSON.stringify(raw)).length>32*1024)bad()
  const v=object(raw,['capturedAtMs','settings','track','held'])
  const s=object(v.settings,['mygrid','rotatorConfigured','satDopplerOff','satVfoMap','radioPegged'])
  if(!integer(v.capturedAtMs)||!text(s.mygrid,16)||![s.rotatorConfigured,s.satDopplerOff,s.radioPegged].every(x=>typeof x==='boolean')||
    !SAT_VFO_MAPS.some(v=>v.value===s.satVfoMap))bad()
  if(v.track!==null){const t=record(v.track);if(!text(t.name,80)||!text(t.state,32)||!text(t.mode,32)||!integer(t.aosUnix)||!integer(t.losUnix))bad()}
  if(v.held!==null){const h=record(v.held);if(!text(h.name,80)||(h.index!==null&&!integer(h.index))||!text(h.description,1024))bad()}
  const value={settings:v.settings,track:v.track,held:v.held} as SatelliteLive
  captureClock(value,Number(v.capturedAtMs),ageMs);return value
}
export async function loadNavigation<T>(source:RemoteCollections,kind:Kind,search:string,current:()=>boolean):Promise<NavigationDocument<T>> {
  let cursor:string|null=null,offset=0,snapshot='',signature='',age=0,total=0,bytes=0
  let meta:Record<string,unknown>|null=null
  const chunks:string[]=[],started=performance.now()
  do{
    if(!current())throw new Error('applicationUnavailable')
    const page=await source.page({collection:kind,cursor,search,unconfirmed:false,after:null})
    if(!current())throw new Error('applicationUnavailable')
    const envelope=object(page.meta,['source','capturedAgeMs'])
    const m=object(envelope.source,['encoding','bytes','chunks','kind','search','contextId','stationContextId','capturedAtMs','validForMs','documentAgeMs'])
    if(m.encoding!=='json-utf8'||m.kind!==kind||m.search!==search||!streamId(m.contextId)||!streamId(m.stationContextId)||
      ![m.bytes,m.chunks,m.capturedAtMs,m.validForMs,m.documentAgeMs,envelope.capturedAgeMs].every(integer)||
      Number(m.bytes)>2*1024*1024||Number(m.chunks)>129||!m.chunks||m.validForMs!==30_000||
      page.total!==page.retained||page.total!==m.chunks||page.offset!==offset||!page.rows.length||
      offset+page.rows.length>page.total||(snapshot&&page.snapshotId!==snapshot)||(signature&&signature!==JSON.stringify(m)))bad()
    meta=m;total=page.total;snapshot=page.snapshotId;signature=JSON.stringify(m)
    age=Number(m.documentAgeMs)+Number(envelope.capturedAgeMs)+page.ageMs
    for(const chunk of page.rows){if(!text(chunk,16*1024)||!chunk)bad();bytes+=encoder.encode(chunk as string).length;if(bytes>Number(m.bytes))bad();chunks.push(chunk as string);offset++}
    cursor=page.nextCursor
  }while(cursor)
  if(!meta||offset!==total||bytes!==meta.bytes)bad()
  age+=performance.now()-started
  if(age>=Number(meta.validForMs))throw new Error('queryExpired')
  const value=parse(JSON.parse(chunks.join('')),kind,search)
  if((kind==='connect'||kind==='path')&&Number((value as ConnectData).sourceAgeMs)+age>=300_000)throw new Error('queryExpired')
  captureClock(value,Number(meta.capturedAtMs),age)
  return {value:value as T,ageMs:age,validForMs:Math.min(Number(meta.validForMs),(kind==='connect'||kind==='path')?300_000-Number((value as ConnectData).sourceAgeMs):Infinity),capturedAtMs:Number(meta.capturedAtMs),stationContextId:String(meta.stationContextId)}
}

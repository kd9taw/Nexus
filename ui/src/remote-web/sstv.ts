import type { BandChannel, SstvState } from '../types'
import type { RemoteCollections } from './collections'
import { sstvImageId } from './application-query-protocol'
import { bool, captureClock, finite, integer, nullableTime, object, rows, text } from './display-validation'

const plans = new WeakMap<SstvState,BandChannel[]>()
export const sstvPlan = (s: SstvState): BandChannel[] => plans.get(s) ?? []
export function parseSstvSample(raw: unknown, ageMs: number): SstvState {
  const sample=object(raw,['state','capturedAtMs','plan'])
  if (!integer(sample.capturedAtMs) || !finite(ageMs) || ageMs<0 || ageMs>=3000 || new TextEncoder().encode(JSON.stringify(raw)).length>256*1024) throw new Error('invalidSstv')
  const s=object(sample.state,['armed','mode','linesDone','linesTotal','previewRgbBase64','previewWidth','previewHeight','hedrShiftHz','gallery','health','sending','txMode','txProgress','txElapsedSecs','txTotalSecs'])
  if (![s.armed,s.sending].every(bool) || ![s.linesDone,s.linesTotal,s.previewWidth,s.previewHeight].every(integer) ||
    ![s.hedrShiftHz,s.txProgress,s.txElapsedSecs,s.txTotalSecs].every(finite) ||
    [s.mode,s.txMode].some(v=>v!==null&&!text(v,80)) || Number(s.previewWidth)>160 || Number(s.previewHeight)>160 ||
    Number(s.linesDone)>Number(s.linesTotal) || Number(s.linesTotal)>1024 || Number(s.txProgress)<0 || Number(s.txProgress)>1) throw new Error('invalidSstv')
  if (s.previewRgbBase64!==null && (!text(s.previewRgbBase64,103000) || atob(s.previewRgbBase64).length!==Number(s.previewWidth)*Number(s.previewHeight)*3)) throw new Error('invalidSstv')
  const ids=new Set<string>()
  for (const v of rows(s.gallery,200)) {
    const g=object(v,['path','mode','finishedUtc','freqMhz','lines',...((v as Record<string,unknown>)?.fskId!==undefined?['fskId']:[])])
    if (!sstvImageId(g.path) || ids.has(g.path) || !text(g.mode,80) || !text(g.finishedUtc,64) || !Number.isFinite(Date.parse(g.finishedUtc)) || !finite(g.freqMhz) || !integer(g.lines) || g.lines>1024 || (g.fskId!=null&&!text(g.fskId,64))) throw new Error('invalidSstv')
    ids.add(g.path)
  }
  const h=object(s.health,['armed','audioPeak','lastAudioUnix','drains','visSeen','lastVisUnix','unknownVis','lastUnknownVisCode','lastUnknownVisUnix','images','lastImageUnix'])
  if (!bool(h.armed) || !finite(h.audioPeak) || ![h.drains,h.visSeen,h.unknownVis,h.images].every(integer) || ![h.lastAudioUnix,h.lastVisUnix,h.lastUnknownVisCode,h.lastUnknownVisUnix,h.lastImageUnix].every(nullableTime)) throw new Error('invalidSstv')
  for (const v of rows(sample.plan,64)) {
    const c=object(v,['band','group','dialMhz','mode','label','note','tx'])
    if (![c.band,c.group,c.mode,c.label,c.note].every(v=>text(v)) || !finite(c.dialMhz) || c.dialMhz<=0 || !bool(c.tx)) throw new Error('invalidSstv')
  }
  const state=sample.state as SstvState
  captureClock(state,sample.capturedAtMs,ageMs)
  plans.set(state,sample.plan as BandChannel[])
  return state
}

/** Read a sealed image with byte, chunk, dimension and digest limits. Never fetch
 * a station path or convert an incoming string into an executable asset URL. */
export async function loadSstvImage(source: RemoteCollections, id: string, current:()=>boolean = ()=>true): Promise<Blob> {
  if (!sstvImageId(id)) throw new Error('invalidSstvImage')
  let cursor:string|null=null, snapshot='', expected=0, length=0, meta:Record<string,unknown>|null=null
  const chunks:Uint8Array<ArrayBuffer>[]=[]
  do {
    if (!current()) throw new Error('applicationUnavailable')
    const page=await source.page({collection:'sstvImage',search:id,cursor,unconfirmed:false,after:null})
    if (!current()) throw new Error('applicationUnavailable')
    const envelope=object(page.meta,['source','capturedAgeMs']), m=object(envelope.source,['imageId','mime','byteLength','sha256','width','height'])
    if (!integer(envelope.capturedAgeMs) || envelope.capturedAgeMs>=60_000 || page.retained!==page.total || page.total<1 || page.total>43 || page.offset!==expected ||
      !page.rows.length || expected+page.rows.length>page.total || page.total!==Math.ceil(Number(m.byteLength)/(48*1024)) ||
      (snapshot && snapshot!==page.snapshotId) || (meta && JSON.stringify(meta)!==JSON.stringify(m)) || m.imageId!==id ||
      m.mime!==(id.endsWith('.bmp')?'image/bmp':'image/png') || !integer(m.byteLength) || !m.byteLength || m.byteLength>2*1024*1024 ||
      typeof m.sha256!=='string' || !/^[a-f0-9]{64}$/.test(m.sha256) || !integer(m.width) || !integer(m.height) || !m.width || !m.height || m.width>1024 || m.height>1024) throw new Error('invalidSstvImage')
    meta=m;snapshot=page.snapshotId
    for (const raw of page.rows) {
      const row=object(raw,['index','base64'])
      if (row.index!==expected++ || !text(row.base64,65536)) throw new Error('invalidSstvImage')
      const data=Uint8Array.from(atob(row.base64),c=>c.charCodeAt(0))
      if (!data.length || data.length>48*1024 || (expected<page.total && data.length!==48*1024)) throw new Error('invalidSstvImage')
      length+=data.length;if(length>Number(m.byteLength))throw new Error('invalidSstvImage')
      chunks.push(data)
    }
    cursor=page.nextCursor
  } while(cursor)
  if (!meta || length!==meta.byteLength) throw new Error('invalidSstvImage')
  const blob=new Blob(chunks,{type:String(meta.mime)})
  const bytes=await blob.arrayBuffer(),view=new DataView(bytes),head=new Uint8Array(bytes)
  const png=meta.mime==='image/png'
  if(png ? bytes.byteLength<33 || [137,80,78,71,13,10,26,10].some((n,i)=>head[i]!==n) ||
    view.getUint32(8)!==13 || view.getUint32(12)!==0x49484452 || view.getUint32(16)!==meta.width || view.getUint32(20)!==meta.height :
    bytes.byteLength<54 || head[0]!==66 || head[1]!==77 || view.getInt32(18,true)!==meta.width || view.getInt32(22,true)!==meta.height ||
    view.getUint32(10,true)!==54 || view.getUint32(14,true)!==40 || view.getUint16(26,true)!==1 || view.getUint16(28,true)!==24 || view.getUint32(30,true)!==0 ||
    54+Math.ceil(Number(meta.width)*3/4)*4*Number(meta.height)!==bytes.byteLength) throw new Error('invalidSstvImage')
  const digest=[...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(b=>b.toString(16).padStart(2,'0')).join('')
  if (!current() || digest!==meta.sha256) throw new Error('invalidSstvImage')
  return blob
}

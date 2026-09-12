import type {QueryPage} from '../application-query-protocol'
const id='8aa041cb-c642-459c-83f3-11a5b720647d'
export function navigationPages(kind:string,value:unknown,search=''):QueryPage[]{
  const body=JSON.stringify(value),chunks=Array.from({length:Math.ceil(body.length/4000)},(_,i)=>body.slice(i*4000,(i+1)*4000))
  const meta={encoding:'json-utf8',bytes:new TextEncoder().encode(body).length,chunks:chunks.length,kind,search,contextId:id,stationContextId:id,
    capturedAtMs:1789012800000,validForMs:30_000,documentAgeMs:0}
  return chunks.map((row,i)=>({type:'applicationPage',requestId:id,collection:kind,snapshotId:id,offset:i,total:chunks.length,retained:chunks.length,
    nextCursor:i+1<chunks.length?`${id}:${i+1}`:null,ageMs:0,rows:[row],meta:{source:structuredClone(meta),capturedAgeMs:0}} as QueryPage))
}

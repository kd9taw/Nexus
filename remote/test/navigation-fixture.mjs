// Native-generated planning DTOs with deterministic synthetic display density.
// No fixture is imported by the production application.
import { readFile } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'

export async function navigationFixture() {
  const load=async name=>JSON.parse(await readFile(new URL(`../../ui/src/remote-web/__fixtures__/navigation-${name}.json`,import.meta.url),'utf8'))
  const [connect,path,satellites,satellite,live]=await Promise.all(['connect','path','satellites','satellite','satellite-live'].map(load))
  const now=Date.now(),delta=Math.floor((now-live.capturedAtMs)/1000),context=randomUUID()
  function shift(value,key='') {
    if(Array.isArray(value)){
      if(value.length===3&&typeof value[0]==='number'&&value[0]>1_000_000_000){value[0]+=delta;return}
      for(const item of value)shift(item,key)
    }else if(value&&typeof value==='object')for(const [name,v]of Object.entries(value)){
      if(typeof v==='number'&&(name.endsWith('Unix')||['asOf','tleFetchedAt','dataFetchedAt'].includes(name))&&v>0)value[name]+=delta
      else shift(v,name)
    }
  }
  for(const data of [connect,path,satellites,satellite])shift(data)
  live.capturedAtMs=now
  connect.prop.spots=[{call:'K2MAP',lat:41.8,lon:-72.9,band:'20m',heardMe:false,ageSecs:2,approx:false,freqMhz:14.025,mode:'CW',entity:'United States'}]
  // Keep the native ISS row/detail as the positive selection. Additional rows
  // stress the real scrolling controls, not SGP4 (the native suite covers that).
  const bird=satellites.view.birds[0],pass=satellites.view.passes[0]
  for(let i=1;i<40;i++){
    satellites.view.birds.push({...structuredClone(bird),name:`LAYOUT-${i}`,norad:90000+i,lat:bird.lat+i/10,lon:bird.lon+i/10})
    satellites.view.passes.push({...structuredClone(pass),name:`LAYOUT-${i}`,norad:90000+i,aosUnix:pass.aosUnix+i*90,losUnix:pass.losUnix+i*90})
  }
  satellites.view.usableCount=40
  const configuration=async kind=>JSON.parse(await readFile(new URL(`../../ui/src/remote-web/__fixtures__/configuration-${kind}.json`,import.meta.url),'utf8'))
  const settings=await configuration('settings'),programming=await configuration('programming')
  const documents={connect,path,satellites,satellite,settings,programming}
  function collection(kind,search='') {
    const value=structuredClone(documents[kind])
    if(kind==='path')value.grid=search
    if(kind==='satellite'){
      value.name=search;value.detail.name=search
      if(search!=='ISS (ZARYA)')for(const p of value.schedule)p.name=search
    }
    const raw=Buffer.from(JSON.stringify(value)),rows=[]
    let start=0
    while(start<raw.length){let end=Math.min(raw.length,start+16*1024);while(end<raw.length&&(raw[end]&0xc0)===0x80)end--;rows.push(raw.subarray(start,end).toString('utf8'));start=end}
    return {rows,meta:{encoding:'json-utf8',bytes:raw.length,chunks:rows.length,kind,search,contextId:randomUUID(),stationContextId:context,capturedAtMs:Date.now(),validForMs:30_000,documentAgeMs:0}}
  }
  return {live,documents,collection}
}

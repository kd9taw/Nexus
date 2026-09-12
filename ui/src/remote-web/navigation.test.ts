import { describe,expect,it,vi } from 'vitest'
import connect from './__fixtures__/navigation-connect.json'
import path from './__fixtures__/navigation-path.json'
import satellites from './__fixtures__/navigation-satellites.json'
import satellite from './__fixtures__/navigation-satellite.json'
import live from './__fixtures__/navigation-satellite-live.json'
import {loadNavigation,parseSatelliteLive} from './navigation'
import type {RemoteCollections} from './collections'
import type {QueryPage} from './application-query-protocol'
import {navigationPages} from './__fixtures__/navigation-page'
import {displayNow} from './display-validation'

const id='8aa041cb-c642-459c-83f3-11a5b720647d'

function source(pages:QueryPage[]):RemoteCollections{return {page:vi.fn(async()=>{const page=pages.shift();if(!page)throw new Error('unexpectedPage');return structuredClone(page)})} as unknown as RemoteCollections}
describe('sealed native planning documents',()=>{
  it.each([['connect',connect,''],['path',path,'PM95'],['satellites',satellites,''],['satellite',satellite,'ISS (ZARYA)']] as const)('reads the actual native %s fixture without changing event times or source data',async(kind,raw,search)=>{
    const result=await loadNavigation(source(navigationPages(kind,raw,search)),kind,search,()=>true)
    expect(result.value).toEqual(raw);expect(result.stationContextId).toBe(id)
    expect(displayNow(result.value as object)).toBeGreaterThanOrEqual(result.capturedAtMs)
  })
  it('retains full-log coverage and needs beyond the displayed collection window',async()=>{
    const result=await loadNavigation<typeof connect>(source(navigationPages('connect',connect)),'connect','',()=>true)
    expect(result.value.coverage.logCount).toBe(2301);expect(result.value.coverage.grids).toContain('PM95')
    expect(satellite.logCount).toBe(2301);expect(satellite.schedule.length).toBeGreaterThan(0)
    expect(JSON.stringify(result)).not.toContain('never-share')
  })
  it('refuses missing/reordered chunks, substituted selections and changed context',async()=>{
    const baseline=navigationPages('satellite',satellite,'ISS (ZARYA)');expect(baseline.length).toBeGreaterThan(1)
    for(const change of [
      (p:QueryPage[])=>{p[0].nextCursor=null},
      (p:QueryPage[])=>{p[1].offset++},
      (p:QueryPage[])=>{(p[1].meta as any).source.stationContextId=crypto.randomUUID()},
      (p:QueryPage[])=>{(p[0].meta as any).source.search='AO-91'},
      (p:QueryPage[])=>{p[0].rows=[]},
      (p:QueryPage[])=>{(p[0].meta as any).source.bytes++},
    ]){const pages=structuredClone(baseline);change(pages);await expect(loadNavigation(source(pages),'satellite','ISS (ZARYA)',()=>true)).rejects.toThrow()}
  })
  it('includes worker, cache and transfer age, and refuses a feed that expired while cached',async()=>{
    const pages=navigationPages('connect',{...connect,sourceAgeMs:299_000})
    for(const p of pages)(p.meta as any).source.documentAgeMs=1500
    await expect(loadNavigation(source(pages),'connect','',()=>true)).rejects.toThrow('queryExpired')
    const fresh=await loadNavigation(source(navigationPages('connect',{...connect,sourceAgeMs:299_000})),'connect','',()=>true)
    expect(fresh.validForMs).toBe(1000)
  })
  it('does not read a second page after cancellation',async()=>{
    const pages=navigationPages('satellite',satellite,'ISS (ZARYA)');let current=true
    const stub={page:vi.fn(async()=>{current=false;return pages[0]})}
    await expect(loadNavigation(stub as unknown as RemoteCollections,'satellite','ISS (ZARYA)',()=>current)).rejects.toThrow('applicationUnavailable')
    expect(stub.page).toHaveBeenCalledTimes(1)
  })
  it('rejects malformed display roots and unsafe object keys',async()=>{
    for(const value of [{...connect,prop:[]},{...connect,coverage:{grids:[],zones:[99],logCount:1}},{...connect,prop:{...connect.prop,spots:[{call:{},lat:0,lon:0}]}},JSON.parse(JSON.stringify(connect).replace('"mycall":','"__proto__":'))]){
      await expect(loadNavigation(source(navigationPages('connect',value)),'connect','',()=>true)).rejects.toThrow()
    }
  })
  it('accepts the native live tracker projection and refuses stale settings or arbitrary fields',()=>{
    expect(parseSatelliteLive(live,100)).toMatchObject({track:null,held:null,settings:{mygrid:'FN31RX09'}})
    for(const bad of [{...live,password:'secret'},{...live,settings:{...live.settings,satVfoMap:'guess'}},{...live,held:{name:'ISS'}}])expect(()=>parseSatelliteLive(bad,0)).toThrow()
    expect(()=>parseSatelliteLive(live,3000)).toThrow()
  })
})

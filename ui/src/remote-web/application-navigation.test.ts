import { describe, expect, it, vi } from 'vitest'
import { applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { NAVIGATION_COLLECTIONS, queryRequest } from './application-query-protocol'
import { streamTopic } from './application-stream-protocol'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'

describe('native planning capabilities',()=>{
  it('adds only the reviewed planner and tracker reads after every earlier contract',()=>{
    expect(applicationCommands(13)).toEqual([...applicationCommands(12),'get_remote_satellite_state','get_remote_navigation'])
    expect(new Set(applicationCommands(13)).size).toBe(applicationCommands(13).length)
    for(let version=1;version<13;version++){
      expect(applicationCommands(version)).not.toContain('get_remote_navigation')
      expect(streamTopic('get_remote_satellite_state',applicationStreamVersion(version))).toBe(false)
    }
    for(const command of ['set_sat_transponder','start_sat_track','stop_sat_track','set_settings','get_propagation','eval','invoke'])expect(applicationCommands(13)).not.toContain(command)
  })
  it('accepts only bounded selections, never a path, arbitrary verb or mutating argument',()=>{
    for(const collection of NAVIGATION_COLLECTIONS){
      const request={type:'applicationQuery',requestId:crypto.randomUUID(),collection,cursor:null,search:collection==='path'?'PM95':collection==='satellite'?'ISS (ZARYA)':'',unconfirmed:false,after:null}
      expect(queryRequest(request,applicationQueryVersion(13))).toEqual(request)
      expect(()=>queryRequest(request,12)).toThrow()
      for(const change of [{path:'/tmp/secret'},{command:'set_frequency'},{unconfirmed:true},{after:1},{search:'\n'},{search:'x'.repeat(97)}])expect(()=>queryRequest({...request,...change},13)).toThrow()
      if(collection==='path')for(const search of ['AA000','SZ99','FN31a','ZZ99','FN31aa','FN31AAA'])expect(()=>queryRequest({...request,search},13)).toThrow()
    }
  })
  it('negotiates all 169 station/browser pairings and refuses planning reads on old stations',async()=>{
    for(let stationVersion=1;stationVersion<=13;stationVersion++)for(let browserVersion=1;browserVersion<=13;browserVersion++){
      const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()}
      const relay=new ApplicationRelay();relay.sync({peer:station,version:stationVersion},[{sessionId:'one',peer:browser}],0)
      const client=new ApplicationClient(s=>relay.receiveBrowser('one',JSON.parse(s),0),vi.fn(),browserVersion)
      client.open();client.receive(JSON.parse(browser.send.mock.lastCall![0]))
      expect(client.supports('get_remote_navigation')).toBe(stationVersion===13&&browserVersion===13)
      if(stationVersion<13||browserVersion<13){
        await expect(client.invoke('get_remote_navigation',{collection:'connect',cursor:null,search:'',unconfirmed:false,after:null})).rejects.toThrow()
        await expect(client.invoke('get_remote_satellite_state')).rejects.toThrow()
        expect(station.send).not.toHaveBeenCalled()
      }
      client.disconnected()
    }
  })
  it('hibernates the planner and tracker without persisting station documents',()=>{
    const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()}
    const peers=[{sessionId:'one',peer:browser}],relay=new ApplicationRelay()
    relay.sync({peer:station,version:13},peers,0);relay.receiveBrowser('one',{type:'applicationHello',version:13},0)
    relay.receiveBrowser('one',{type:'applicationSubscribe',topics:['get_remote_satellite_state'],requestId:crypto.randomUUID()},1)
    const saved=relay.checkpoint('one')!;expect(saved.version).toBe(13)
    const resumed=new ApplicationRelay();resumed.sync({peer:station,version:13},peers,2);resumed.restore('one',saved)
    const request={type:'applicationQuery',requestId:crypto.randomUUID(),collection:'satellite',cursor:null,search:'ISS (ZARYA)',unconfirmed:false,after:null}
    resumed.receiveBrowser('one',request,3)
    expect(JSON.parse(station.send.mock.lastCall![0]).collection).toBe('satellite')
    expect(JSON.stringify(resumed.checkpoint('one'))).not.toContain('ISS (ZARYA)')
    expect(station.close).not.toHaveBeenCalled();expect(browser.close).not.toHaveBeenCalled()
  })
})

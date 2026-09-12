import { expect,it,vi } from 'vitest'
import {applicationCommands,applicationQueryVersion,applicationStreamVersion} from './application-capabilities'
import {CONFIGURATION_COLLECTIONS,queryRequest} from './application-query-protocol'
import {ApplicationClient} from './application-client'
import {ApplicationRelay} from './application-relay'
it('adds exactly a closed configuration read while preserving every old instrument grammar',()=>{
  expect(applicationCommands(14)).toEqual([...applicationCommands(13),'get_remote_configuration'])
  expect(applicationStreamVersion(14)).toBe(13)
  for(let v=1;v<14;v++)expect(applicationCommands(v)).not.toContain('get_remote_configuration')
  for(const collection of CONFIGURATION_COLLECTIONS){
    const request={type:'applicationQuery',requestId:crypto.randomUUID(),collection,cursor:null,search:'',unconfirmed:false,after:null}
    expect(queryRequest(request,applicationQueryVersion(14))).toEqual(request)
    for(const v of [3,6,9,12,13])expect(()=>queryRequest(request,v)).toThrow()
    for(const change of [{path:'/tmp/file'},{command:'set_settings'},{settings:{}},{unconfirmed:true},{after:1},{search:'working'}])expect(()=>queryRequest({...request,...change},14)).toThrow()
  }
})
it('negotiates all 196 station/browser pairs and never sends configuration to an older station',async()=>{
  for(let stationVersion=1;stationVersion<=14;stationVersion++)for(let browserVersion=1;browserVersion<=14;browserVersion++){
    const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()}
    const relay=new ApplicationRelay();relay.sync({peer:station,version:stationVersion},[{sessionId:'one',peer:browser}],0)
    const client=new ApplicationClient(s=>relay.receiveBrowser('one',JSON.parse(s),0),vi.fn(),browserVersion)
    client.open();client.receive(JSON.parse(browser.send.mock.lastCall![0]))
    expect(client.supports('get_remote_configuration')).toBe(stationVersion===14&&browserVersion===14)
    if(stationVersion<14||browserVersion<14){await expect(client.invoke('get_remote_configuration',{collection:'settings',cursor:null,search:'',unconfirmed:false,after:null})).rejects.toThrow();expect(station.send).not.toHaveBeenCalled()}
    client.disconnected()
  }
})
it('retains v14 query authority through hibernation without persisting the station document',()=>{
  const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()},peers=[{sessionId:'one',peer:browser}]
  const relay=new ApplicationRelay();relay.sync({peer:station,version:14},peers,0);relay.receiveBrowser('one',{type:'applicationHello',version:14},0)
  const saved=relay.checkpoint('one')!;expect(saved.version).toBe(14)
  const resumed=new ApplicationRelay();resumed.sync({peer:station,version:14},peers,1);resumed.restore('one',saved)
  resumed.receiveBrowser('one',{type:'applicationQuery',requestId:crypto.randomUUID(),collection:'settings',cursor:null,search:'',unconfirmed:false,after:null},2)
  expect(JSON.parse(station.send.mock.lastCall![0]).collection).toBe('settings');expect(browser.close).not.toHaveBeenCalled()
})

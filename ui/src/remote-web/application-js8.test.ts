import { describe, expect, it } from 'vitest'
import { applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { queryRequest } from './application-query-protocol'
import { streamTopic } from './application-stream-protocol'
import { vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'

describe('JS8 capability negotiation', () => {
  it('adds only reviewed JS8 reads at version 11 and preserves every older vocabulary', () => {
    expect(applicationCommands(11)).toEqual([...applicationCommands(10), 'get_js8_state', 'get_remote_js8_context'])
    expect(streamTopic('get_js8_state', applicationStreamVersion(11))).toBe(true)
    for (let version = 1; version <= 10; version++) {
      expect(applicationCommands(version)).not.toContain('get_js8_state')
      expect(applicationCommands(version)).not.toContain('get_remote_js8_context')
    }
    for (const command of ['js8_enter', 'js8_send', 'js8_arm', 'js8_inbox_delete', 'set_frequency']) {
      expect(applicationCommands(11)).not.toContain(command)
    }
  })
  it('admits a closed context read and refuses cursors, search, and extra fields', () => {
    const request = { type: 'applicationQuery', requestId: '8aa041cb-c642-459c-83f3-11a5b720647d',
      collection: 'js8Context', cursor: null, search: '', unconfirmed: false, after: null }
    expect(queryRequest(request, applicationQueryVersion(11))).toEqual(request)
    expect(() => queryRequest(request, 10)).toThrow()
    for (const change of [{ cursor: `${request.requestId}:128` }, { search: 'W1AW' }, { unconfirmed: true }, { after: 1 }, { path: 'log' }]) {
      expect(() => queryRequest({ ...request, ...change }, applicationQueryVersion(11))).toThrow()
    }
  })
})

it('negotiates every old/new pairing and never sends JS8 reads to an older station', async () => {
  for(let stationVersion=1;stationVersion<=11;stationVersion++)for(let browserVersion=1;browserVersion<=11;browserVersion++){
    const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()}
    const relay=new ApplicationRelay();relay.sync({peer:station,version:stationVersion},[{sessionId:'one',peer:browser}],0)
    const client=new ApplicationClient(s=>relay.receiveBrowser('one',JSON.parse(s),0),vi.fn(),browserVersion)
    client.open();client.receive(JSON.parse(browser.send.mock.lastCall![0]))
    expect(client.supports('get_js8_state')).toBe(stationVersion===11&&browserVersion===11)
    if(stationVersion<11||browserVersion<11){
      await expect(client.invoke('get_js8_state')).rejects.toThrow()
      await expect(client.invoke('get_remote_js8_context',{collection:'js8Context',cursor:null,search:'',unconfirmed:false,after:null})).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('restores JS8 stream and query routing without persisting station contents', () => {
  const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()},old={send:vi.fn(),close:vi.fn()}
  const peers=[{sessionId:'one',peer:browser},{sessionId:'old',peer:old}],relay=new ApplicationRelay()
  relay.sync({peer:station,version:11},peers,0)
  relay.receiveBrowser('one',{type:'applicationHello',version:11},0)
  relay.receiveBrowser('old',{type:'applicationHello',version:10},0)
  const id=crypto.randomUUID()
  relay.receiveBrowser('one',{type:'applicationSubscribe',topics:['get_js8_state'],requestId:id},1)
  const saved=relay.checkpoint('one')!
  expect(saved.version).toBe(11)
  const resumed=new ApplicationRelay();resumed.sync({peer:station,version:11},peers,2)
  expect(()=>resumed.restore('one',saved)).not.toThrow()
  const request={type:'applicationQuery',requestId:crypto.randomUUID(),collection:'js8Context',cursor:null,search:'',unconfirmed:false,after:null}
  resumed.receiveBrowser('one',request,3)
  expect(JSON.parse(station.send.mock.lastCall![0]).collection).toBe('js8Context')
  expect(JSON.stringify(resumed.checkpoint('one'))).not.toContain('STORED REMOTE TEST')
  relay.receiveBrowser('old',{type:'applicationSubscribe',topics:['get_js8_state'],requestId:crypto.randomUUID()},4)
  expect(old.close).toHaveBeenCalled()
  expect(station.close).not.toHaveBeenCalled()
})

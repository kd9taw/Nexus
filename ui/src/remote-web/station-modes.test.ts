import { expect, it, vi } from 'vitest'
import { applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import { streamTopic } from './application-stream-protocol'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { createHash } from 'node:crypto'
import { parseSstvSample, loadSstvImage } from './sstv'
import { parseAprsLive, loadAprsRoster } from './aprs'
import { displayNow } from './display-validation'
import type { RemoteCollections } from './collections'
import sstv from './__fixtures__/sstv.json'
import aprs from './__fixtures__/aprs.json'
import roster from './__fixtures__/aprs-roster.json'

it('negotiates station modes only when every hop supports version 12', async()=>{
  const added=['get_sstv_state','get_remote_aprs_state','get_remote_sstv_image','get_remote_aprs']
  expect(applicationCommands(12)).toEqual([...applicationCommands(11),...added])
  for(let stationVersion=1;stationVersion<=12;stationVersion++)for(let browserVersion=1;browserVersion<=12;browserVersion++){
    const station={send:vi.fn<(s:string)=>void>(),close:vi.fn()},browser={send:vi.fn<(s:string)=>void>(),close:vi.fn()}
    const relay=new ApplicationRelay();relay.sync({peer:station,version:stationVersion},[{sessionId:'one',peer:browser}],0)
    const client=new ApplicationClient(s=>relay.receiveBrowser('one',JSON.parse(s),0),vi.fn(),browserVersion)
    client.open();client.receive(JSON.parse(browser.send.mock.lastCall![0]))
    for(const command of added)expect(client.supports(command)).toBe(stationVersion===12&&browserVersion===12)
    for(const action of ['sstv_arm','sstv_send','sstv_delete_image','aprs_arm','aprs_send_beacon','set_settings']){
      await expect(client.invoke(action)).rejects.toThrow();expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
  expect(streamTopic('get_sstv_state',applicationStreamVersion(11))).toBe(false)
  expect(streamTopic('get_sstv_state',applicationStreamVersion(12))).toBe(true)
})
it('image queries accept only opaque handles and exact cursors, never file paths',()=>{
  const id='8aa041cb-c642-459c-83f3-11a5b720647d'
  const request={type:'applicationQuery',requestId:id,collection:'sstvImage',search:id+'.png',cursor:null,after:null,unconfirmed:false}
  expect(queryRequest(request,applicationQueryVersion(12))).toEqual(request)
  expect(queryRequest({...request,cursor:id+':3'},12).cursor).toBe(id+':3')
  expect(()=>queryRequest(request,11)).toThrow()
  for(const change of [{search:'/home/test/received.png'},{search:'../received.png'},{search:'file:///received.png'},{search:'https://station/file.png'},{search:id+'.svg'},{path:'received.png'},{after:1},{unconfirmed:true}])expect(()=>queryRequest({...request,...change},12)).toThrow()
})
it('preserves SSTV event timestamps, preview pixels and the native band plan',()=>{
  const before=performance.now(),s=parseSstvSample(structuredClone(sstv),150)
  expect(s).toEqual(sstv.state)
  expect(displayNow(s,before+1000)).toBeGreaterThanOrEqual(sstv.capturedAtMs+1100)
  expect(()=>parseSstvSample(sstv,3000)).toThrow()
  for(const change of [{previewWidth:2000},{previewRgbBase64:'AA=='},{gallery:[{...sstv.state.gallery[0],path:'/private/file.png'}]},{sending:'yes'}]){
    expect(()=>parseSstvSample({...sstv,state:{...sstv.state,...change}},0)).toThrow()
  }
})
it('preserves APRS source evidence, health, and the station settings without defaults',()=>{
  const sample=parseAprsLive(structuredClone(aprs),100)
  expect(sample.health).toEqual(aprs.health);expect(sample.settings).toEqual(aprs.settings)
  expect(sample.isStatus.verified).toBe(false)
  expect(()=>parseAprsLive({...aprs,settings:{...aprs.settings,aprsIsPasscode:'secret'}},0)).toThrow()
  expect(()=>parseAprsLive({...aprs,health:{...aprs.health,arm:'enabled'}},0)).toThrow()
  expect(()=>parseAprsLive(aprs,3000)).toThrow()
})
it('reassembles distinct APRS packets and stations and refuses mixed, partial or invalid pages',async()=>{
  const id='8aa041cb-c642-459c-83f3-11a5b720647d'
  const page={collection:'aprs',snapshotId:id,total:6,retained:6,offset:0,nextCursor:null,ageMs:0,rows:roster.rows,meta:{capturedAgeMs:0,source:roster.meta}} as unknown as QueryPage
  const source={page:vi.fn().mockResolvedValue(page)} as unknown as RemoteCollections
  const value=await loadAprsRoster(source,()=>true)
  expect(value.heard).toHaveLength(3);expect(value.roster.stations).toHaveLength(3)
  expect(value.roster.stations.map(s=>s.sourceKind)).toEqual(['rf','inet','both'])
  for(const change of [{retained:5},{total:7},{offset:1},{rows:roster.rows.slice(0,3)},{rows:[{...roster.rows[0],kind:'station'},...roster.rows.slice(1)]}]){
    vi.mocked(source.page).mockResolvedValueOnce({...page,...change} as unknown as QueryPage)
    await expect(loadAprsRoster(source,()=>true)).rejects.toThrow()
  }
  vi.mocked(source.page).mockResolvedValueOnce({...page,rows:roster.rows.slice(0,3),nextCursor:id+':3'} as unknown as QueryPage)
    .mockResolvedValueOnce({...page,snapshotId:'8aa041cb-c642-459c-83f3-11a5b720647e',offset:3,rows:roster.rows.slice(3)} as unknown as QueryPage)
  await expect(loadAprsRoster(source,()=>true)).rejects.toThrow()
})

it('transfers complete image chunks and rejects corruption, reordered bytes and unsafe dimensions',async()=>{
  const id='8aa041cb-c642-459c-83f3-11a5b720647d',bytes=Buffer.alloc(54+320*256*3,127)
  bytes.write('BM');bytes.writeUInt32LE(bytes.length,2);bytes.writeUInt32LE(54,10);bytes.writeUInt32LE(40,14)
  bytes.writeInt32LE(320,18);bytes.writeInt32LE(256,22);bytes.writeUInt16LE(1,26);bytes.writeUInt16LE(24,28);bytes.writeUInt32LE(0,30)
  const rows=Array.from({length:Math.ceil(bytes.length/(48*1024))},(_,index)=>({index,base64:bytes.subarray(index*48*1024,(index+1)*48*1024).toString('base64')}))
  const meta={imageId:id+'.bmp',mime:'image/bmp',byteLength:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex'),width:320,height:256}
  let altered=false
  const source={page:vi.fn(async(args:{cursor:string|null})=>{
    const offset=Number(args.cursor?.split(':')[1]??0),part=rows.slice(offset,offset+3),end=offset+part.length
    return {collection:'sstvImage',snapshotId:id,total:rows.length,retained:rows.length,offset,nextCursor:end<rows.length?id+':'+end:null,ageMs:0,
      rows:part,meta:{capturedAgeMs:0,source:altered?{...meta,sha256:'0'.repeat(64)}:meta}}
  })} as unknown as RemoteCollections
  const blob=await loadSstvImage(source,id+'.bmp')
  expect(Buffer.from(await blob.arrayBuffer())).toEqual(bytes)
  expect(source.page).toHaveBeenCalledTimes(2)
  altered=true;await expect(loadSstvImage(source,id+'.bmp')).rejects.toThrow();altered=false
  rows[0].index=1;await expect(loadSstvImage(source,id+'.bmp')).rejects.toThrow();rows[0].index=0
  meta.width=1025;await expect(loadSstvImage(source,id+'.bmp')).rejects.toThrow();meta.width=319
  await expect(loadSstvImage(source,id+'.bmp')).rejects.toThrow()
  await expect(loadSstvImage(source,id+'.bmp',()=>false)).rejects.toThrow()
})

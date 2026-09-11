// @vitest-environment jsdom
import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { RemoteStationAction, RemoteStationStatus } from './types'

afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })
it('renders local pairing and device approval through only the isolated Remote commands', async () => {
  const accountId = crypto.randomUUID(), pairingId = crypto.randomUUID(), deviceId = crypto.randomUUID()
  let status: RemoteStationStatus = { phase: 'approval', origin: 'https://remote-staging.hamradiotools.io',
    stationId: null, accountId, pairingId, pairingCode: crypto.randomUUID().replace(/-/g,'').slice(0,16),
    expiresAt: Date.now()+600000, devices: [], error: null }
  const actions: RemoteStationAction[] = []
  const invoke = async (command: string, input: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    actions.push(action)
    if (action.type === 'approve') status = { ...status, stationId: pairingId, pairingCode: null, phase: 'disabled',
      devices: [{ id: deviceId, name: 'Test browser', approved: 0, expiresAt: Date.now()+600000 }] }
    if (action.type === 'enable') status = { ...status, phase: 'connected' }
    if (action.type === 'disable') status = { ...status, phase: 'disabled' }
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<form><RemoteStation /></form>)
  await screen.findByText(accountId)
  fireEvent.click(screen.getByRole('button', { name: 'Approve this account pairing' }))
  await screen.findByText(deviceId.slice(-6))
  fireEvent.click(screen.getByRole('button', { name: 'Approve browser' }))
  await waitFor(() => expect((screen.getByRole('button', { name: 'Enable Remote observation' }) as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(screen.getByRole('button', { name: 'Enable Remote observation' }))
  await screen.findByRole('button', { name: 'Disable Remote observation' })
  fireEvent.click(screen.getByRole('button', { name: 'Disable Remote observation' }))
  await screen.findByRole('button', { name: 'Enable Remote observation' })
  expect(actions).toContainEqual({ type: 'approve', accountId, enrollmentId: pairingId })
  expect(actions).toContainEqual({ type: 'device', deviceId, approve: true })
  expect(actions).toContainEqual({ type: 'disable' })
  expect([...document.querySelectorAll('button')].every(button => button.type === 'button')).toBe(true)
})

it('keeps logging approval local, per browser, and provides immediate local takeover',async()=>{
 const deviceId=crypto.randomUUID(),actions:RemoteStationAction[]=[]
 let status:RemoteStationStatus={phase:'connected',origin:'https://remote-staging.hamradiotools.io',stationId:crypto.randomUUID(),accountId:crypto.randomUUID(),pairingId:null,pairingCode:null,expiresAt:null,devices:[{id:deviceId,name:'Approved browser',approved:1,expiresAt:Date.now()+600000}],error:null,loggingPermissions:[],loggingController:null}
 window.__TAURI_INTERNALS__={invoke:(async(command:string,input:unknown)=>{
  if(command==='get_remote_station_status')return status
  if(command!=='remote_station_action')throw Error('unexpectedCommand')
  const action=(input as {action:RemoteStationAction}).action;actions.push(action)
  if(action.type==='loggingPermission')status={...status,loggingPermissions:action.allow?[deviceId]:[]}
  if(action.type==='takeOverLogging')status={...status,loggingPermissions:[],loggingController:null}
  return status
 }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke']}
 render(<RemoteStation/>);fireEvent.click(await screen.findByRole('button',{name:'Allow remote logging'}))
 await screen.findByRole('button',{name:'Revoke logging permission'})
 fireEvent.click(screen.getByRole('button',{name:'End remote logging and clear permissions'}))
 await screen.findByRole('button',{name:'Allow remote logging'})
 expect(actions).toEqual([{type:'loggingPermission',deviceId,allow:true},{type:'takeOverLogging'}])
})

it('keeps station control permission separate from logging and clears both on takeover',async()=>{
 const deviceId=crypto.randomUUID(),actions:RemoteStationAction[]=[]
 let status:RemoteStationStatus={phase:'connected',origin:'https://remote-staging.hamradiotools.io',stationId:crypto.randomUUID(),accountId:crypto.randomUUID(),pairingId:null,pairingCode:null,expiresAt:null,devices:[{id:deviceId,name:'Approved browser',approved:1,expiresAt:Date.now()+600000}],error:null,loggingPermissions:[],stationPermissions:[],loggingController:null}
 window.__TAURI_INTERNALS__={invoke:(async(command:string,input:unknown)=>{
  if(command==='get_remote_station_status')return status
  if(command!=='remote_station_action')throw Error('unexpectedCommand')
  const action=(input as {action:RemoteStationAction}).action;actions.push(action)
  if(action.type==='stationPermission')status={...status,stationPermissions:action.allow?[deviceId]:[]}
  if(action.type==='takeOverLogging')status={...status,stationPermissions:[],loggingPermissions:[],loggingController:null}
  return status
 }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke']}
 render(<RemoteStation/>);fireEvent.click(await screen.findByRole('button',{name:'Allow station controls'}))
 await screen.findByRole('button',{name:'Revoke station controls'})
 expect(screen.getByRole('button',{name:'Allow remote logging'})).toBeTruthy()
 fireEvent.click(screen.getByRole('button',{name:'End remote control and clear permissions'}))
 await screen.findByRole('button',{name:'Allow station controls'})
 expect(actions).toEqual([{type:'stationPermission',deviceId,allow:true},{type:'takeOverLogging'}])
})

it('keeps transmit revocation available during a pending refresh and discards its older grant display', async () => {
 const deviceId = crypto.randomUUID()
 const original:RemoteStationStatus={phase:'connected',origin:'https://remote-staging.hamradiotools.io',stationId:crypto.randomUUID(),accountId:crypto.randomUUID(),pairingId:null,pairingCode:null,expiresAt:null,devices:[{id:deviceId,name:'FT browser',approved:1,expiresAt:Date.now()+600000}],error:null,stationPermissions:[deviceId],transmitPermissions:[deviceId]}
 let status = original, release!:()=>void
 const delayed = new Promise<RemoteStationStatus>(resolve => { release = () => resolve(original) })
 const actions:RemoteStationAction[]=[]
 const invoke = async (command:string,input?:unknown) => {
  if(command==='get_remote_station_status')return status
  if(command!=='remote_station_action')throw Error('unexpectedCommand')
  const action=(input as {action:RemoteStationAction}).action;actions.push(action)
  if(action.type==='refresh')return delayed
  if(action.type==='transmitPermission')status={...status,transmitPermissions:action.allow?[deviceId]:[]}
  return status
 }
 window.__TAURI_INTERNALS__={invoke:invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke']}
 render(<RemoteStation />)
 const revoke=await screen.findByRole('button',{name:'Revoke transmission permission'})
 fireEvent.click(screen.getByRole('button',{name:'Refresh browser requests'}))
 expect((revoke as HTMLButtonElement).disabled).toBe(false)
 fireEvent.click(revoke)
 await screen.findByRole('button',{name:'Allow FT8/FT4 transmission'})
 release()
 await waitFor(()=>expect(actions).toContainEqual({type:'transmitPermission',deviceId,allow:false}))
 await new Promise(resolve=>setTimeout(resolve,0))
 expect(screen.queryByRole('button',{name:'Revoke transmission permission'})).toBeNull()
 expect(screen.getByRole('button',{name:'Allow FT8/FT4 transmission'})).toBeTruthy()
})

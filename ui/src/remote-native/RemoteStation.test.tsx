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
  await waitFor(() => expect((screen.getByRole('button', { name: 'Turn on Remote' }) as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(screen.getByRole('button', { name: 'Turn on Remote' }))
  await screen.findByRole('button', { name: 'Turn off Remote' })
  fireEvent.click(screen.getByRole('button', { name: 'Turn off Remote' }))
  await screen.findByRole('button', { name: 'Turn on Remote' })
  expect(actions).toContainEqual({ type: 'approve', accountId, enrollmentId: pairingId, transmit: false })
  expect(actions).toContainEqual({ type: 'device', deviceId, approve: true, transmit: false })
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

// One approval (operator decisions 2026-09-13 and 2026-09-14): approving the pairing turns Remote on
// and approves the browser that paired; approving any browser grants station controls and logging,
// and FT8/FT4 transmit only when its box is ticked.
it('approves the pairing and each browser in one step, with the transmit tick on the approval', async () => {
  const accountId = crypto.randomUUID(), pairingId = crypto.randomUUID(), first = crypto.randomUUID(), second = crypto.randomUUID()
  let status: RemoteStationStatus = { phase: 'approval', origin: 'https://remote-staging.hamradiotools.io',
    stationId: null, accountId, pairingId, pairingCode: crypto.randomUUID().replace(/-/g,'').slice(0,16),
    expiresAt: Date.now()+600000, devices: [], error: null }
  const actions: RemoteStationAction[] = []
  const invoke = async (command: string, input: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command === 'get_settings') return { launchAtLogin: false, remoteAutostartOfferAnswered: false }
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    actions.push(action)
    if (action.type === 'approve') status = { ...status, stationId: pairingId, pairingCode: null, pairingId: null, phase: 'connecting',
      devices: [first, second].map(id => ({ id, name: 'Test browser', approved: 0, expiresAt: Date.now()+600000 })) }
    if (action.type === 'device') status = { ...status, devices: status.devices.map(d => d.id === action.deviceId ? { ...d, approved: 1 } : d) }
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  fireEvent.click(await screen.findByRole('checkbox', { name: 'Also allow FT8/FT4 transmit' }))
  fireEvent.click(screen.getByRole('button', { name: 'Approve this account pairing' }))
  await screen.findByText(second.slice(-6))
  expect(actions).toEqual([{ type: 'approve', accountId, enrollmentId: pairingId, transmit: true }])
  // The approval turned Remote on, which is when start at sign-in is offered.
  await screen.findByRole('button', { name: 'Start Nexus when I sign in' })
  // One Approve per browser, each with its own transmit tick, off unless ticked.
  const ticks = screen.getAllByRole('checkbox', { name: 'Also allow FT8/FT4 transmit' })
  expect(ticks).toHaveLength(2)
  fireEvent.click(ticks[1])
  fireEvent.click(screen.getAllByRole('button', { name: 'Approve browser' })[1])
  await waitFor(() => expect(screen.getAllByRole('button', { name: 'Approve browser' })).toHaveLength(1))
  fireEvent.click(screen.getByRole('button', { name: 'Approve browser' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Approve browser' })).toBeNull())
  expect(actions.slice(1)).toEqual([
    { type: 'device', deviceId: second, approve: true, transmit: true },
    { type: 'device', deviceId: first, approve: true, transmit: false },
  ])
  expect(screen.getAllByRole('button', { name: 'Revoke browser approval' })).toHaveLength(2)
})

// Remote remembers being on, and approved browsers keep what the operator allowed, transmit included.
// A restart still never arms the transmitter, and the shack has to say both.
it('tells the operator approved browsers keep their access across a restart and transmit stays off until TX On', async () => {
  const deviceId = crypto.randomUUID()
  const status: RemoteStationStatus = { phase: 'connected', origin: 'https://remote-staging.hamradiotools.io',
    stationId: crypto.randomUUID(), accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null,
    devices: [{ id: deviceId, name: 'FT browser', approved: 1, expiresAt: Date.now() + 600000 }], error: null,
    loggingPermissions: [], stationPermissions: [deviceId], transmitPermissions: [] }
  const invoke = async (command: string) => {
    if (command === 'get_remote_station_status') return status
    throw new Error('unexpectedCommand')
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  await screen.findByRole('button', { name: 'Turn off Remote' })
  expect(screen.getByText(/stays on when Nexus restarts/).textContent).toMatch(/FT8\/FT4 transmit included/)
  expect(screen.getByText(/stays on when Nexus restarts/).textContent).toMatch(/always off after a restart until the browser presses TX On/)
  // Beside the transmit permission itself, not only in the general hint.
  expect(screen.getByText(/FT8\/FT4 transmit also needs station controls/).textContent).toMatch(/stays allowed across restarts until you revoke it/)
  expect(screen.getByText(/Approving a browser gives it station controls/).textContent).toMatch(/To limit a browser, revoke them here/)
  expect(screen.queryByText(/turns off whenever Nexus restarts|not kept|grant it again after every restart|resets whenever/)).toBeNull()
})

function offerHarness(options: { failLaunchAtLogin?: boolean } = {}) {
  const settings = { launchAtLogin: false, remoteAutostartOfferAnswered: false }
  let status: RemoteStationStatus = { phase: 'disabled', origin: 'https://remote-staging.hamradiotools.io',
    stationId: crypto.randomUUID(), accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null,
    devices: [], error: null }
  const calls: string[] = []
  const invoke = async (command: string, input?: unknown) => {
    calls.push(command)
    if (command === 'get_remote_station_status') return status
    if (command === 'get_settings') return { ...settings }
    if (command === 'answer_remote_autostart_offer') { settings.remoteAutostartOfferAnswered = true; return {} }
    if (command === 'set_launch_at_login') {
      if (options.failLaunchAtLogin) throw 'launchAtLoginUnsupported'
      settings.launchAtLogin = (input as { on: boolean }).on; return {}
    }
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    if (action.type === 'enable') status = { ...status, phase: 'connecting' }
    if (action.type === 'disable') status = { ...status, phase: 'disabled' }
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  return { settings, calls }
}

it('offers start at sign-in once, only when the operator turns Remote on, and remembers "No thanks"', async () => {
  const { settings, calls } = offerHarness()
  render(<RemoteStation />)
  await screen.findByRole('button', { name: 'Turn on Remote' })
  // Nothing is asked before the operator acts — a launch that turned Remote back on asks nothing.
  expect(screen.queryByRole('button', { name: 'Start Nexus when I sign in' })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Turn on Remote' }))
  await screen.findByRole('button', { name: 'Start Nexus when I sign in' })
  expect(calls).not.toContain('set_launch_at_login')
  fireEvent.click(screen.getByRole('button', { name: 'No thanks' }))
  await waitFor(() => expect(settings.remoteAutostartOfferAnswered).toBe(true))
  expect(screen.queryByRole('button', { name: 'Start Nexus when I sign in' })).toBeNull()
  expect(settings.launchAtLogin).toBe(false)
  expect(calls).not.toContain('set_launch_at_login')
  // Answered once: off and on again does not ask again.
  fireEvent.click(await screen.findByRole('button', { name: 'Turn off Remote' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Turn on Remote' }))
  await screen.findByRole('button', { name: 'Turn off Remote' })
  await waitFor(() => expect(calls.filter(c => c === 'get_settings').length).toBe(2))
  expect(screen.queryByRole('button', { name: 'Start Nexus when I sign in' })).toBeNull()
})

it('switches start at sign-in on only when the operator accepts, and says so if the computer refuses', async () => {
  const accepted = offerHarness()
  render(<RemoteStation />)
  fireEvent.click(await screen.findByRole('button', { name: 'Turn on Remote' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Start Nexus when I sign in' }))
  await waitFor(() => expect(accepted.settings.launchAtLogin).toBe(true))
  expect(accepted.settings.remoteAutostartOfferAnswered).toBe(true)
  expect(screen.queryByRole('alert')).toBeNull()
  cleanup()

  const refused = offerHarness({ failLaunchAtLogin: true })
  render(<RemoteStation />)
  fireEvent.click(await screen.findByRole('button', { name: 'Turn on Remote' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Start Nexus when I sign in' }))
  expect((await screen.findByRole('alert')).textContent).toMatch(/did not let Nexus start at sign-in/)
  expect(refused.settings.launchAtLogin).toBe(false)
  // Still answered: a refusal is not a reason to ask again; the switch in Settings remains.
  expect(refused.settings.remoteAutostartOfferAnswered).toBe(true)
})

// Approving at the radio before agreeing in the browser used to fall through to the generic refusal,
// which points at the network. The service refused for a reason the operator can fix in one click.
it('tells an operator to confirm in the browser when they approve at the shack first', async () => {
  const status: RemoteStationStatus = { phase: 'approval', origin: 'https://remote-staging.hamradiotools.io',
    stationId: null, accountId: crypto.randomUUID(), pairingId: crypto.randomUUID(),
    pairingCode: crypto.randomUUID().replace(/-/g,'').slice(0,16), expiresAt: Date.now()+600000, devices: [],
    error: 'awaitingConfirmation' }
  const invoke = async (command: string) => {
    if (command === 'get_remote_station_status') return status
    throw new Error('unexpectedCommand')
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toMatch(/confirm this station in your browser/i)
})

// The service refuses approval as `trialEnded` or `trialDisabled`, and the shack used to show one
// shared "service access has run out" sentence for both. They need different next steps.
it.each([
  ['trialEnded', /trial has ended/i, /switched off/i],
  ['trialDisabled', /switched off/i, /trial has ended/i],
] as const)('tells the operator at the shack which refusal %s is', async (code, says, doesNotSay) => {
  const status: RemoteStationStatus = { phase: 'approval', origin: 'https://remote-staging.hamradiotools.io',
    stationId: null, accountId: crypto.randomUUID(), pairingId: crypto.randomUUID(),
    pairingCode: crypto.randomUUID().replace(/-/g,'').slice(0,16), expiresAt: Date.now()+600000, devices: [],
    error: code }
  const invoke = async (command: string) => {
    if (command === 'get_remote_station_status') return status
    throw new Error('unexpectedCommand')
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toMatch(says)
  expect(alert.textContent).not.toMatch(doesNotSay)
  // Refused at approval, the station was never attached, so it cannot have "stopped connecting".
  expect(alert.textContent).not.toMatch(/stopped connecting/i)
})

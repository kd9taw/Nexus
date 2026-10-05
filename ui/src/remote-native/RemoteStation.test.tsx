// @vitest-environment jsdom
import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { RemoteStationAction, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

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
 const revoke=await screen.findByRole('button',{name:'Revoke FT8/FT4 transmission from the Remote page'})
 fireEvent.click(screen.getByRole('button',{name:'Refresh browser requests'}))
 expect((revoke as HTMLButtonElement).disabled).toBe(false)
 fireEvent.click(revoke)
 await screen.findByRole('button',{name:'Allow FT8/FT4 transmission from the Remote page'})
 release()
 await waitFor(()=>expect(actions).toContainEqual({type:'transmitPermission',deviceId,allow:false}))
 await new Promise(resolve=>setTimeout(resolve,0))
 expect(screen.queryByRole('button',{name:'Revoke FT8/FT4 transmission from the Remote page'})).toBeNull()
 expect(screen.getByRole('button',{name:'Allow FT8/FT4 transmission from the Remote page'})).toBeTruthy()
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
  fireEvent.click(await screen.findByRole('checkbox', { name: 'Also allow FT8/FT4 transmit from the Remote page' }))
  fireEvent.click(screen.getByRole('button', { name: 'Approve this account pairing' }))
  await screen.findByText(second.slice(-6))
  expect(actions).toEqual([{ type: 'approve', accountId, enrollmentId: pairingId, transmit: true }])
  // The approval turned Remote on, which is when start at sign-in is offered.
  await screen.findByRole('button', { name: 'Start Nexus when I sign in' })
  // One Approve per browser, each with its own transmit tick, off unless ticked.
  const ticks = screen.getAllByRole('checkbox', { name: 'Also allow FT8/FT4 transmit from the Remote page' })
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
it('tells the operator approved browsers keep their access across a restart and transmit stays off until the browser turns it on', async () => {
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
  expect(screen.getByText(/stays on when Nexus restarts/).textContent).toMatch(/always off after a restart until the browser turns it on again/)
  expect(screen.getByText(/stays on when Nexus restarts/).textContent).toMatch(/Turn off Remote only disconnects/)
  // Beside the transmit permission itself, not only in the general hint.
  expect(screen.getByText(/FT8\/FT4 transmit from the Remote page also needs station controls/).textContent).toMatch(/stays allowed across restarts until you revoke it/)
  expect(screen.getByText(/Approving a browser gives it station controls/).textContent).toMatch(/To limit a browser, revoke them here/)
  expect(screen.queryByText(/turns off whenever Nexus restarts|not kept|grant it again after every restart|resets whenever/)).toBeNull()
})

// The operator, 2026-10-03: "an approved, streaming browser is you at the shack and may change everything. I fix only the
// wording in Settings that says otherwise." A stream needs station controls alone, so the card says what that browser can
// do, transmit included, and names the FT8/FT4 permission for what it covers: the Remote page's own controls.
it('says a streaming browser uses Nexus as you would here, transmit included, and names the FT8/FT4 permission for the Remote page', async () => {
  const deviceId = crypto.randomUUID()
  const status: RemoteStationStatus = { phase: 'connected', origin: 'https://remote-staging.hamradiotools.io',
    stationId: crypto.randomUUID(), accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null,
    devices: [{ id: deviceId, name: 'Streaming browser', approved: 1, expiresAt: Date.now() + 30 * 86400000 },
      { id: crypto.randomUUID(), name: 'Waiting browser', approved: 0, expiresAt: Date.now() + 600000 }], error: null,
    loggingPermissions: [], stationPermissions: [deviceId], transmitPermissions: [] }
  const invoke = async (command: string) => {
    if (command === 'get_remote_station_status') return status
    throw new Error('unexpectedCommand')
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  await screen.findByRole('button', { name: 'Turn off Remote' })
  const control = screen.getByText(/Approving a browser gives it station controls/).textContent
  expect(control).toMatch(/With streaming on, they also let it stream this window and use Nexus as you would here, transmit included/)
  expect(control).toMatch(/revoke them here, which also ends its stream/)
  const transmit = screen.getByText(/FT8\/FT4 transmit from the Remote page also needs station controls/).textContent
  expect(transmit).toMatch(/A streaming browser doesn’t need it: through the stream it transmits as you would here/)
  expect(transmit).toMatch(/Nothing transmits from the page until the browser presses TX On/)
  // The old words read as a limit that a stream does not have; no line of the card says them now.
  const card = document.querySelector('.remote-native')!.textContent
  expect(card).not.toMatch(/Starting a transmission needs its own permission/)
  expect(card).not.toMatch(/Nothing transmits until the browser presses TX On/)
  expect(screen.getByRole('checkbox', { name: 'Also allow FT8/FT4 transmit from the Remote page' })).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Allow FT8/FT4 transmission from the Remote page' })).toBeTruthy()
})

// The same words in every language. A translation that kept the old limit would tell its operator the opposite of the
// English, and an English-only maintainer would never see it.
it('in every language the FT8/FT4 permission names the Remote page, and the switch, the card and the question say transmit included', () => {
  type Locale = 'en' | 'de' | 'es' | 'fr' | 'ja'
  const PAGE: Record<Locale, string> = { en: 'Remote page', de: 'Remote-Seite', es: 'página de Remote', fr: 'page Remote', ja: 'Remote のページ' }
  const INCLUDED: Record<Locale, string> = { en: 'transmit included', de: 'Senden eingeschlossen', es: 'incluida la transmisión', fr: 'émission comprise', ja: '送信を含む' }
  const STREAMED: Record<Locale, string> = { en: 'streamed to this browser', de: 'als Stream in diesem Browser', es: 'en directo en este navegador',
    fr: 'en direct dans ce navigateur', ja: 'このブラウザーへのライブ' }
  const OLD_LIMIT: Record<Locale, string> = { en: 'Starting a transmission needs its own permission', de: 'Eine Aussendung zu starten erfordert eine eigene Berechtigung',
    es: 'Iniciar una transmisión requiere un permiso independiente', fr: 'Lancer une émission nécessite une autorisation distincte', ja: '送信を始めるには別の許可が必要です' }
  const catalogs = { en: EN, de: DE, es: ES, fr: FR, ja: JA } as Record<Locale, Record<string, unknown>>
  for (const loc of Object.keys(catalogs) as Locale[]) {
    const say = (key: string) => String(catalogs[loc][key])
    for (const key of ['remote.approveTransmit', 'remote.transmitAllow', 'remote.transmitRevoke', 'remote.transmitLocalHint'])
      expect(say(key), `${loc} ${key}`).toContain(PAGE[loc])
    for (const key of ['settings.remoteStream.hint', 'remote.controlLocalHint', 'remote.approval.body', 'remote.approval.bodyAgain'])
      expect(say(key), `${loc} ${key}`).toContain(INCLUDED[loc])
    for (const key of ['remote.configurationLocal', 'remote.settingsEditable'])
      expect(say(key), `${loc} ${key}`).toContain(STREAMED[loc])
    expect(say('remote.controlLocalHint'), `${loc} still says a transmission needs its own permission`).not.toContain(OLD_LIMIT[loc])
  }
})

// Browser approval lifetime (operator decision 2026-09-14): each approved browser shows when its
// approval ends. In the last seven days before the end that use cannot move, the shack warns and
// offers to approve it again, with the transmit tick off unless ticked.
it('shows each browser approval expiry, and warns and offers approval again in its last seven days', async () => {
  const day = 86400000, now = Date.now(), date = (ms: number) => new Date(ms).toISOString().slice(0, 10)
  const renewing = crypto.randomUUID(), capped = crypto.randomUUID(), older = crypto.randomUUID(), idle = crypto.randomUUID()
  const status: RemoteStationStatus = { phase: 'connected', origin: 'https://remote-staging.hamradiotools.io',
    stationId: crypto.randomUUID(), accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null, error: null,
    devices: [
      { id: renewing, name: 'Laptop', approved: 1, expiresAt: now + 30 * day, generation: 2, renewsUntil: now + 60 * day },
      { id: capped, name: 'Phone', approved: 1, expiresAt: now + 5 * day, generation: 2, renewsUntil: now + 5 * day },
      // Approved by an older Nexus: a fixed expiry, and the service never renews it.
      { id: older, name: 'Tablet', approved: 1, expiresAt: now + 6 * day },
      // Not used lately, but use would still carry it on: no warning.
      { id: idle, name: 'Desk', approved: 1, expiresAt: now + 3 * day, generation: 4, renewsUntil: now + 80 * day },
    ] }
  const actions: RemoteStationAction[] = []
  const invoke = async (command: string, input?: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    actions.push((input as { action: RemoteStationAction }).action)
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  const card = async (id: string) => (await screen.findByText(id.slice(-6))).closest('div')!
  expect((await card(renewing)).textContent).toContain(`Approved until ${date(now + 30 * day)} UTC`)
  expect((await card(renewing)).textContent).toContain(`up to ${date(now + 60 * day)} UTC`)
  expect((await card(older)).textContent).toContain(`Approved until ${date(now + 6 * day)} UTC.`)
  for (const id of [renewing, idle]) {
    expect((await card(id)).textContent).not.toContain('approval ends')
    expect(within(await card(id)).queryByRole('button', { name: 'Approve again' })).toBeNull()
  }
  for (const [id, end] of [[capped, now + 5 * day], [older, now + 6 * day]] as const) {
    expect((await card(id)).textContent).toContain(`This approval ends ${date(end)} UTC`)
    expect(within(await card(id)).getByRole('button', { name: 'Approve again' })).toBeTruthy()
  }
  const phone = await card(capped)
  fireEvent.click(within(phone).getByRole('checkbox', { name: 'Also allow FT8/FT4 transmit from the Remote page' }))
  fireEvent.click(within(phone).getByRole('button', { name: 'Approve again' }))
  await waitFor(() => expect(actions).toContainEqual({ type: 'device', deviceId: capped, approve: true, transmit: true }))
  // Ending it is still one click away, beside the warning.
  expect(within(phone).getByRole('button', { name: 'Revoke browser approval' })).toBeTruthy()
})

// A5: the operator compares the browser's key with the one the browser shows, and approving pins the
// key the station showed. The fingerprints here are random hex of the right shape, not keys.
it('shows each browser key beside its name, approves with the key shown, and asks again for a key not pinned here', async () => {
  const day = 86400000, now = Date.now()
  const fingerprint = () => Array.from(crypto.getRandomValues(new Uint8Array(32)), b => b.toString(16).padStart(2, '0')).join('')
  // As both ends show a key: the first 128 bits of its fingerprint, eight groups of four (S3-L1).
  const short = (hex: string) => hex.slice(0, 32).toUpperCase().match(/.{4}/g)!.join(' ')
  const waiting = crypto.randomUUID(), unpinned = crypto.randomUUID(), pinned = crypto.randomUUID(), keyless = crypto.randomUUID()
  const keys = { [waiting]: fingerprint(), [unpinned]: fingerprint(), [pinned]: fingerprint() }
  const status: RemoteStationStatus = { phase: 'connected', origin: 'https://remote-staging.hamradiotools.io',
    stationId: crypto.randomUUID(), accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null, error: null,
    devices: [
      { id: waiting, name: 'New browser', approved: 0, expiresAt: now + 600000, key: keys[waiting] },
      // Approved before keys, or back with a new key: the service lists a key this station has not pinned.
      { id: unpinned, name: 'Laptop', approved: 1, expiresAt: now + 30 * day, generation: 2, renewsUntil: now + 80 * day, key: keys[unpinned] },
      { id: pinned, name: 'Phone', approved: 1, expiresAt: now + 30 * day, generation: 2, renewsUntil: now + 80 * day, key: keys[pinned] },
      // No key listed yet: nothing to pin, so nothing to ask.
      { id: keyless, name: 'Tablet', approved: 1, expiresAt: now + 30 * day, generation: 2, renewsUntil: now + 80 * day },
    ], pinnedDevices: [pinned] }
  const actions: RemoteStationAction[] = []
  const invoke = async (command: string, input?: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    actions.push((input as { action: RemoteStationAction }).action)
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  const card = async (id: string) => (await screen.findByText(id.slice(-6))).closest('div')!
  for (const id of [waiting, unpinned, pinned]) expect((await card(id)).textContent).toContain(`Key ${short(keys[id])}`)
  expect((await card(keyless)).textContent).not.toContain('Key ')
  const notice = 'can’t stream until you approve it again here'
  expect((await card(unpinned)).textContent).toContain(notice)
  for (const id of [pinned, keyless]) {
    expect((await card(id)).textContent).not.toContain(notice)
    expect(within(await card(id)).queryByRole('button', { name: 'Approve again' })).toBeNull()
  }
  fireEvent.click(within(await card(waiting)).getByRole('button', { name: 'Approve browser' }))
  await waitFor(() => expect(actions).toContainEqual({ type: 'device', deviceId: waiting, approve: true, transmit: false, key: keys[waiting] }))
  fireEvent.click(within(await card(unpinned)).getByRole('button', { name: 'Approve again' }))
  await waitFor(() => expect(actions).toContainEqual({ type: 'device', deviceId: unpinned, approve: true, transmit: false, key: keys[unpinned] }))
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

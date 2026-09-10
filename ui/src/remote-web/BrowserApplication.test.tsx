// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import App from '../App'
import configurationSettings from './__fixtures__/configuration-settings.json'
import configurationProgramming from './__fixtures__/configuration-programming.json'
import navigationConnect from './__fixtures__/navigation-connect.json'
import navigationPath from './__fixtures__/navigation-path.json'
import navigationSatellites from './__fixtures__/navigation-satellites.json'
import navigationSatellite from './__fixtures__/navigation-satellite.json'
import navigationLive from './__fixtures__/navigation-satellite-live.json'
import { navigationPages } from './__fixtures__/navigation-page'
import { parseSatelliteLive } from './navigation'
import { installApplicationTransport } from '../applicationTransport'
import type { AppSnapshot, ChatMessage, Settings, Station, Tier } from '../types'
import settingsFixture from '../components/__fixtures__/defaultSettings.json'
import { StationControlContext, StationDataContext } from '../stationAccess'
import { Dialog } from '../components/ui/Dialog'
import { cwDecode, getScopeRow } from '../api'
import { RttyCockpit } from '../components/RttyCockpit'
import { PskCockpit } from '../components/PskCockpit'
import { coerceMemory, memoriesStore } from '../features/memories'
import sstvFixture from './__fixtures__/sstv.json'
import aprsFixture from './__fixtures__/aprs.json'
import aprsRosterFixture from './__fixtures__/aprs-roster.json'
import { parseSstvSample } from './sstv'
import { parseAprsLive } from './aprs'
import js8Fixture from './__fixtures__/js8.json'
import { parseJs8Sample } from './js8'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'

const snapshot = {
  mycall: 'N0CALL', mygrid: 'AA00', mode: 'Normal',
  radio: { dialMhz: 3.573, band: '80m', catOk: true, sideband: 'USB', operatingMode: 'digital',
    transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500,
    txLevel: 0.5, slot: 0, nextSlotMs: 12000 },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
} as unknown as AppSnapshot
let dispose: (() => void) | undefined
function projectedSettings(): Settings {
  const rust = readFileSync(resolve('../src-tauri/src/remote_service/application.rs'), 'utf8')
  const list = rust.match(/const SETTINGS_KEYS: &\[&str\] = &\[([\s\S]*?)\];/)![1]
  const keys = [...list.matchAll(/"([a-zA-Z0-9]+)"/g)].map(match => match[1])
  expect(keys).toContain('units')
  return Object.fromEntries(Object.entries(settingsFixture).filter(([key]) => keys.includes(key))) as unknown as Settings
}
beforeEach(() => {
  localStorage.clear()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  window.matchMedia = ((media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {},
    addListener() {}, removeListener() {} })) as unknown as typeof window.matchMedia
})
afterEach(() => { cleanup(); dispose?.(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

function keyboardSample(mode: string) {
  const text = 'CQ W1AW'
  return { armed: true, text, charConf: [...text].map((_, i) => i ? 100 : 30), afcHz: -12.5,
    sending: true, latched: true, keyerError: null,
    ...(mode === 'rtty' ? { afcLocked: true, markHz: 915, spaceHz: 1085, baud: 45.45, shiftHz: 170,
      backend: 'afsk', auto: true, seqState: 'idle', peer: null, peerExchange: [], heardCq: 'W1AW' }
      : { signal: true, centerHz: 1000, mode: 'qpsk31', reverse: true }) }
}

it('connects exactly one real JS8 cockpit without entry, inbox, send, shortcut or log mutations', async () => {
  const current=structuredClone(snapshot), settings=projectedSettings(), calls:string[]=[]
  const state=structuredClone(js8Fixture.state)
  let stateAvailable=true, js8Reads=0
  dispose=installApplicationTransport({kind:'remote',invoke:async <T,>(command:string):Promise<T>=>{
    calls.push(command)
    if(command==='get_snapshot')return structuredClone(current) as T
    if(command==='get_settings')return settings as T
    if(command==='get_band_plan')return [] as T
    if(command==='get_js8_state'&&stateAvailable){
      js8Reads++
      return parseJs8Sample({...js8Fixture,state,capturedAtMs:js8Fixture.capturedAtMs+js8Reads*500},50+(js8Reads%3)*10) as T
    }
    if(command==='get_spectrum_row')return {row:[],loHz:0,hiHz:4000,source:'audio'} as T
    if(command==='get_meters')return {rxLevel:0,smeterDb:null,cwToneHz:null} as T
    throw new Error('applicationUnsupported')
  }})
  const context=new RemoteCollections({supports:()=>false} as unknown as ApplicationClient)
  vi.spyOn(context,'page').mockImplementation(async()=>({collection:'js8Context',offset:0,total:0,retained:0,rows:[],nextCursor:null,ageMs:0,
    meta:{capturedAgeMs:0,source:{plan:[],history:{W1AW:{count:2,lastUnix:1700000000,grid:'FN31',name:'PRIOR CONTACT',comment:'COMPLETE LOG'},
      K2ABC:{count:0,lastUnix:null,grid:'',name:'',comment:''}}}}} as unknown as QueryPage))
  const view=(available:boolean)=><StationControlContext.Provider value={false}><StationDataContext.Provider value={available}>
    <RemoteCollectionsContext.Provider value={context}><App remote={{snapshot:current,settings,bandPlan:[],js8:true,status:<div>Observer</div>}}/></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const {container,rerender,unmount}=render(view(true))
  fireEvent.click(screen.getByRole('button',{name:/^JS8 —/}))
  await waitFor(()=>expect(container.querySelector('.js8-stations')?.textContent).toContain('COMPLETE LOG'))
  const root=container.querySelector('.js8-cockpit')!
  expect(container.querySelectorAll('.js8-cockpit')).toHaveLength(1)
  expect(root.querySelectorAll('.waterfall-wrap')).toHaveLength(1)
  expect(container.querySelector('.grid-center .conversation')).toBeNull()
  expect(container.querySelector('.grid-stations')).toBeNull()
  expect(root.textContent).toContain('STORED REMOTE TEST')
  expect(root.textContent).toContain('TEST QUEUED FRAMES')
  const activityRow=root.querySelector('.js8-row'), observedReads=js8Reads
  await waitFor(()=>expect(js8Reads).toBeGreaterThan(observedReads+1), {timeout:2000})
  expect(root.querySelector('.js8-row')).toBe(activityRow)
  const controls=root.querySelectorAll<HTMLButtonElement>('.js8-speed-chip,.js8-query,.js8-inbox-act,.js8-send,.js8-cq,.js8-hb,.js8-arm,.js8-cancel,.js8-drop')
  expect(controls.length).toBeGreaterThan(15)
  for(const button of controls){expect(button.disabled).toBe(true);fireEvent.click(button)}
  fireEvent.doubleClick(root.querySelector('.js8-offset-row')!)
  fireEvent.change(root.querySelector('.js8-compose')!,{target:{value:'TEST SEND'}})
  fireEvent.keyDown(root.querySelector('.js8-compose')!,{key:'Enter'})
  fireEvent.keyDown(window,{key:'Escape'})
  fireEvent.click(root.querySelector('.js8-station-call')!)
  expect((root.querySelector('.js8-to') as HTMLInputElement).value).toBe('W1AW')
  fireEvent.click(root.querySelector('.js8-pin')!)
  expect(root.querySelector('.js8-pin')?.getAttribute('aria-pressed')).toBe('true')
  stateAvailable=false
  await waitFor(()=>expect(root.querySelector('.js8-inbox')?.textContent).not.toContain('STORED REMOTE TEST'))
  expect(root.querySelector('.js8-pending-row')).toBeNull()
  stateAvailable=true
  await waitFor(()=>expect(root.querySelector('.js8-inbox')?.textContent).toContain('STORED REMOTE TEST'))
  rerender(view(false))
  await waitFor(()=>expect(root.querySelector('.js8-stations')?.textContent).not.toContain('COMPLETE LOG'))
  unmount()
  const reads=['get_snapshot','get_settings','get_band_plan','get_js8_state','get_spectrum_row','get_meters',
    'app_version','dxcc_entity_locations','get_declination','get_awards','get_journey','radio_launch_info',
    'check_for_update','get_propagation','sat_track_status','get_tle_status','get_kp_forecast','get_xray_now',
    'get_dxped_windows','get_feed_health','get_need_alerts','get_all_spots','get_fd_ruleset','log_operators','read_rotator','get_sat_transponder']
  expect(calls.filter(c=>!reads.includes(c))).toEqual([])
  for(const forbidden of ['get_log','get_licensed_band_plan','qrz_lookup','resolve_entity','js8_enter','js8_send','js8_inbox_mark']) expect(calls).not.toContain(forbidden)
})

it.each(['TempoFast', 'TempoDeep'])('browses the actual %s conversations without sending, archiving or changing the station', async tier => {
  const current = structuredClone(snapshot), settings = projectedSettings(), calls: string[] = []
  current.link.tier = tier as Tier
  current.activePeer = 'W1AW'
  const favorite = coerceMemory({ id:'observer-favorite',name:'Test memory',rxMhz:14.074,mode:'FT8',favorite:true })!
  expect(favorite.favorite).toBe(true)
  vi.spyOn(memoriesStore,'get').mockReturnValue({...memoriesStore.get(),memories:[favorite]})
  const message = (text: string, extra: Partial<ChatMessage> = {}): ChatMessage => ({
    from: 'N0CALL', to: 'W1AW', text, slot: 10, directedToMe: false, outbound: true,
    snr: -8, freqHz: 1500, dtSec: 0.1, tier: tier as Tier, ...extra,
  })
  current.conversations = [{ peer: 'W1AW', messages: [
    message('HELD', { stored: true }), message('SENDING', { attempts: 2 }),
    message('CONFIRMED', { confirmed: true }), message('DELIVERED', { delivered: true }),
    message('NO ACK', { noAck: true }), message('ABANDONED', { abandoned: true }),
    message('PARTIAL', { from: 'W1AW', outbound: false, incomplete: [2, 3] }),
    message('LEGACY', { from: 'W1AW', outbound: false, tier: null }),
  ] }, { peer: '*', messages: [message('BAND MESSAGE', { outbound: false, to: null })] }]
  current.stations = ['TempoFast', 'TempoDeep', 'FT8'].map((mode, i) => ({
    call: ['W1AW', 'K2ABC', 'K3FT'][i], grid: 'FN31', tier: mode, snr: -8,
    lastHeardSlot: 0, heardCount: 1, presence: 'active', worked: false,
  })) as Station[]
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === 'get_snapshot') return structuredClone(current) as T
    if (command === 'get_settings') return structuredClone(settings) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_spectrum_row' || command === 'get_scope_snapshot') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0.2, smeterDb: -12, cwToneHz: null } as T
    throw new Error('applicationUnsupported')
  } })
  const { container, unmount } = render(<StationControlContext.Provider value={false}>
    <App remote={{ snapshot: current, settings, bandPlan: [], status: <div>Observer</div> }} />
  </StationControlContext.Provider>)
  await waitFor(() => expect(container.querySelector('.bubble-text')?.textContent).toBe('HELD'))
  for (const stage of ['held', 'sending', 'confirmed', 'delivered', 'no-ack', 'abandoned']) {
    expect(container.querySelector(`.delivery.${stage}`), stage).not.toBeNull()
  }
  expect(container.querySelector('.bubble-incomplete')?.textContent).toContain('2 of 3')
  expect(screen.getByText('LEGACY', { selector: '.bubble-text' })).toBeTruthy()
  expect(container.querySelector('.bubble.resendable')).toBeNull()
  expect([...container.querySelectorAll('.station-call')].map(e => e.textContent)).toEqual(['W1AW', 'K2ABC'])
  const actions = container.querySelectorAll<HTMLButtonElement>('.conversation button,.cq-run button,.recent-archive,.station-work,.cockpit-mode')
  expect(actions.length).toBeGreaterThan(10)
  for (const button of actions) { expect(button.disabled, button.className).toBe(true);fireEvent.click(button) }
  const input = container.querySelector<HTMLInputElement>('.composer-input')!
  expect(input.disabled).toBe(true)
  fireEvent.change(input,{target:{value:'TEST'}})
  fireEvent.submit(input.closest('form')!)
  for (const bubble of container.querySelectorAll('.bubble')) { fireEvent.click(bubble);fireEvent.keyDown(bubble,{key:'Enter'}) }
  for (const card of container.querySelectorAll('.station-card')) fireEvent.doubleClick(card)
  fireEvent.click(container.querySelectorAll('.station-open')[1])
  await waitFor(() => expect(container.querySelector('.conv-peer')?.textContent).toBe('K2ABC'))
  fireEvent.click(container.querySelector('.band-row')!)
  await screen.findByText('BAND MESSAGE', { selector: '.bubble-text' })
  fireEvent.click((await screen.findByText('FT',{selector:'.mode-label'})).closest('button')!)
  fireEvent.click((await screen.findByText('Tempo',{selector:'.mode-label'})).closest('button')!)
  await screen.findByText('BAND MESSAGE', { selector: '.bubble-text' })
  fireEvent.keyDown(window,{key:'1',code:'Digit1',ctrlKey:true})
  await act(async () => { await new Promise(resolve => setTimeout(resolve,20)) })
  unmount()
  const reads = ['get_snapshot','get_settings','get_band_plan','get_spectrum_row','get_scope_snapshot','get_meters',
    'app_version','dxcc_entity_locations','get_declination','get_awards','get_journey','radio_launch_info',
    'check_for_update','get_propagation','sat_track_status','get_tle_status','get_kp_forecast','get_xray_now',
    'get_dxped_windows','get_feed_health','get_need_alerts','get_all_spots','get_fd_ruleset','log_operators',
    'read_rotator','get_sat_transponder']
  expect(calls.filter(command => !reads.includes(command))).toEqual([])
})

it.each(['native', 'older observer', 'Field Day observer'])('%s navigation preserves the station mode boundary', async kind => {
  localStorage.setItem('nexus.features.wizardSeen', '1')
  const current = structuredClone(snapshot)
  const settings = { ...settingsFixture, fdActive: true } as unknown as Settings
  const calls: { command: string; args: unknown }[] = []
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args })
    if (command === 'get_snapshot' || command === 'set_mode' || command === 'set_area') return structuredClone(current) as T
    if (command === 'get_settings') return structuredClone(settings) as T
    if (command === 'get_band_plan' || command === 'log_operators') return [] as T
    throw new Error('applicationUnsupported')
  } })
  const native = kind === 'native'
  const { container, unmount } = render(<StationControlContext.Provider value={native}>
    <App remote={native ? undefined : { snapshot: current, settings, bandPlan: [],
      fieldDay: kind === 'Field Day observer', status: <div>Observer</div> }} />
  </StationControlContext.Provider>)
  fireEvent.click((await screen.findByText('Field Day', { selector: '.mode-label' })).closest('button')!)
  await waitFor(() => expect(container.querySelector(native ? '.panel.fieldday' : kind === 'older observer'
    ? '.remote-view-unavailable' : '.remote-field-day-view')).not.toBeNull())
  const board = (await screen.findByText('Club Board', { selector: '.mode-label' })).closest('button')!
  expect(board.disabled).toBe(!native)
  if (!native) fireEvent.click(board)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)) })
  const modeCalls = calls.filter(({ command }) => command === 'set_mode')
  expect(modeCalls).toEqual(native ? [{ command: 'set_mode', args: { mode: 'fieldday-sp' } }] : [])
  unmount()
})

it.each(['rtty', 'psk'])('observes the actual %s cockpit without decoder, keyboard, radio or log mutations', async mode => {
  const current = structuredClone(snapshot)
  current.radio.operatingMode = mode === 'rtty' ? 'rtty' : 'keyboard'
  current.radio.txEnabled = true
  const settings = projectedSettings(), calls: string[] = []
  let available = true
  const data = keyboardSample(mode)
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === 'get_snapshot') return structuredClone(current) as T
    if (command === 'get_settings') return structuredClone(settings) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_spectrum_row' || command === 'get_scope_snapshot') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0.2, smeterDb: -12, cwToneHz: null } as T
    if (command === `get_${mode}_state` && available) return structuredClone(data) as T
    throw new Error('applicationUnsupported')
  } })
  const workspace = (fresh = true) => <StationControlContext.Provider value={false}>
    <StationDataContext.Provider value={fresh}>
      <App remote={{ snapshot: current, settings, bandPlan: [], cwPhone: true, keyboard: true,
        stale: !fresh, status: <div>Observer</div> }} />
    </StationDataContext.Provider>
  </StationControlContext.Provider>
  const { container, rerender, unmount } = render(workspace())
  await waitFor(() => expect(container.querySelector(`.${mode}-cockpit .cw-decode-text`)?.textContent).toBe(data.text))
  const root = container.querySelector(`.${mode}-cockpit`)!
  expect(root.querySelector('.pane-frame[data-pane="log"]')).not.toBeNull()
  expect(root.textContent).toContain('Remote QSO entry is not connected yet.')
  expect(root.querySelector('.cw-decode-text span')?.getAttribute('style')).toContain('opacity:')
  expect(root.querySelector('.cockpit-pwr-val')?.textContent).toBe('—')
  const controls = root.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement>(
    '.rtty-arm, .cw-decode-clear, .cw-macro, .cw-type, .cw-send-btn, .rtty-hiscall, .psk-mode-select')
  expect(controls.length).toBeGreaterThan(10)
  for (const control of controls) {
    expect(control.disabled, control.textContent ?? control.className).toBe(true)
    fireEvent.click(control)
  }
  const compose = root.querySelector<HTMLInputElement>('.cw-type')!
  fireEvent.change(compose, { target: { value: 'TEST' } })
  fireEvent.keyDown(compose, { key: 'Enter' })
  compose.dispatchEvent(new InputEvent('beforeinput', { data: 'X', inputType: 'insertText', bubbles: true, cancelable: true }))
  for (const key of ['F1', 'F2', 'F3', 'F4', 'Escape', ' ']) {
    fireEvent.keyDown(window, { key }); fireEvent.keyUp(window, { key })
  }
  current.radio.rfPower = 0.42
  await waitFor(() => expect(root.querySelector('.cockpit-pwr-val')?.textContent).toBe('42%'))
  available = false
  await waitFor(() => expect(root.textContent).toContain('Station decoder data unavailable.'))
  expect(root.querySelector('.cw-decode-text')?.textContent).not.toContain(data.text)
  available = true
  data.armed = false; data.text = ''
  await waitFor(() => expect(root.textContent).toContain('Start the decoder in Nexus at the shack to receive text.'))
  data.armed = true; data.text = 'RETURNED'
  await waitFor(() => expect(root.querySelector('.cw-decode-text')?.textContent).toBe('RETURNED'))
  rerender(workspace(false))
  expect(root.querySelector('.cw-decode-text')?.textContent).not.toContain('RETURNED')
  expect(root.querySelector('.cockpit-pwr-val')?.textContent).toBe('—')
  unmount()
  await new Promise(resolve => setTimeout(resolve, 50))
  const reads = ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_scope_snapshot',
    'get_meters', 'get_cw_state', 'get_rtty_state', 'get_psk_state',
    // The real App also attempts these startup reads. This fake records calls
    // before ApplicationClient's allowlist refuses the unsupported reads.
    'app_version', 'dxcc_entity_locations', 'get_declination', 'get_awards', 'get_journey',
    'radio_launch_info', 'check_for_update', 'get_propagation', 'sat_track_status', 'get_tle_status',
    'get_kp_forecast', 'get_xray_now', 'get_dxped_windows', 'get_feed_health', 'get_need_alerts',
    'get_all_spots', 'get_fd_ruleset', 'log_operators']
  expect(calls.filter(command => !reads.includes(command))).toEqual([])
})

it.each(['rtty', 'psk'])('discards late %s reads after station loss or navigation and reads again on return', async mode => {
  const pending: ((value: unknown) => void)[] = [], calls: string[] = []
  dispose = installApplicationTransport({ kind: 'remote', invoke: <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === `get_${mode}_state`) return new Promise<T>(resolve => pending.push(value => resolve(value as T)))
    return Promise.reject(new Error('applicationUnsupported'))
  } })
  const Cockpit = mode === 'rtty' ? RttyCockpit : PskCockpit
  const workspace = (active: boolean, fresh: boolean) => <StationControlContext.Provider value={false}>
    <StationDataContext.Provider value={fresh}><Cockpit snap={snapshot} active={active} /></StationDataContext.Provider>
  </StationControlContext.Provider>
  const { container, rerender, unmount } = render(workspace(true, true))
  const transcript = () => container.querySelector('.cw-decode-text')?.textContent
  for (const [active, fresh] of [[true, false], [false, true]]) {
    await waitFor(() => expect(pending.length).toBeGreaterThan(0))
    rerender(workspace(active, fresh))
    await act(async () => { for (const resolve of pending.splice(0)) resolve(keyboardSample(mode)) })
    expect(transcript()).not.toContain('CQ W1AW')
    rerender(workspace(true, true))
    expect(transcript()).not.toContain('CQ W1AW')
  }
  await waitFor(() => expect(pending.length).toBeGreaterThan(0))
  await act(async () => { for (const resolve of pending.splice(0)) resolve({ ...keyboardSample(mode), text: 'FRESH' }) })
  expect(transcript()).toBe('FRESH')
  unmount()
  expect(calls).not.toContain(`${mode}_auto_arm`)
  expect(calls.filter(command => !['get_meters', 'get_spectrum_row', `get_${mode}_state`].includes(command))).toEqual([])
})

it.each(['rtty', 'psk'])('retains native %s decoder entry and keyboard stop behavior', async mode => {
  const current = structuredClone(snapshot)
  current.radio.operatingMode = mode === 'rtty' ? 'rtty' : 'keyboard'
  current.radio.txEnabled = true
  const data = keyboardSample(mode), calls: string[] = []
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === 'get_settings') return structuredClone(settingsFixture) as T
    if (command === 'get_licensed_band_plan' || command === 'get_log' || command === 'log_operators') return [] as T
    if (command.startsWith(`${mode}_`) || command === `get_${mode}_state`) return structuredClone(data) as T
    if (command === 'halt_tx') return current as T
    throw new Error('applicationUnsupported')
  } })
  const Cockpit = mode === 'rtty' ? RttyCockpit : PskCockpit
  const { container, unmount } = render(<Cockpit snap={current} />)
  await waitFor(() => expect(calls).toContain(`${mode}_auto_arm`))
  expect(container.querySelector('.remote-observer-dock')).toBeNull()
  await waitFor(() => expect(container.querySelector('.cw-decode-text')?.textContent).toBe(data.text))
  const compose = container.querySelector<HTMLInputElement>('.cw-type')!
  expect(compose.disabled).toBe(false)
  compose.dispatchEvent(new InputEvent('beforeinput', { data: 'X', inputType: 'insertText', bubbles: true, cancelable: true }))
  await waitFor(() => expect(calls).toContain(`${mode}_type`))
  fireEvent.keyDown(window, { key: 'Escape' })
  expect(calls).toContain(`${mode}_stop`)
  expect(calls).toContain('halt_tx')
  unmount()
})

it.each(['cw', 'phone'])('opens the actual %s cockpit as an observer without keyboard or unmount commands', async mode => {
  const current = structuredClone(snapshot)
  current.radio.operatingMode = mode
  current.radio.txEnabled = true
  current.radio.txAllowed = true
  current.radio.cwWpm = 23
  const settings = projectedSettings()
  const calls: { command: string; args: unknown }[] = []
  let cwAvailable = true
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args })
    if (command === 'get_snapshot') return structuredClone(current) as T
    if (command === 'get_settings') return structuredClone(settings) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_spectrum_row' || command === 'get_scope_snapshot') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0.2, smeterDb: -12, cwToneHz: 600 } as T
    if (command === 'get_cw_state' && cwAvailable) return { text: 'CQ TEST', wpm: 22, sent: ['DE TEST'], keyerError: null,
      candidates: [{ call: 'W1AW', best: true }], state: 'cq', headline: '', prompt: '', recommended: null,
      workedCall: null, rst: null, name: null } as T
    throw new Error('applicationUnsupported')
  } })
  const { container, unmount } = render(<StationControlContext.Provider value={false}>
    <App remote={{ snapshot: current, settings, bandPlan: [], cwPhone: true, status: <div>Observer</div> }} />
  </StationControlContext.Provider>)
  await waitFor(() => expect(container.querySelector(`.${mode}-cockpit`)).not.toBeNull())
  expect(container.textContent).toContain('Remote QSO entry is not connected yet.')
  expect(container.textContent).toContain('Update the station app to view cockpit contact history.')
  const root = container.querySelector(`.${mode}-cockpit`)!
  if (mode === 'phone') {
    expect(root.querySelector('.cockpit-pwr-val')?.textContent).toBe('—')
    current.radio.rfPower = 0.42
    await waitFor(() => expect(root.querySelector('.cockpit-pwr-val')?.textContent).toBe('42%'))
  }
  const commands = mode === 'cw' ? '.cw-macro, .cw-send-btn, .cw-keyer-select, .cw-pitch, .cw-decode-clear' : '.ph-ptt, .ph-mode-btn'
  const controls = root.querySelectorAll<HTMLButtonElement>(commands)
  expect(controls.length).toBeGreaterThan(2)
  for (const element of controls) { expect(element.disabled).toBe(true); fireEvent.click(element); fireEvent.pointerDown(element); fireEvent.pointerUp(element) }
  for (const key of ['F1', 'F2', 'Escape', 'PageUp', 'PageDown', ' ']) {
    fireEvent.keyDown(window, { key, code: key === ' ' ? 'Space' : key })
    fireEvent.keyUp(window, { key, code: key === ' ' ? 'Space' : key })
  }
  await getScopeRow(false, 300, 1100, 'sharp')
  expect(calls.find(c => c.command === 'get_scope_snapshot')?.args).toBeUndefined()
  await cwDecode(0.9)
  expect(calls.find(c => c.command === 'get_cw_state')?.args).toBeUndefined()
  if (mode === 'cw') {
    await waitFor(() => expect(root.textContent).toContain('W1AW'))
    expect(root.querySelector('[role="log"]')?.textContent).toBe('CQ TEST')
    cwAvailable = false
    await waitFor(() => expect(root.textContent).toContain('CW decoder data unavailable.'))
    expect(root.querySelector('[role="log"]')?.textContent).toBe('')
    expect(root.textContent).not.toContain('W1AW')
  }
  unmount()
  await new Promise(resolve => setTimeout(resolve, 50))
  // log_operators is a read of the existing logbook's operator list.
  const mutations = calls.filter(c => c.command !== 'log_operators' && /^(set_|send_|stop_|halt_|start_|log_|cw_clear|amp_command|pick_band|select_peer|play_|cancel_|open_panel)/.test(c.command))
  expect(mutations).toEqual([])
  for (const command of ['get_voice_messages', 'get_log', 'read_rotator', 'preview_cw', 'get_licensed_band_plan']) {
    expect(calls.map(c => c.command)).not.toContain(command)
  }
})

it('mounts the real Nexus workspace through the real API and never asserts a restored browser mode', async () => {
  // The projection is the actual station-side whitelist, not a full Settings
  // fixture that would hide a missing browser startup field.
  const settings = projectedSettings()
  const calls: string[] = []
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === 'get_snapshot') return structuredClone(snapshot) as T
    if (command === 'get_settings') return structuredClone(settings) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_spectrum_row') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0, smeterDb: null, cwToneHz: null } as T
    throw new Error('applicationUnsupported')
  } })
  localStorage.setItem('nexus.workspace', 'msg')
  localStorage.setItem('nexus.view', 'phone')
  const workspace = (stale = false) => <StationControlContext.Provider value={false}>
    <App remote={{ snapshot, settings, bandPlan: [], stale, status: <div className="remote-application-status">Observer</div> }} />
  </StationControlContext.Provider>
  const { container, rerender } = render(workspace())
  await waitFor(() => expect(container.querySelector('.operate-host:not([hidden])')).not.toBeNull())
  expect(container.textContent).toContain('N0CALL')
  expect(container.querySelectorAll('.app')).toHaveLength(1)
  expect(container.querySelector('.remote-workspace .remote-cockpit-lower')).not.toBeNull()
  expect(container.querySelector('.grid-center')).toBeNull()
  expect(container.querySelector('.nb-need')?.textContent).toContain('—')
  expect(calls).toContain('get_snapshot')
  for (const command of ['set_area', 'set_operating_mode', 'set_tx_enabled', 'amp_command', 'open_panel_window']) {
    expect(calls, command).not.toContain(command)
  }
  const cockpit = container.querySelector('.operate-host')
  for (const button of container.querySelectorAll<HTMLButtonElement>('.cockpit-qso button, .tuning-nudge, .cockpit-mode, .cs-opt, .tier-btn')) {
    expect(button.disabled, button.textContent ?? '').toBe(true)
  }
  const settingsButton = [...container.querySelectorAll('button')].find(button => button.textContent?.trim() === 'Settings')!
  fireEvent.click(settingsButton)
  await waitFor(() => expect(container.querySelector('.remote-view-unavailable')).not.toBeNull())
  expect(container.querySelector('input[type="password"]')).toBeNull()
  expect(calls).not.toContain('get_credentials_status')
  const digitalButton = [...container.querySelectorAll('button')].find(button => button.textContent?.trim() === 'FT')!
  fireEvent.click(digitalButton)
  await waitFor(() => expect(container.querySelector('.operate-host:not([hidden])')).not.toBeNull())
  expect(calls).not.toContain('set_operating_mode')
  expect(calls).not.toContain('set_area')
  rerender(workspace(true))
  expect(container.querySelector('.app')?.getAttribute('data-remote-stale')).toBe('true')
  expect(container.querySelector('.operate-host')).toBe(cockpit)
})

it('closes a portaled station dialog when station data becomes unavailable', () => {
  const content = (fresh: boolean) => <StationDataContext.Provider value={fresh}>
    <Dialog open onOpenChange={() => {}} title="Synthetic station detail"><p>N0CALL</p></Dialog>
  </StationDataContext.Provider>
  const { rerender } = render(content(true))
  expect(screen.getByRole('dialog')).toBeTruthy()
  rerender(content(false))
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(document.body.style.pointerEvents).not.toBe('none')
})

it('connects the real SSTV cockpit, clears failed samples, and keeps composition local',async()=>{
  const current=structuredClone(snapshot),settings=projectedSettings(),calls:string[]=[]
  let available=true
  dispose=installApplicationTransport({kind:'remote',invoke:async <T,>(command:string):Promise<T>=>{
    calls.push(command)
    if(command==='get_snapshot')return current as T
    if(command==='get_settings')return settings as T
    if(command==='get_band_plan')return [] as T
    if(command==='get_sstv_state'&&available)return parseSstvSample(structuredClone(sstvFixture),0) as T
    if(command==='get_spectrum_row')return {row:[],loHz:0,hiHz:4000,source:'audio'} as T
    if(command==='get_meters')return {rxLevel:0,smeterDb:null,cwToneHz:null} as T
    throw new Error('applicationUnsupported')
  }})
  const context=new RemoteCollections({supports:()=>false} as unknown as ApplicationClient)
  vi.spyOn(context,'page').mockRejectedValue(new Error('applicationUnavailable'))
  const view=(live:boolean)=><StationControlContext.Provider value={false}><StationDataContext.Provider value={live}>
    <RemoteCollectionsContext.Provider value={context}><App remote={{snapshot:current,settings,bandPlan:[],stationModes:true,status:<div>Observer</div>}}/></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const {container,rerender,unmount}=render(view(true))
  fireEvent.click(screen.getByRole('button',{name:/^SSTV —/}))
  await waitFor(()=>expect(container.querySelector('.sstv-thumb-call')?.textContent).toBe('W1AW'))
  expect(container.querySelectorAll('.sstv-view')).toHaveLength(1)
  expect(container.querySelector('.sstv-live-canvas')).not.toBeNull()
  const controls=container.querySelectorAll<HTMLButtonElement>('.sstv-arm,.sstv-tx-send,.sstv-tx-stop,.sstv-thumb-del')
  expect(controls.length).toBe(4)
  for(const button of controls){expect(button.disabled).toBe(true);fireEvent.click(button)}
  const mode=container.querySelector('.sstv-tx-mode select') as HTMLSelectElement
  const alternate=[...mode.options].find(o=>o.value!==mode.value)!.value
  fireEvent.change(mode,{target:{value:alternate}});expect(mode.value).toBe(alternate)
  available=false
  await waitFor(()=>expect(container.querySelector('.sstv-live-canvas')).toBeNull(),{timeout:2000})
  expect(container.querySelector('.sstv-thumb')).toBeNull()
  available=true
  await waitFor(()=>expect(container.querySelector('.sstv-thumb')).not.toBeNull(),{timeout:2000})
  rerender(view(false));expect(container.querySelector('.sstv-live-canvas')).toBeNull()
  for(const forbidden of ['sstv_auto_arm','sstv_arm','sstv_send','sstv_stop','sstv_delete_image','set_rf_power','get_licensed_band_plan'])expect(calls).not.toContain(forbidden)
  unmount();context.dispose()
})

it('connects the real APRS roster and keeps tune, beacon, message and settings actions disabled',async()=>{
  const current=structuredClone(snapshot),settings=projectedSettings(),calls:string[]=[]
  let available=true
  const read=async <T,>(command:string):Promise<T>=>{
    calls.push(command)
    if(command==='get_snapshot')return current as T
    if(command==='get_settings')return settings as T
    if(command==='get_band_plan')return [] as T
    if(command==='get_remote_aprs_state'&&available)return parseAprsLive(structuredClone(aprsFixture),0) as T
    if(command==='get_spectrum_row')return {row:[],loHz:0,hiHz:4000,source:'audio'} as T
    if(command==='get_meters')return {rxLevel:0,smeterDb:null,cwToneHz:null} as T
    throw new Error('applicationUnsupported')
  }
  dispose=installApplicationTransport({kind:'remote',invoke:read})
  const context=new RemoteCollections({supports:()=>false,invoke:read} as unknown as ApplicationClient)
  vi.spyOn(context,'page').mockImplementation(async()=>({collection:'aprs',offset:0,total:6,retained:6,rows:aprsRosterFixture.rows,nextCursor:null,ageMs:0,
    snapshotId:'8aa041cb-c642-459c-83f3-11a5b720647d',meta:{capturedAgeMs:0,source:aprsRosterFixture.meta}} as unknown as QueryPage))
  const view=(live:boolean)=><StationControlContext.Provider value={false}><StationDataContext.Provider value={live}>
    <RemoteCollectionsContext.Provider value={context}><App remote={{snapshot:current,settings,bandPlan:[],stationModes:true,status:<div>Observer</div>}}/></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const {container,rerender,unmount}=render(view(true))
  fireEvent.click(screen.getByRole('button',{name:/^APRS —/}))
  await waitFor(()=>expect(container.querySelector('.aprs-cockpit')?.textContent).toContain('K2ABC'))
  expect(container.querySelectorAll('.aprs-cockpit')).toHaveLength(1)
  await waitFor(()=>expect(container.querySelector<HTMLInputElement>('.aprs-beacon-comment input')?.value).toBe('Nexus station'))
  const controls=container.querySelectorAll<HTMLButtonElement>('.aprs-retune,.aprs-beacon-send')
  expect(controls.length).toBeGreaterThanOrEqual(2)
  for(const button of controls){expect(button.disabled).toBe(true);fireEvent.click(button)}
  const message=container.querySelector<HTMLInputElement>('.aprs-message-compose .aprs-beacon-comment input')!
  fireEvent.change(message,{target:{value:'TEST MESSAGE'}});fireEvent.keyDown(message,{key:'Enter'})
  expect(message.value).toBe('TEST MESSAGE')
  rerender(view(false));expect(container.querySelector('.aprs-cockpit')?.textContent).not.toContain('K2ABC')
  available=false;rerender(view(true))
  await waitFor(()=>expect(container.querySelector('.aprs-health')?.textContent).not.toContain('Receiving'))
  for(const forbidden of ['aprs_auto_arm','aprs_arm','aprs_send_beacon','aprs_send_message','aprs_tune','get_aprs_stations','set_settings'])expect(calls).not.toContain(forbidden)
  unmount();context.dispose()
})


it.each(['connect','sats'] as const)('connects the actual %s section, keeps its selections usable and clears lost station data',async(section)=>{
  const current=structuredClone(snapshot),settings=projectedSettings(),calls:string[]=[],queries:string[]=[]
  let available=true
  const read=async <T,>(command:string):Promise<T>=>{
    calls.push(command)
    if(command==='get_snapshot')return current as T
    if(command==='get_settings')return settings as T
    if(command==='get_band_plan')return [] as T
    if(command==='get_remote_satellite_state'&&available)return parseSatelliteLive(structuredClone(navigationLive),0) as T
    if(command==='get_spectrum_row')return {row:[],loHz:0,hiHz:4000,source:'audio'} as T
    if(command==='get_meters')return {rxLevel:0,smeterDb:null,cwToneHz:null} as T
    throw new Error('applicationUnsupported')
  }
  dispose=installApplicationTransport({kind:'remote',invoke:read})
  const context=new RemoteCollections({supports:()=>false,invoke:read} as unknown as ApplicationClient)
  vi.spyOn(context,'page').mockImplementation(async args=>{
    queries.push(args.collection)
    if(!available)throw new Error('applicationUnavailable')
    const data={connect:navigationConnect,path:navigationPath,satellites:navigationSatellites,satellite:navigationSatellite}[args.collection as 'connect']
    if(!data)throw new Error('applicationUnsupported')
    const pages=navigationPages(args.collection,data,args.search)
    return pages[args.cursor?Number(args.cursor.split(':')[1]):0]
  })
  const view=(live:boolean)=><StationControlContext.Provider value={false}><StationDataContext.Provider value={live}>
    <RemoteCollectionsContext.Provider value={context}><App remote={{snapshot:current,settings,bandPlan:[],navigation:true,status:<div>Observer</div>}}/></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const {container,rerender,unmount}=render(view(true))
  fireEvent.click(screen.getByRole('button',{name:section==='connect'?/^Connect —/:/^Satellites —/}))
  if(section==='connect'){
    await waitFor(()=>expect(container.querySelector('.connect-header')?.textContent).toContain('Station data · Read only'))
    expect(container.querySelectorAll('.connect-shell')).toHaveLength(1)
    const intents=container.querySelectorAll<HTMLButtonElement>('.connect-intent button')
    expect(intents.length).toBe(4);fireEvent.click(intents[2]);expect(intents[2].classList.contains('active')).toBe(true)
    expect(queries).toContain('connect')
    for(const command of ['get_band_outlook','get_getting_out','get_space_wx_scales','get_path_outlook'])expect(calls).not.toContain(command)
  }else{
    await waitFor(()=>expect(container.querySelector('.sats-view')?.textContent).toContain('ISS (ZARYA)'))
    const pick=container.querySelector<HTMLButtonElement>('.sat-pick')!;expect(pick).not.toBeNull();fireEvent.click(pick)
    await waitFor(()=>expect(queries).toContain('satellite'))
    const controls=container.querySelectorAll<HTMLButtonElement>('.sat-track')
    expect(controls.length).toBeGreaterThan(0)
    for(const control of controls){expect(control.disabled).toBe(true);fireEvent.click(control)}
    expect(container.querySelector<HTMLInputElement>('input.sats-search')).not.toBeNull();expect(container.querySelector<HTMLInputElement>('input.sats-search')!.disabled).toBe(false)
  }
  available=false;rerender(view(false))
  if(section==='connect')expect(container.querySelector('.connect-header')?.textContent).not.toContain('Station data')
  else expect(container.querySelector('.sat-transponders')).toBeNull()
  for(const command of ['select_peer','set_sat_transponder','start_sat_track','stop_sat_track','set_settings','confirm_sat_uplink','fetch_tles_now','set_peg_lock'])expect(calls).not.toContain(command)
  unmount();context.dispose()
})


it.each(['settings','program'] as const)('connects the actual %s section with saved station values, usable navigation and no native side effects',async section=>{
  const current=structuredClone(snapshot),settings=projectedSettings(),calls:string[]=[]
  const read=async <T,>(command:string):Promise<T>=>{
    calls.push(command)
    if(command==='get_snapshot')return current as T
    if(command==='get_settings')return settings as T
    if(command==='get_band_plan')return [] as T
    if(command==='get_spectrum_row')return {row:[],loHz:0,hiHz:4000,source:'audio'} as T
    if(command==='get_meters')return {rxLevel:0,smeterDb:null,cwToneHz:null} as T
    throw new Error('applicationUnsupported')
  }
  dispose=installApplicationTransport({kind:'remote',invoke:read})
  const context=new RemoteCollections({supports:()=>false,invoke:read} as unknown as ApplicationClient)
  const pages=navigationPages(section==='settings'?'settings':'programming',section==='settings'?configurationSettings:configurationProgramming)
  vi.spyOn(context,'page').mockImplementation(async args=>pages[args.cursor?Number(args.cursor.split(':')[1]):0])
  const view=(live:boolean)=><StationControlContext.Provider value={false}><StationDataContext.Provider value={live}>
    <RemoteCollectionsContext.Provider value={context}><App remote={{snapshot:current,settings,bandPlan:[],configuration:true,status:<div>Observer</div>}}/></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const {container,rerender,unmount}=render(view(true))
  fireEvent.click(screen.getByRole('button',{name:section==='settings'?/^Settings$/:/^Program —/}))
  if(section==='settings'){
    await waitFor(()=>expect(container.querySelector('.settings-panel')?.textContent).toContain('Station settings'))
    expect(container.querySelectorAll('.settings-panel')).toHaveLength(1)
    const tabs=[...container.querySelectorAll<HTMLButtonElement>('.settings-tab')]
    expect(tabs.length).toBeGreaterThan(5)
    let checkedInputs=0
    for(const tab of tabs){
      expect(tab.disabled).toBe(false);fireEvent.click(tab)
      for(const disclosure of container.querySelectorAll<HTMLButtonElement>('.settings-group-toggle'))fireEvent.click(disclosure)
      for(const input of container.querySelectorAll<HTMLInputElement>('.settings-scroll input,.settings-scroll select,.settings-scroll textarea')){
        if(input.closest('.settings-search,.theme-switcher,.watchlist'))continue
        checkedInputs++;expect(input.matches(':disabled'),input.outerHTML).toBe(true)
      }
    }
    expect(checkedInputs).toBeGreaterThan(60)
    expect(container.querySelector('input[type="password"]')).toBeNull()
    expect(container.textContent).not.toContain('must-not-leave-station')
  }else{
    await waitFor(()=>expect(container.querySelector<HTMLInputElement>('.rp-chan-row:last-child .rp-chan-name')?.value).toBe('CH1199'),{timeout:4000})
    expect(container.querySelectorAll('.radioprog')).toHaveLength(1)
    const controls=container.querySelectorAll<HTMLButtonElement>('.radioprog button')
    expect(controls.length).toBeGreaterThan(10)
    for(const control of controls){expect(control.disabled,control.outerHTML).toBe(true);fireEvent.click(control)}
  }
  rerender(view(false))
  if(section==='settings')expect(container.querySelector('.settings-tabs')).toBeNull()
  else expect(container.querySelector('.rp-body')?.hasAttribute('hidden')).toBe(true)
  unmount();context.dispose()
  for(const command of ['get_credentials_status','serial_ports','radioprog_list_projects','radioprog_save_projects','radioprog_search','set_settings','get_audio_devices','set_dial_mhz','set_operating_mode','list_configs','remote_service_status'])expect(calls).not.toContain(command)
  expect(calls.filter(c=>/^(set_|save_|radioprog_|amp_command|start_|stop_|halt_|log_manual|store_|delete_|remote_)/.test(c))).toEqual([])
})

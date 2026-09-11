// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { CwCockpit } from '../components/CwCockpit'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import settings from '../components/__fixtures__/defaultSettings.json'
import { RemoteObservationContext } from './amplifier-observation'
import frames from '../remote-monitor/fixtures.v2.json'
import type { MonitorState } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
// The real cockpit owns the DSP and AGC controls. Only unrelated heavy children are
// substituted here; the compiled browser suite exercises the full application.
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, getCatCwUnprovenRigModels, cwDecode, setRfPower, setMicGain, setNrLevel, setCompLevel, setNotchFreq, setPtt } from '../api'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(mode: 'cw' | 'phone' = 'phone', capabilities: ControlCapability[] = ['radioLevels'], version: 2 | 3 = 3, local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(getCatCwUnprovenRigModels).mockResolvedValue([])
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 22, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: mode,
      dialMhz: 14.275, band: '20m', sideband: 'USB', rigMode: mode === 'cw' ? 'CW' : 'LSB', catOk: true,
      txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: mode === 'cw' ? 500 : 2400,
      rfPower: 0.5, micGain: 0.5, compLevel: 0.5, notchFreqHz: 600, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: false, manualNotch: false, comp: false, vox: false,
      splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, version,
    pendingControlStorage(() => storage, 'radio-levels', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn(), Component = mode === 'cw' ? CwCockpit : PhoneCockpit
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const view = (current = snap, available = true, shown = observation) => <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={local ? null : client}>
      <RemoteObservationContext.Provider value={shown}>
      <Component snap={current} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={onSnap}/>
      </RemoteObservationContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(view())
  const writes = () => sent.filter(w => w.request.type === 'stationControl')
  const finish = (outcome: 'applied' | 'unknown' = 'applied') => {
    const ws = writes(), request = ws[ws.length - 1].request
    reply({ operation: 'stationControl', operationId: request.requestId, outcome,
      ...(outcome === 'applied' ? { evidence: 'radioReadback' } : { reason: 'hardwareUnconfirmed' }) })
  }
  return { ...ui, snap, state, client, reply, onSnap, view, writes, finish, observation,
    functions: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-dsp-btn')],
    agc: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-agc button')],
    func: (label: string) => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-dsp-btn')].find(b => b.textContent === label)! }
}


import { t } from '../i18n'
import { CockpitHeader } from '../components/CockpitHeader'
const choices = [
  ['phone','power','rfPower','phone.header.power.label',50,35],
  ['phone','micGain','micGain','phone.mic.aria',50,35],
  ['phone','nr','nrLevel','phone.rxDsp.nr.aria',30,35],
  ['phone','compression','compLevel','phone.rxDsp.comp.aria',50,35],
  ['phone','notch','notchFreqHz','phone.rxDsp.notchFreq.aria',600,1500],
  ['cw','nr','nrLevel','cw.rxDsp.nr.aria',30,35],
] as const
it.each(choices)('actual %s %s control waits for confirmed station samples', async (mode,level,field,label,before,target) => {
  const h=fixture(mode); await tick()
  const slider=()=>h.getByRole('slider',{name:t(label)}) as HTMLInputElement
  expect(slider().disabled).toBe(false);expect(Number(slider().value)).toBe(before)
  fireEvent.change(slider(),{target:{value:String(target)}});await tick()
  expect(h.writes()).toHaveLength(1)
  const scale=level==='notch'?1:100
  expect(h.writes()[0].request.action).toEqual({action:'radio.level',mode,level,expected:before/scale,value:target/scale})
  expect(h.writes()[0].request.context).toEqual(h.state.controls!.context)
  expect(Number(slider().value)).toBe(before)
  act(()=>h.finish());await tick()
  expect(Number(slider().value)).toBe(before);expect(h.onSnap).not.toHaveBeenCalled()
  h.rerender(h.view({...h.snap,radio:{...h.snap.radio,[field]:target/scale}}));await tick()
  expect(Number(slider().value)).toBe(target)
  for(const fn of [setRfPower,setMicGain,setNrLevel,setCompLevel,setNotchFreq,setPtt]) expect(fn).not.toHaveBeenCalled()
})

it.each(['capability','version','unavailable','keyed','armed','busy','different radio','different connection'] as const)
('level controls refuse %s without dispatch',async problem=>{
  const h=fixture('phone',problem==='capability'?[]:['radioLevels'],problem==='version'?2:3);await tick()
  const snap=structuredClone(h.snap)
  if(problem==='keyed')snap.radio.rigKeyed=true
  if(problem==='armed')snap.radio.txEnabled=true
  if(problem==='busy')snap.radio.txBusyReason='busy'
  if(problem==='different radio')snap.activeRadioId=2
  const shown=structuredClone(h.observation)
  if(problem==='different connection')shown.frame!.station.radio.readings.cat!.connectionGeneration=8
  h.rerender(h.view(snap,problem!=='unavailable',shown));await tick()
  for(const [, , ,label] of choices.filter(c=>c[0]==='phone')) {
    const slider=h.getByRole('slider',{name:t(label)}) as HTMLInputElement
    expect(slider.disabled).toBe(true)
    fireEvent.change(slider,{target:{value:'40'}})
  }
  await tick();expect(h.writes()).toHaveLength(0)
})

it('an unconfirmed level keeps the actual value and does not replay on navigation',async()=>{
  const h=fixture();await tick()
  const slider=()=>h.getByRole('slider',{name:t('phone.mic.aria')}) as HTMLInputElement
  fireEvent.change(slider(),{target:{value:'35'}});await tick()
  act(()=>h.finish('unknown'));await tick()
  h.rerender(h.view());await tick(1000)
  expect(Number(slider().value)).toBe(50);expect(h.writes()).toHaveLength(1)
})

it('native level controls keep their local command path',async()=>{
  const h=fixture('phone',[],3,true);await tick()
  fireEvent.change(h.getByRole('slider',{name:t('phone.mic.aria')}),{target:{value:'35'}});await tick()
  expect(setMicGain).toHaveBeenCalledWith(0.35);expect(h.writes()).toHaveLength(0)
})

it('a radio-level capability never enables the shared header audio-drive control',async()=>{
  const h=fixture();await tick();h.unmount()
  const changed=vi.fn()
  const ui=render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteObservationContext.Provider value={h.observation}>
      <CockpitHeader snap={h.snap} modeIndicator={null} bandControl={null} power={{value:0.5,unit:'drive',onChange:changed,label:'Drive'}}/>
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const input=ui.getByRole('slider',{name:'Drive'}) as HTMLInputElement
  expect(input.disabled).toBe(true);fireEvent.change(input,{target:{value:'0.7'}});await tick()
  expect(changed).not.toHaveBeenCalled();expect(h.writes()).toHaveLength(0)
})


it.each(choices)('%s %s drag submits the released target once and waits for a station sample', async(mode,level,field,label,before,target)=>{
  const h=fixture(mode);await tick()
  const slider=()=>h.getByRole('slider',{name:t(label)}) as HTMLInputElement
  fireEvent.pointerDown(slider(),{pointerId:1})
  fireEvent.change(slider(),{target:{value:String(target+(level==='notch'?10:1))}});await tick()
  fireEvent.change(slider(),{target:{value:String(target)}});await tick()
  expect(Number(slider().value)).toBe(target);expect(h.writes()).toHaveLength(0)
  fireEvent.pointerUp(slider(),{pointerId:1});await tick()
  expect(h.writes()).toHaveLength(1)
  const scale=level==='notch'?1:100
  expect(h.writes()[0].request.action).toEqual({action:'radio.level',mode,level,expected:before/scale,value:target/scale})
  expect(Number(slider().value)).toBe(before)
  act(()=>h.finish());await tick()
  expect(Number(slider().value)).toBe(before)
  h.rerender(h.view({...h.snap,radio:{...h.snap.radio,[field]:target/scale}}));await tick()
  expect(Number(slider().value)).toBe(target)
})

it.each(['pointer cancel','blur','permission loss','reading change','radio change','mode change'] as const)
('a level drag canceled by %s cannot revive when the context returns', async(reason)=>{
  const h=fixture();await tick()
  const slider=()=>h.getByRole('slider',{name:t('phone.mic.aria')}) as HTMLInputElement
  fireEvent.pointerDown(slider(),{pointerId:1});fireEvent.change(slider(),{target:{value:'35'}});await tick()
  if(reason==='pointer cancel')fireEvent.pointerCancel(slider(),{pointerId:1})
  else if(reason==='blur')fireEvent.blur(slider())
  else {
    const next=structuredClone(h.snap)
    if(reason==='reading change')next.radio.micGain=0.6
    if(reason==='radio change')next.activeRadioId=2
    if(reason==='mode change')next.radio.operatingMode='cw'
    h.rerender(h.view(next,reason!=='permission loss'));await tick()
    h.rerender(h.view());await tick()
  }
  fireEvent.change(slider(),{target:{value:'42'}});fireEvent.pointerUp(slider(),{pointerId:1});await tick()
  expect(h.writes()).toHaveLength(0);expect(Number(slider().value)).toBe(50)
  fireEvent.pointerDown(slider(),{pointerId:2});fireEvent.change(slider(),{target:{value:'42'}});fireEvent.pointerUp(slider(),{pointerId:2});await tick()
  expect(h.writes()).toHaveLength(1);expect(h.writes()[0].request.action.value).toBe(0.42)
})

it('held adjustment keys submit on release and cannot restart a canceled edit through key repeat', async()=>{
  const h=fixture();await tick()
  const slider=()=>h.getByRole('slider',{name:t('phone.mic.aria')}) as HTMLInputElement
  fireEvent.keyDown(slider(),{key:'ArrowRight'});fireEvent.change(slider(),{target:{value:'51'}});await tick()
  h.rerender(h.view(h.snap,false));await tick();h.rerender(h.view());await tick()
  fireEvent.keyDown(slider(),{key:'ArrowRight',repeat:true});fireEvent.change(slider(),{target:{value:'52'}})
  fireEvent.keyUp(slider(),{key:'ArrowRight'});await tick();expect(h.writes()).toHaveLength(0)
  fireEvent.keyDown(slider(),{key:'ArrowRight'});fireEvent.change(slider(),{target:{value:'51'}})
  fireEvent.keyDown(slider(),{key:'ArrowRight',repeat:true});fireEvent.change(slider(),{target:{value:'52'}});await tick()
  expect(h.writes()).toHaveLength(0);fireEvent.keyUp(slider(),{key:'ArrowRight'});await tick()
  expect(h.writes()).toHaveLength(1);expect(h.writes()[0].request.action.value).toBe(0.52)
})


it.each(['leaseId','stationBootId','revision'] as const)('a drag cannot cross a changed %s even if the original state returns',async field=>{
  const h=fixture();await tick()
  const slider=()=>h.getByRole('slider',{name:t('phone.mic.aria')}) as HTMLInputElement
  fireEvent.pointerDown(slider());fireEvent.change(slider(),{target:{value:'35'}});await tick()
  await tick(1000)
  act(()=>h.reply({...h.state,[field]:field==='revision'?h.state.revision+1:crypto.randomUUID()}));await tick()
  await tick(1000);act(()=>h.reply(h.state));await tick()
  fireEvent.change(slider(),{target:{value:'42'}});fireEvent.pointerUp(slider());await tick()
  expect(h.writes()).toHaveLength(0)
  fireEvent.pointerDown(slider());fireEvent.change(slider(),{target:{value:'42'}});fireEvent.pointerUp(slider());await tick()
  expect(h.writes()).toHaveLength(1)
})

// The toast-corner probe's stand-in backend (toastcorner-probe.mjs): the real app, a fake station.
// Settings overrides come from localStorage 'probe.settings'; every other api function the probe does not
// name answers {} (or [] on the Phone and CW screens), as the other desktop probes' stubs do.
import settingsFixture from '../src/components/__fixtures__/defaultSettings.json'

;(globalThis as any).__DEFAULT_ARR = location.hash.match(/phone|cw|psk/) != null
let over: any = {}
try { over = JSON.parse(localStorage.getItem('probe.settings') ?? '{}') } catch {}
const settings: any = { ...settingsFixture, ...over }
if (over.rigModel != null) settings.radios = settings.radios.map((r: any, i: number) => (i === 0 ? { ...r, rigModel: over.rigModel, rigModelName: over.rigModelName ?? r.rigModelName, rigConn: over.rigConn ?? r.rigConn } : r))
const keyed = localStorage.getItem('probe.keyed') === '1'
const phone = location.hash.includes('phone')
let snapshot: any = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: { dialMhz: phone ? 14.25 : 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: keyed, rigKeyed: keyed, txEnabled: keyed, txAllowed: true,
    rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0, operatingMode: phone ? 'USB' : 'DIGU',
    txSwr: 1.3, txAlc: 0.4, txPoW: 87, ratedWatts: 100, swrScaleVerified: true },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
}
const calls: string[] = []
;(window as any).__probeCalls = calls
let listener: ((s: any) => void) | null = null

export function isTauri(): boolean { return false }
export function subscribeSnapshot(fn: (s: any) => void): () => void { listener = fn; return () => { listener = null } }
export async function getSnapshot(): Promise<any> { return snapshot }
export async function setOperatingMode(): Promise<any> { return snapshot }
export async function setArea(): Promise<any> { return snapshot }
export async function getSettings(): Promise<any> { return settings }
export async function saveSettings(s: any): Promise<any> { calls.push('saveSettings'); Object.assign(settings, s); return settings }
export async function haltTx(): Promise<any> { calls.push('haltTx'); return snapshot }
export async function setTune(on: boolean): Promise<any> { calls.push('setTune:' + on); return snapshot }
export async function setPtt(on: boolean): Promise<any> { calls.push('setPtt:' + on); return snapshot }
export async function setTxEnabled(on: boolean): Promise<any> { calls.push('setTxEnabled:' + on); return snapshot }
export async function clientStatus(): Promise<any> { return null }
export async function askLog(): Promise<any> { throw new Error('no log') }
export async function getAwards(): Promise<any> { return { achievements: [] } }
export async function getJourney(): Promise<any> { return { firsts: [], feats: [], ladders: [] } }
export async function getBandPlan(): Promise<any> { return [] }
export async function getLicensedBandPlan(): Promise<any> { return [] }
export async function getFdRuleset(): Promise<any> { return null }
export async function logOperators(): Promise<any> { return [] }
export async function logActivations(): Promise<any> { return [] }
export async function radioLaunchInfo(): Promise<any> { return { showPicker: false } }
export async function uiStateLoad(): Promise<any> { return {} }
export async function getAllSpots(): Promise<any> { return [] }
export async function getNeedAlerts(): Promise<any> { return [] }
export async function getPropagation(): Promise<any> { return null }
export async function getFeedHealth(): Promise<any> { return null }
export async function getXrayNow(): Promise<any> { return null }
export async function getDxpedWindows(): Promise<any> { return [] }
export async function getSatSchedule(): Promise<any> { return [] }
export async function getSatTrackStatus(): Promise<any> { return null }
export async function getIssPass(): Promise<any> { return null }
export async function getTleStatus(): Promise<any> { return null }
export async function appVersion(): Promise<any> { return '0.0.0-probe' }
export async function getSpectrumRow(): Promise<any> { return { row: new Array(512).fill(0.1), loHz: 0, hiHz: 4000, source: 'audio' } }
export async function getScopeFrame(lo: number, hi: number): Promise<any> {
  return { seq: 1, tMs: Date.now(), source: 'audio', loHz: lo, hiHz: hi, scale: { kind: 'relative' }, bins: new Array(512).fill(0.2) }
}
export async function getRfFrame(): Promise<any> { return null }
export async function cwDecode(): Promise<any> {
  return { text: '', pending: '', wpm: null, sent: [], keyerError: null, paddleTookOver: false, unassisted: false, candidates: [],
    workedCall: null, rst: null, name: null, turn: null }
}
;(window as any).__probePush = () => listener?.(snapshot)

// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import App from '../App'
import { installApplicationTransport } from '../applicationTransport'
import type { AppSnapshot, Settings } from '../types'
import settingsFixture from '../components/__fixtures__/defaultSettings.json'
import { StationControlContext, StationDataContext } from '../stationAccess'
import { Dialog } from '../components/ui/Dialog'
import { cwDecode, getScopeRow } from '../api'

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
afterEach(() => { cleanup(); dispose?.(); vi.unstubAllGlobals() })

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

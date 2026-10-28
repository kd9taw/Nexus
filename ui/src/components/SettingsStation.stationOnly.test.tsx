// @vitest-environment jsdom
//
// From a computer driving a station that has "My station" profiles, the call is set at the station:
// the read names `mycall` among the settings the save keeps as the station has it, and the
// callsign field shows the station's call read-only with "Set at the station". The grid beside it
// stays the form's. With no profiles the read does not name it, and the field is editable as before.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { SettingsStation } from './SettingsStation'
import type { Settings } from '../types'
import { t } from '../i18n'

afterEach(cleanup)

const form = {
  mycall: 'KD9TAW', mygrid: 'EN52', opName: 'Op', licenseClass: 'general',
  units: 'auto', dialMhz: 14.074, band: '20m', sideband: 'USB',
} as unknown as Settings

function mount(stationOnly: string[], callWaits = false) {
  const locked = (key: string) => stationOnly.includes(key)
  const setAtStation = (key: string) =>
    locked(key) ? <span data-set-at-station={key}>{t('settings.stationOnly.hint')}</span> : null
  render(<SettingsStation form={form} error={null} bandPlan={[]} onUpdate={vi.fn()} onSetFreq={vi.fn()}
    locked={locked as never} setAtStation={setAtStation as never} callWaits={callWaits} />)
  return {
    callsign: screen.getByDisplayValue('KD9TAW') as HTMLInputElement,
    grid: screen.getByDisplayValue('EN52') as HTMLInputElement,
  }
}

const said = (el: Element) => el.closest('label')?.querySelector('[data-set-at-station]')?.textContent ?? null

it('the call is read-only from afar while the station has profiles, and says so', () => {
  const { callsign, grid } = mount(['mycall'])
  expect(callsign.matches(':disabled')).toBe(true)
  expect(said(callsign)).toBe(t('settings.stationOnly.hint'))
  expect(grid.matches(':disabled'), 'CONTROL: the grid is the form\'s').toBe(false)
  expect(said(grid)).toBeNull()
})

it('with no profiles the call is editable, as before', () => {
  const { callsign } = mount([])
  expect(callsign.matches(':disabled')).toBe(false)
  expect(said(callsign)).toBeNull()
})

// At the station, while a run, a contact or a JS8 message is under way on a station with profiles
// (`stationProfileLocked`), the call stays as the station has it: the field is read-only and says why.
const waits = (el: Element) => el.closest('label')?.querySelector('[data-call-waits]')?.textContent ?? null

it('the call waits while a run or contact is under way, and says why', () => {
  const { callsign, grid } = mount([], true)
  expect(callsign.matches(':disabled')).toBe(true)
  expect(waits(callsign)).toBe(t('settings.station.callWaits'))
  expect(grid.matches(':disabled'), 'CONTROL: the grid is the form\'s').toBe(false)
  expect(waits(grid)).toBeNull()
})

it('idle, the call is editable and says nothing more', () => {
  const { callsign } = mount([], false)
  expect(callsign.matches(':disabled')).toBe(false)
  expect(waits(callsign)).toBeNull()
})

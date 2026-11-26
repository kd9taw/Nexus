// @vitest-environment jsdom
//
// Settings ▸ Station ▸ My station profiles. A new profile starts as the station is now (only what
// differs needs typing) and is saved through its own writer, never the panel's Save; Home and the
// profile in use cannot be removed; from a computer driving the station the list is shown and
// nothing in it is editable.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { Settings } from '../types'

const saveStationProfile = vi.fn(async (..._a: unknown[]) => ({}))
const removeStationProfile = vi.fn(async (..._a: unknown[]) => ({}))
const setStationProfileQrzKey = vi.fn(async (..._a: unknown[]) => {})
const stationProfileQrzKeyStored = vi.fn(async (..._a: unknown[]) => false)
const qrzTestConnection = vi.fn(async (..._a: unknown[]) => 'KD9TAW/P (Portable) — 3 QSOs in the online logbook')
vi.mock('../api', () => ({
  saveStationProfile: (...a: unknown[]) => saveStationProfile(...a),
  removeStationProfile: (...a: unknown[]) => removeStationProfile(...a),
  setStationProfileQrzKey: (...a: unknown[]) => setStationProfileQrzKey(...a),
  stationProfileQrzKeyStored: (...a: unknown[]) => stationProfileQrzKeyStored(...a),
  qrzTestConnection: (...a: unknown[]) => qrzTestConnection(...a),
}))

import { StationProfiles } from './StationProfiles'

afterEach(() => {
  cleanup()
  saveStationProfile.mockClear()
  removeStationProfile.mockClear()
  setStationProfileQrzKey.mockClear()
  stationProfileQrzKeyStored.mockReset()
  stationProfileQrzKeyStored.mockImplementation(async () => false)
  qrzTestConnection.mockClear()
})

const station = {
  mycall: 'KD9TAW', mygrid: 'EN52', opState: 'WI', stationPowerW: 100,
} as unknown as Settings

const blank = { county: '', cqZone: null, ituZone: null, iota: '', park: '', rig: '', antenna: '', lotwLocation: '' }

describe('My station profiles', () => {
  it('adds a profile that starts as the station is now, through its own writer', async () => {
    const onChanged = vi.fn()
    render(<StationProfiles form={station} readOnly={false} onChanged={onChanged} />)
    fireEvent.click(screen.getByText('Add a profile…'))
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Portable' } })
    fireEvent.change(screen.getByLabelText('POTA park', { exact: false }), { target: { value: 'US-1234' } })
    fireEvent.click(screen.getByText('Save profile'))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    expect(saveStationProfile).toHaveBeenCalledWith({
      id: '', name: 'Portable', call: 'KD9TAW', grid: 'EN52', state: 'WI', powerW: 100,
      ...blank, park: 'US-1234',
    })
  })

  it('keeps Home and the profile in use, and removes another', async () => {
    const form = {
      ...station,
      activeStationProfile: 'p1',
      stationProfiles: [
        { id: 'home', name: '', call: 'KD9TAW', grid: 'EN52', state: 'WI', powerW: 100, ...blank },
        { id: 'p1', name: 'Portable', call: 'KD9TAW/P', grid: 'EN43', state: 'WI', powerW: 10, ...blank },
        { id: 'p2', name: 'Club', call: 'W9ABC', grid: 'EN52', state: 'WI', powerW: 100, ...blank },
      ],
    } as unknown as Settings
    const onChanged = vi.fn()
    render(<StationProfiles form={form} readOnly={false} onChanged={onChanged} />)
    expect(screen.getAllByText('Remove')).toHaveLength(1)
    fireEvent.click(screen.getByText('Remove'))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    expect(removeStationProfile).toHaveBeenCalledWith('p2')
  })

  it('names the TQSL Station Location a profile signs its LoTW contacts with', async () => {
    const form = {
      ...station,
      activeStationProfile: 'home',
      stationProfiles: [
        { id: 'home', name: '', call: 'KD9TAW', grid: 'EN52', state: 'WI', powerW: 100, ...blank },
        { id: 'p1', name: 'Portable', call: 'KD9TAW/P', grid: 'EN43', state: 'WI', powerW: 10, ...blank },
      ],
    } as unknown as Settings
    const onChanged = vi.fn()
    render(<StationProfiles form={form} readOnly={false} onChanged={onChanged} />)
    fireEvent.click(screen.getAllByText('Edit')[1])
    expect(screen.getByText(/Leave blank to use the one in Settings/)).toBeTruthy()
    fireEvent.change(screen.getByLabelText('TQSL Station Location', { exact: false }), {
      target: { value: 'Portable EN43' },
    })
    fireEvent.click(screen.getByText('Save profile'))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    expect(saveStationProfile).toHaveBeenCalledWith({
      id: 'p1', name: 'Portable', call: 'KD9TAW/P', grid: 'EN43', state: 'WI', powerW: 10,
      ...blank, lotwLocation: 'Portable EN43',
    })
  })

  // #503: a profile's own QRZ Logbook key, kept in the system keychain through its own writer,
  // never in the profile the form saves.
  it("keeps a profile's own QRZ Logbook key apart from the profile, with a Test", async () => {
    const form = {
      ...station,
      activeStationProfile: 'home',
      stationProfiles: [
        { id: 'home', name: '', call: 'KD9TAW', grid: 'EN52', state: 'WI', powerW: 100, ...blank },
        { id: 'p1', name: 'Portable', call: 'KD9TAW/P', grid: 'EN43', state: 'WI', powerW: 10, ...blank },
      ],
    } as unknown as Settings
    render(<StationProfiles form={form} readOnly={false} onChanged={vi.fn()} />)
    fireEvent.click(screen.getAllByText('Edit')[1])
    await waitFor(() => expect(stationProfileQrzKeyStored).toHaveBeenCalledWith('p1'))
    expect(await screen.findByText(/uses the key in Settings ▸ Services/)).toBeTruthy()
    const field = screen.getByLabelText('QRZ Logbook API key') as HTMLInputElement
    expect(field.type).toBe('password')
    fireEvent.change(field, { target: { value: 'A1B2-C3D4-E5F6-0789' } })
    stationProfileQrzKeyStored.mockImplementation(async () => true)
    fireEvent.click(screen.getByText('Save key'))
    await waitFor(() => expect(setStationProfileQrzKey).toHaveBeenCalledWith('p1', 'A1B2-C3D4-E5F6-0789'))
    expect(await screen.findByText('This profile has its own key.')).toBeTruthy()
    expect((screen.getByLabelText('QRZ Logbook API key') as HTMLInputElement).value).toBe('')
    fireEvent.click(screen.getByText('Test'))
    await waitFor(() => expect(qrzTestConnection).toHaveBeenCalledWith('p1'))
    expect(await screen.findByText(/3 QSOs in the online logbook/)).toBeTruthy()
    fireEvent.click(screen.getByText('Remove key'))
    await waitFor(() => expect(setStationProfileQrzKey).toHaveBeenCalledWith('p1', ''))
    fireEvent.click(screen.getByText('Save profile'))
    await waitFor(() => expect(saveStationProfile).toHaveBeenCalled())
    expect(JSON.stringify(saveStationProfile.mock.calls)).not.toContain('A1B2')
  })

  it('a profile not saved yet has no key field', () => {
    render(<StationProfiles form={station} readOnly={false} onChanged={vi.fn()} />)
    fireEvent.click(screen.getByText('Add a profile…'))
    expect(screen.queryByLabelText('QRZ Logbook API key')).toBeNull()
    expect(stationProfileQrzKeyStored).not.toHaveBeenCalled()
  })

  it('from a computer driving the station: shown, set at the station', () => {
    render(<StationProfiles form={station} readOnly onChanged={vi.fn()} />)
    expect(screen.queryByText('Add a profile…')).toBeNull()
    expect(screen.getByText('Set at the station.')).toBeTruthy()
  })
})

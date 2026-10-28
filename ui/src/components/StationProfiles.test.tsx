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
vi.mock('../api', () => ({
  saveStationProfile: (...a: unknown[]) => saveStationProfile(...a),
  removeStationProfile: (...a: unknown[]) => removeStationProfile(...a),
}))

import { StationProfiles } from './StationProfiles'

afterEach(() => {
  cleanup()
  saveStationProfile.mockClear()
  removeStationProfile.mockClear()
})

const station = {
  mycall: 'KD9TAW', mygrid: 'EN52', opState: 'WI', stationPowerW: 100,
} as unknown as Settings

const blank = { county: '', cqZone: null, ituZone: null, iota: '', park: '', rig: '', antenna: '' }

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

  it('from a computer driving the station: shown, set at the station', () => {
    render(<StationProfiles form={station} readOnly onChanged={vi.fn()} />)
    expect(screen.queryByText('Add a profile…')).toBeNull()
    expect(screen.getByText('Set at the station.')).toBeTruthy()
  })
})

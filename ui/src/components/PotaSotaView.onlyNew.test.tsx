// @vitest-environment jsdom
//
// ONLY NEW — the POTA/SOTA board's "show me only the references I have never logged" chip.
//
// The station decides what is new (`newPark`: the reference is on no hunter-side row of the log
// and not in the imported Hunted Parks list — parks, summits and WWFF references alike, one
// index); these tests take that as given. The board's job: offer the chip only when the rows can
// answer, hide the rest when it is on, count what it hides AFTER the other filters, remember the
// choice per surface (the pop-out keeps its own), and say so when it is what empties the board.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'

const spot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA',
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
  ...over,
})
const snap = { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot
const chip = (count?: number) =>
  screen.getByRole('button', {
    name: count === undefined ? t('ota.filter.onlyNew.label') : t('ota.filter.onlyNew.hidden', { count }),
  })

beforeEach(() => {
  api.getOtaSpots.mockReset()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  window.history.replaceState(null, '', '/')
})

describe('Only new', () => {
  it('is off by default and hides nothing', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { newPark: true }), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('W9XYZ')
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(chip().getAttribute('aria-pressed')).toBe('false')
  })

  it('on shows only the new references and counts the rest on the chip', async () => {
    api.getOtaSpots.mockResolvedValue([
      spot('K1ABC', 'US-0001', { newPark: true }),
      spot('W9XYZ', 'US-0002'),
      spot('N0OLD', 'US-0003'),
    ])
    render(<PotaSotaView snap={snap} />)
    fireEvent.click(await screen.findByRole('button', { name: t('ota.filter.onlyNew.label') }))
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(screen.queryByText('W9XYZ')).toBeNull()
    expect(screen.queryByText('N0OLD')).toBeNull()
    expect(chip(2).getAttribute('aria-pressed')).toBe('true')
  })

  it('counts what it hides AFTER the other filters — exactly what one click brings back', async () => {
    // Mode CW is picked, and Hide worked today (on by default) has already taken one row.
    localStorage.setItem('nexus.ota.modeFilter', 'CW')
    localStorage.setItem('nexus.ota.onlyNew', '1')
    api.getOtaSpots.mockResolvedValue([
      spot('K1NEW', 'US-0001', { mode: 'CW', newPark: true }),
      spot('W1OLD', 'US-0002', { mode: 'CW' }),
      spot('W2WKD', 'US-0003', { mode: 'CW', huntedToday: true }), // Hide worked's, not ours
      spot('W3SSB', 'US-0004', { mode: 'SSB' }), // the mode filter's, not ours
    ])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('K1NEW')
    expect(chip(1)).toBeTruthy()
    expect(screen.queryByText('W1OLD')).toBeNull()
  })

  it('works on a SOTA board: a never-logged summit is new', async () => {
    localStorage.setItem('nexus.ota.program', 'SOTA')
    localStorage.setItem('nexus.ota.onlyNew', '1')
    api.getOtaSpots.mockResolvedValue([
      spot('K1SUM', 'W7A/AP-001', { program: 'SOTA', newPark: true }),
      spot('W1SUM', 'W7A/AP-002', { program: 'SOTA' }),
    ])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('K1SUM')
    expect(screen.queryByText('W1SUM')).toBeNull()
    expect(chip(1)).toBeTruthy()
  })

  it('offers no chip and hides nothing when the rows carry no flag', async () => {
    // Remembered ON from a board that could answer — and still hides nothing on one that can't.
    localStorage.setItem('nexus.ota.onlyNew', '1')
    const legacy = spot('K1ABC', 'US-0001')
    delete (legacy as Partial<OtaSpot>).newPark
    api.getOtaSpots.mockResolvedValue([legacy])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('K1ABC')
    expect(screen.queryByRole('button', { name: t('ota.filter.onlyNew.label') })).toBeNull()
    expect(screen.queryByRole('button', { name: t('ota.filter.onlyNew.hidden', { count: 1 }) })).toBeNull()
  })

  it('points at the chip when it is what empties the board', async () => {
    localStorage.setItem('nexus.ota.onlyNew', '1')
    api.getOtaSpots.mockResolvedValue([spot('W9XYZ', 'US-0002'), spot('N0OLD', 'US-0003')])
    render(<PotaSotaView snap={snap} />)
    expect(await screen.findByText(t('ota.empty.notNew', { count: 2 }))).toBeTruthy()
  })

  it('is remembered between sessions, and the Connect box keeps its own', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { newPark: true }), spot('W9XYZ', 'US-0002')])
    const first = render(<PotaSotaView snap={snap} />)
    fireEvent.click(await screen.findByRole('button', { name: t('ota.filter.onlyNew.label') }))
    expect(localStorage.getItem('nexus.ota.onlyNew')).toBe('1')
    expect(localStorage.getItem('nexus.connect.ota.onlyNew'), 'the box is not moved').not.toBe('1')
    first.unmount()

    render(<PotaSotaView snap={snap} />)
    await screen.findByText('K1ABC')
    expect(screen.queryByText('W9XYZ')).toBeNull()
    expect(chip(1).getAttribute('aria-pressed')).toBe('true')
  })

  it('works in the pop-out, which inherits the view and then keeps its own', async () => {
    localStorage.setItem('nexus.ota.onlyNew', '1')
    window.history.replaceState(null, '', '/?panel=pota')
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { newPark: true }), spot('W9XYZ', 'US-0002')])
    render(
      <div className="app detached">
        <PotaSotaView snap={snap} onSnap={() => {}} detached onHunt={() => {}} />
      </div>,
    )
    await screen.findByText('K1ABC')
    expect(screen.queryByText('W9XYZ'), 'inherits the view’s choice').toBeNull()
    fireEvent.click(chip(1))
    expect(screen.getByText('W9XYZ')).toBeTruthy()
    expect(localStorage.getItem('nexus.ota.onlyNew.pota')).toBe('0')
    expect(localStorage.getItem('nexus.ota.onlyNew'), 'the main view is not moved').toBe('1')
  })
})

// @vitest-environment jsdom
//
// THE SCORING CHIPS SAVE ONLY THE SCORING. The Contest view reads the settings once, when it
// opens, and keeps that copy for its display. Its scoring save used to post the copy back WHOLE
// with the one changed field merged in, and `set_settings` takes a whole struct as the truth for
// every field in it — so each field changed anywhere else since the view opened went back to what
// it was: "use one radio" chosen in the launch picker turned simultaneous radios on again, a seat
// swap made in the pop-out scoreboard handed the log back to the previous operator, and the dial
// the rig had been tuned to since was sent back to the old one (which the radio loop then
// commands). The backend here is a live one: a read returns what it holds NOW, and a save
// replaces it, so "changed elsewhere" is a change to `station` after the view has read it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "'use one radio' chosen after the view opened stays…", takes 0.33 s
// and 0.31 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

let station: Record<string, unknown> = {}
/** How long a save takes to land at the backend, in ms (0: at once). */
let writeDelay = 0

vi.mock('../api', () => ({
  // The contest screen's Removed list, read on mount: none removed.
  contestRemoved: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...station })),
  setSettings: vi.fn(async (s: Record<string, unknown>) => {
    if (writeDelay) await new Promise((r) => setTimeout(r, writeDelay))
    station = { ...s }
    return {}
  }),
  setFdOperator: vi.fn(async (call: string) => {
    station = { ...station, fdOperator: call }
    return {}
  }),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

const FD: FieldDayStatus = {
  composing: [
    { key: 'CLASS', raw: '2A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: false,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

/** Open the view on `held`, the settings it reads when it opens and keeps from then on. */
async function openOn(held: Partial<Settings>) {
  station = { ...defaultSettings, fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [], ...held }
  render(<ContestView fieldDay={FD} onSetMode={() => {}} />)
  await settle()
}

/** Something outside this view changes the station's settings after the view has read them. */
const changedElsewhere = (change: Partial<Settings>) => {
  station = { ...station, ...change }
}

const pickQrp = async () => {
  fireEvent.click(screen.getByRole('button', { name: /Bonuses/ }))
  fireEvent.click(screen.getByRole('button', { name: '×5 QRP / battery' }))
  await settle()
}

const tickYouth = async () => {
  fireEvent.click(screen.getByRole('button', { name: /Bonuses/ }))
  fireEvent.click(screen.getByRole('checkbox', { name: /Youth Participation/ }))
  await settle()
}

afterEach(() => {
  cleanup()
  writeDelay = 0
})

describe('a scoring save keeps what changed elsewhere since the view opened', () => {
  it("'use one radio' chosen after the view opened stays chosen", async () => {
    await openOn({ simultaneousRadios: true })
    changedElsewhere({ simultaneousRadios: false })
    await pickQrp()
    expect(station.fdPowerMult).toBe(5)
    expect(station.simultaneousRadios).toBe(false)
  })

  it('the frequency the radio was tuned to since stays where it is', async () => {
    await openOn({ dialMhz: 14.025, band: '20m', sideband: 'USB' })
    changedElsewhere({ dialMhz: 7.03, band: '40m', sideband: 'LSB' })
    await tickYouth()
    expect(station.fdBonuses).toEqual(['youth'])
    expect([station.dialMhz, station.band, station.sideband]).toEqual([7.03, '40m', 'LSB'])
  })

  it('a seat swap made in the pop-out scoreboard keeps the new operator', async () => {
    await openOn({ fdOperator: 'K1ABC' })
    changedElsewhere({ fdOperator: 'W9XYZ' })
    await pickQrp()
    expect(station.fdPowerMult).toBe(5)
    expect(station.fdOperator).toBe('W9XYZ')
  })

  it('two bonuses ticked one right after the other are both saved', async () => {
    // Each save reads the station first, so the second tick must not read before the first
    // has landed: a real save takes a moment.
    await openOn({})
    writeDelay = 5
    fireEvent.click(screen.getByRole('button', { name: /Bonuses/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Youth Participation/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Safety Officer/ }))
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(station.fdBonuses).toEqual(['youth', 'safety-officer'])
  })
})

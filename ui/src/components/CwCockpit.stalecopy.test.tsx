// @vitest-environment jsdom
//
// SWITCHING THE CW MACRO SET SAVES ONLY WHICH SET IS ACTIVE. The cockpit reads the settings when
// the CW view opens and keeps that copy. The switch used to save the copy back WHOLE with the new
// set index merged in, and `set_settings` takes a whole struct as the truth for every field in it —
// so everything changed since the view opened went back: the dial the operator had tuned to since
// (which the radio loop then commands, so the rig jumped back), macro text edited from Remote,
// "use one radio" chosen in the launch picker. The backend here is a live one: a read returns what
// it holds NOW, and a save replaces it.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { AppSnapshot, Settings } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "the frequency the radio was tuned to since stays where…", takes
// 0.22 s and 0.26 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

let station: Record<string, unknown> = {}

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { get }
})

// Every export of `../api` is a spy (the SettingsPanel suites' pattern): the header's children
// call more of it than a hand-kept list would name.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})

// The macro-set switch is one of the header's children, so the stub has to render them.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: ReactNode }) => <header className="cockpit-header">{children}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const EVERYDAY = { name: 'Everyday', macros: [{ key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} K' }] }
const CONTEST = { name: 'Contest', macros: [{ key: 'F1', label: 'CQ', text: 'TEST {MYCALL}' }] }

beforeEach(() => {
  api.get('getSettings').mockImplementation(async () => structuredClone(station))
  api.get('setSettings').mockImplementation(async (s: Record<string, unknown>) => {
    station = structuredClone(s)
    return {}
  })
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening',
  }))
  api.get('previewCw').mockImplementation(async (text: string) => text)
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const snap = {
  mycall: 'KD9TAW',
  radio: { dialMhz: 14.025, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false, txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat' },
} as unknown as AppSnapshot

/** Open the cockpit on `held`, the settings it reads when the view opens and keeps from then on. */
async function openOn(held: Partial<Settings>) {
  station = structuredClone({
    ...defaultSettings,
    macros: { ...defaultSettings.macros, cwProfiles: [EVERYDAY, CONTEST], activeCwProfile: 0 },
    ...held,
  })
  render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await settle()
}

/** Something outside the cockpit changes the station's settings after the cockpit has read them. */
const changedElsewhere = (change: Partial<Settings>) => {
  station = { ...station, ...structuredClone(change) }
}

const switchToContest = async () => {
  fireEvent.change(screen.getByRole('combobox', { name: 'CW macro profile' }), { target: { value: '1' } })
  await settle()
}

const macros = () => station.macros as Settings['macros']

describe('a macro-set switch keeps what changed elsewhere since the CW view opened', () => {
  it('the frequency the radio was tuned to since stays where it is', async () => {
    await openOn({ dialMhz: 14.025, band: '20m', sideband: 'USB' })
    changedElsewhere({ dialMhz: 7.03, band: '40m', sideband: 'LSB' })
    await switchToContest()
    expect(macros().activeCwProfile).toBe(1)
    expect([station.dialMhz, station.band, station.sideband]).toEqual([7.03, '40m', 'LSB'])
  })

  it('macro text edited elsewhere (Remote may edit macros) is kept', async () => {
    await openOn({})
    const edited = { ...CONTEST, macros: [{ key: 'F1', label: 'CQ', text: 'CQ TEST {MYCALL}' }] }
    changedElsewhere({ macros: { ...macros(), cwProfiles: [EVERYDAY, edited] } })
    await switchToContest()
    expect(macros().activeCwProfile).toBe(1)
    expect(macros().cwProfiles).toEqual([EVERYDAY, edited])
  })

  it("'use one radio' chosen after the view opened stays chosen", async () => {
    await openOn({ simultaneousRadios: true })
    changedElsewhere({ simultaneousRadios: false })
    await switchToContest()
    expect(macros().activeCwProfile).toBe(1)
    expect(station.simultaneousRadios).toBe(false)
  })
})

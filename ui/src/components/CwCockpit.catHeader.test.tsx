// @vitest-environment jsdom
//
// STOP TX STAYS PUT WHEN CAT DROPS — the CW header (OperateCockpit.structure.test.tsx has the FT twin
// and the measurement). The header is in flow above the TX strip, so a box it adds or drops on a
// state change re-wraps its rows and moves Stop TX. SplitControl was drawn only while `catOk`: CAT
// going down took five boxes out of the header row. A state change may repaint a header box (the
// CAT pill, a disabled control); it may not add or remove one.
//
// ⚠️ jsdom never lays out: this pins the boxes; their sizes are the browser sweep's job.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Every export auto-stubbed from the REAL module (CwCockpit.subreceiver.test.tsx's pattern).
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getBandPlan: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    cwDecode: vi.fn(async () => ({ text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening' })),
    previewCw: vi.fn(async (t: string) => t),
    readRotator: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    getMeters: vi.fn(async () => ({ rxLevel: 0, smeterDb: null, cwToneHz: null })),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (a: () => Promise<unknown>) => a()) }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

const snap = (catOk: boolean) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: { dialMhz: 14.05, band: '20m', catOk, sideband: 'USB', rigMode: 'CW', transmitting: false, txEnabled: true, txAllowed: true,
      cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500, splitTxMhz: null, smeterDb: null },
  }) as unknown as AppSnapshot

const headerBoxes = () =>
  [...document.querySelectorAll('.cockpit-header *')].map((e) => `${e.tagName.toLowerCase()}.${e.classList[0] ?? ''}`)

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('the CW header with CAT up and down', () => {
  it('draws the same boxes either way, Split included and disabled without CAT', async () => {
    const view = render(<CwCockpit snap={snap(true)} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await settle()
    const up = headerBoxes()
    expect(up).toContain('div.ph-split')
    view.rerender(<CwCockpit snap={snap(false)} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await settle()
    expect(headerBoxes()).toEqual(up)
    const split = document.querySelector('.cockpit-header .ph-split') as HTMLElement
    for (const b of split.querySelectorAll('button')) expect((b as HTMLButtonElement).disabled).toBe(true)
  })
})

// @vitest-environment jsdom
//
// WWFF IN THE LOG FORM (#363). The program picker offers WWFF beside POTA and SOTA; a WWFF hunt
// (a WWFF spot clicked on the hunter board) fills the picker and the reference the way a POTA hunt
// fills its park; and a WWFF reference typed by hand goes out as WWFF, which the ADIF writer turns
// into SIG=WWFF / SIG_INFO / WWFF_REF and the Awards page's WWFF count reads.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const mockedLog = vi.mocked(logQso)
type Hunt = { program: string; reference: string; call: string }
const snapWith = (hunt: Hunt | null) =>
  ({ mycall: 'KD9TAW', radio: { band: '20m', dialMhz: 14.244 }, hunt }) as unknown as AppSnapshot
const strip = (hunt: Hunt | null, work: { call: string; ts: number } | null) => (
  <LogEntry snap={snapWith(hunt)} mode="SSB" defaultRst="59" exchange="terrestrial" pendingWork={work} />
)
const program = () => document.querySelector('select.le-park-prog') as HTMLSelectElement
const ref = () => document.querySelector('input.le-park-ref') as HTMLInputElement
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })

beforeEach(() => mockedLog.mockClear())
afterEach(cleanup)

describe('WWFF in the log form', () => {
  it('offers WWFF beside POTA and SOTA', () => {
    render(strip(null, null))
    expect([...program().options].map((o) => o.value)).toEqual(['POTA', 'SOTA', 'WWFF'])
  })

  it('a WWFF hunt fills the picker and the reference', async () => {
    render(strip({ program: 'WWFF', reference: 'DLFF-0001', call: 'DL0XYZ/P' }, { call: 'DL0XYZ/P', ts: 1 }))
    await waitFor(() => expect(ref().value).toBe('DLFF-0001'))
    expect(program().value).toBe('WWFF')
    expect(ref().placeholder).toBe('Area (reference)')
  })

  it('a WWFF reference typed by hand is logged under WWFF', async () => {
    render(strip(null, { call: 'K1ABC', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('K1ABC')).toBeTruthy())
    fireEvent.change(program(), { target: { value: 'WWFF' } })
    fireEvent.change(ref(), { target: { value: 'kff-1234' } })
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    const rec = mockedLog.mock.calls[0][0] as { ota?: { theirProgram?: string; theirRef?: string } }
    expect(rec.ota).toEqual({ theirProgram: 'WWFF', theirRef: 'KFF-1234' })
  })
})

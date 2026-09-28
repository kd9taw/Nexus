// @vitest-environment jsdom
//
// "WHY ARE SOME POTA LOCATIONS BEING FILLED OUT AND NOT OTHERS?" (operator, 2026-09-27). Two log
// strips with a hunt chip naming the park: VA4ADM with its park filled (CA-2475), then KE7G with
// the chip reading US-3216 and the park box EMPTY. So the hunt reached the strip both times, and
// the prefill did not run the second time.
//
// HOW A SPOT CLICK REACHES THIS STRIP. App sets the hunt, then applies the snapshot and the
// click-to-work call in one batch; the call lands in `logCall` from an effect, one render later,
// with the new hunt already in `snap`. In that render two effects run together: the call-change
// effect clears the enrichment of the previous call and its park, and the prefill effect decides
// whether to fill. The prefill read the park box as it was rendered, still holding the PREVIOUS
// activator's park, took it for a park the operator had typed, and left it alone; the clear then
// emptied the box, and nothing re-ran the prefill. Where the hunt arrives a render AFTER the call
// (a later snapshot poll), the clear has already happened and the prefill fills. Some parks
// filled and some did not.
//
// With no callbook answer, nothing clears the box at all, and the previous activator's park
// stayed there. It differed from the new hunt, so the Log button would have sent it as a park
// the operator typed: the wrong park, on the wrong contact.
//
// The rest of the file is the base call: `KE7G/P` spotted and `KE7G` logged are one station, as
// the engine's own auto-tag (`tempo_core::message::same_call`) says, and `KE7G/P` and `KF7XYZ/P`
// are not.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso, qrzLookup } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern); the overrides are
  // the callbook, the park directory and the log write this file reads.
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
    logQso: vi.fn(async (rec: unknown) => rec),
    // The logbook never answers: the recall card is not what this file is about.
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedLog = vi.mocked(logQso)

type Hunt = { program: string; reference: string; call: string }
const snapWith = (hunt: Hunt | null) =>
  ({ mycall: 'KD9TAW', radio: { band: '20m', dialMhz: 14.25 }, hunt }) as unknown as AppSnapshot

const VA4ADM: Hunt = { program: 'POTA', reference: 'CA-2475', call: 'VA4ADM' }
const KE7G: Hunt = { program: 'POTA', reference: 'US-3216', call: 'KE7G' }

function strip(hunt: Hunt | null, work: { call: string; ts: number } | null) {
  return (
    <LogEntry snap={snapWith(hunt)} mode="SSB" defaultRst="59" exchange="terrestrial" pendingWork={work} />
  )
}
const park = () => document.querySelector('input.le-park-ref') as HTMLInputElement
const logButton = () => screen.getByRole('button', { name: /^log$/i })
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })

/** A callbook that knows both activators, as the operator's did (name and QTH filled). */
function callbookKnowsThem() {
  mockedQrz.mockImplementation(async (call: string) =>
    ({ call, name: call === 'KE7G' ? 'Gene' : 'Adam', qth: call === 'KE7G' ? 'Salem' : 'Winnipeg' }) as never,
  )
}

beforeEach(() => {
  mockedQrz.mockReset().mockImplementation(async () => null as never)
  mockedLog.mockClear()
})
afterEach(cleanup)

describe('the hunted park fills the strip for every activator clicked', () => {
  it('fills the second park when the operator moves on from a first activator without logging', async () => {
    callbookKnowsThem()
    const view = render(strip(VA4ADM, { call: 'VA4ADM', ts: 1 }))
    // The first QSO, as the operator's first screenshot shows it: park and callbook both in.
    await waitFor(() => expect(park().value).toBe('CA-2475'))
    await waitFor(() => expect((screen.getByDisplayValue('Adam') as HTMLInputElement).value).toBe('Adam'))

    // The second spot: the hunt and the call arrive in one batch, as App sends them.
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 2 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    expect(park().value, 'the second hunted park never reached the strip').toBe('US-3216')
  })

  // THE CONTROL: the order that always worked. The hunt one render after the call.
  it('fills it too when the hunt arrives a render after the call', async () => {
    callbookKnowsThem()
    const view = render(strip(VA4ADM, { call: 'VA4ADM', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('CA-2475'))
    await waitFor(() => expect(screen.getByDisplayValue('Adam')).toBeTruthy())
    view.rerender(strip(VA4ADM, { call: 'KE7G', ts: 2 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 2 }))
    await settle()
    expect(park().value).toBe('US-3216')
  })

  it('never lets the first activator’s park ride onto the second call when no callbook answers', async () => {
    const view = render(strip(VA4ADM, { call: 'VA4ADM', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('CA-2475'))
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 2 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    expect(park().value, 'VA4ADM’s park is still in KE7G’s strip').toBe('US-3216')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    const rec = mockedLog.mock.calls[0][0] as { call: string; ota?: { theirRef?: string } }
    expect(rec.call).toBe('KE7G')
    // The prefill equals the pending hunt, so the engine's own callsign-matched tag supplies it;
    // what must never happen is the previous park going out as one the operator typed.
    expect(rec.ota?.theirRef, 'KE7G was logged at VA4ADM’s park').toBeUndefined()
  })

  it('does not carry a park typed for one call onto the next hunted call', async () => {
    // A park the operator typed while working W1XYZ belongs to W1XYZ. Clicking KE7G's spot next
    // must fill KE7G's park, not keep W1XYZ's (and with no callbook, nothing else would clear it).
    const view = render(strip(KE7G, { call: 'W1XYZ', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('W1XYZ')).toBeTruthy())
    await settle()
    expect(park().value, 'fixture: W1XYZ is not the hunted call').toBe('')
    fireEvent.change(park(), { target: { value: 'us-5555' } })
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 2 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    expect(park().value, 'W1XYZ’s park stuck to KE7G').toBe('US-3216')
  })

  it('keeps a park the operator typed for this call when the hunt changes under it', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    fireEvent.change(park(), { target: { value: 'us-9999' } })
    expect(park().value).toBe('US-9999')
    // The same activator's hunt moves to another park (a two-fer re-spotted, say): the strip holds
    // the operator's correction rather than the machine's guess.
    view.rerender(strip({ ...KE7G, reference: 'US-3217' }, { call: 'KE7G', ts: 1 }))
    await settle()
    expect(park().value, 'the typed park was overwritten').toBe('US-9999')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect((mockedLog.mock.calls[0][0] as { ota?: { theirRef?: string } }).ota?.theirRef).toBe('US-9999')
  })
})

describe('one station under a portable prefix or suffix, as the engine matches it', () => {
  it('fills the park when the spot says KE7G/P and the log says KE7G', async () => {
    render(strip({ ...KE7G, call: 'KE7G/P' }, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    expect(park().value).toBe('US-3216')
  })

  it('fills it for a prefix form, and the hunt chip says it matches', async () => {
    render(strip({ ...KE7G, call: 'VE7/KE7G' }, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('KE7G')).toBeTruthy())
    await settle()
    expect(park().value).toBe('US-3216')
    expect(document.querySelector('.le-hunt-chip')?.classList.contains('match'), 'the chip warns call ≠ hunt').toBe(true)
  })

  // THE CONTROL: two different stations that are both portable are still two stations.
  it('does not fill one /P activator’s park for another /P station', async () => {
    render(strip({ ...KE7G, call: 'KE7G/P' }, { call: 'KF7XYZ/P', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('KF7XYZ/P')).toBeTruthy())
    await settle()
    expect(park().value, 'KE7G/P’s park was filled for KF7XYZ/P').toBe('')
  })
})

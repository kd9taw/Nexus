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

// THE BUDGET (2026-10-09). The slowest case here, "fills the second park when the operator moves on from a…", takes
// 0.74 s and 0.74 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
    setLogFormGrid: vi.fn(async () => {}),
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

// A HUNTED PARK LEAVES WITH ITS STATION. A park a hunt put in the box belongs to that activator.
// Once the operator types another station's call, the park goes, whatever the pending hunt now is
// (another activator's, or none) and whether or not a callbook lookup ran. It used to stay unless
// it equalled the CURRENT hunt: with the hunt moved on and no callbook, it went out on the typed
// station's contact as a park the operator had typed. A park the operator typed is theirs and never
// leaves this way, so finishing a half-typed call keeps it.
describe('a hunted park leaves with its station', () => {
  const callBox = () => document.querySelector('input.le-call') as HTMLInputElement
  const loggedPark = () => (mockedLog.mock.calls[0][0] as { call: string; ota?: { theirRef?: string } })

  it('drops it when the operator types another call after the hunt moved to a third station', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    // The pending hunt moves to another activator without the call changing (a hunt set from the
    // map, the pop-out or another window): KE7G's park is still right for KE7G.
    view.rerender(strip(VA4ADM, { call: 'KE7G', ts: 1 }))
    await settle()
    expect(park().value, 'fixture: the park still belongs to the call in the box').toBe('US-3216')
    fireEvent.change(callBox(), { target: { value: 'w1xyz' } })
    await settle()
    // Soft, so a red run also reaches the logged record below: the box and the log are both claims.
    expect.soft(park().value, 'KE7G’s park stayed on W1XYZ').toBe('')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(loggedPark().call).toBe('W1XYZ')
    expect(loggedPark().ota?.theirRef, 'W1XYZ was logged at KE7G’s park').toBeUndefined()
  })

  it('drops it when the hunt is gone', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    // The pend expired, or another strip logged KE7G and consumed it.
    view.rerender(strip(null, { call: 'KE7G', ts: 1 }))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'w1xyz' } })
    await settle()
    // Soft, so a red run also reaches the logged record below: the box and the log are both claims.
    expect.soft(park().value, 'KE7G’s park stayed on W1XYZ').toBe('')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(loggedPark().ota?.theirRef, 'W1XYZ was logged at KE7G’s park').toBeUndefined()
  })

  // THE CONTROL: this one always worked, because the park equalled the pending hunt.
  it('drops it with KE7G’s own hunt still pending', async () => {
    render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    fireEvent.change(callBox(), { target: { value: 'w1xyz' } })
    await settle()
    expect(park().value).toBe('')
  })

  // With the hunt gone, the call is compared with the station the park is bound to, not with a
  // hunt, so this is the case that proves the comparison is the base call's.
  it('keeps it when the same station’s call only gains a /P, with the hunt gone', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    view.rerender(strip(null, { call: 'KE7G', ts: 1 }))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'ke7g/p' } })
    await settle()
    expect(park().value, 'the same station under a /P lost its park').toBe('US-3216')
    fireEvent.change(callBox(), { target: { value: 'w1xyz' } })
    await settle()
    expect(park().value).toBe('')
  })

  // THE OTHER CONTROL: a park the operator typed is theirs, and a call finished mid-edit keeps it.
  it('keeps a park the operator typed while the call is still being typed', async () => {
    render(strip(KE7G, null))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'w1xy' } })
    await settle()
    fireEvent.change(park(), { target: { value: 'us-1234' } })
    fireEvent.change(callBox(), { target: { value: 'w1xyz' } })
    await settle()
    expect(park().value, 'the typed park was dropped when the call was finished').toBe('US-1234')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(loggedPark().ota?.theirRef).toBe('US-1234')
  })

  it('follows the same activator to the new park it is spotted at', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    // KE7G re-spotted at another park, and clicked: the box follows the click. The earlier
    // prefill is the machine's, not an override, and left in place it went out as a typed park.
    view.rerender(strip({ ...KE7G, reference: 'US-3217' }, { call: 'KE7G', ts: 2 }))
    await settle()
    expect(park().value, 'the earlier prefill held the box against the new spot').toBe('US-3217')
  })
})

// THE BOX IS READ AS IT STANDS, NOT AS IT WAS RENDERED (#383, the case the two fixes above left).
// When the call in the box moves off a station the callbook answered for, the call-change effect
// empties the park box, and the prefill runs in the same commit. It judged "the operator's own
// park, keep it" from the box as RENDERED, a value that commit was erasing: the chip named the
// park and the box stayed empty, the symptom of the report. A park typed before any call
// reaches it, because such a park counts as the operator's for whichever call follows.
describe('the prefill sees the park box the call change has just emptied', () => {
  const callBox = () => document.querySelector('input.le-call') as HTMLInputElement

  it('fills the hunted park when the previous station’s park is cleared in the same render', async () => {
    callbookKnowsThem()
    const view = render(strip(null, null))
    await settle()
    // A park typed with no call in the box (looked up by name, say), then a call the callbook
    // knows: the enrichment is what arms the call-change clear.
    fireEvent.change(park(), { target: { value: 'us-1111' } })
    fireEvent.change(callBox(), { target: { value: 'w1abc' } })
    await waitFor(() => expect(screen.getByDisplayValue('Adam')).toBeTruthy())
    expect(park().value, 'fixture: the typed park is still in the box').toBe('US-1111')

    // KE7G's spot is clicked: the hunt and the call arrive as App sends them.
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 2 }))
    await waitFor(() => expect(callBox().value).toBe('KE7G'))
    await settle()
    expect(park().value, 'the hunt chip names US-3216 and the box is empty').toBe('US-3216')
  })
})

// A HUNTED PARK LEAVES WITH ITS HUNT WHEN NO CALL HOLDS IT (#383). Logging the hunted contact spends
// the hunt in the engine, but the strip's snapshot names it until the next poll, so the reset
// strip (call empty) prefilled the park again. When the poll brought the hunt's end, nothing took
// the park out: an empty strip showed the last contact's park, the chip gone.
describe('the park box after the hunted contact is logged', () => {
  const callBox = () => document.querySelector('input.le-call') as HTMLInputElement

  it('empties once the hunt that filled it is gone', async () => {
    const view = render(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(park().value).toBe('US-3216'))
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    await settle()
    expect(callBox().value, 'fixture: the strip reset after the log').toBe('')
    // The next snapshot poll: the engine spent the hunt on that contact.
    view.rerender(strip(null, { call: 'KE7G', ts: 1 }))
    await settle()
    expect(park().value, 'KE7G’s park stayed in the empty strip after its contact was logged').toBe('')
  })

  // THE CONTROL: a park the operator typed into the empty strip is theirs, and stays when the hunt
  // it replaced ends.
  it('keeps a park the operator typed into the empty strip when the hunt ends', async () => {
    const view = render(strip(KE7G, null))
    await waitFor(() => expect(park().value, 'fixture: the hunt prefilled the empty strip').toBe('US-3216'))
    fireEvent.change(park(), { target: { value: 'us-4444' } })
    view.rerender(strip(null, null))
    await settle()
    expect(park().value, 'the typed park went with the hunt').toBe('US-4444')
  })
})

// #383, THE OPERATOR'S RULING (2026-09-29): "a park you type before any call binds to the first call
// you TYPE, so a clicked spot for another park counts as a new station." It used to count as the
// operator's park for WHATEVER call came next, so with no callbook a clicked hunted spot kept the
// typed park, and that contact went out at it. The two parks always disagree here: the one typed,
// US-1111, and the clicked spot's, US-3216.
describe('a park typed before any call belongs to the first call typed after it', () => {
  const callBox = () => document.querySelector('input.le-call') as HTMLInputElement
  /** Type a call as the operator does, one keystroke at a time. */
  const typeCall = (call: string) => {
    for (let i = 1; i <= call.length; i++) fireEvent.change(callBox(), { target: { value: call.slice(0, i) } })
  }
  const logged = () => mockedLog.mock.calls[0][0] as { call: string; ota?: { theirRef?: string } }

  it.each([
    ['no callbook', false],
    ['a callbook', true],
  ])('a hunted spot clicked before any call is typed is a new station and fills its own park (%s)', async (_what, withCallbook) => {
    if (withCallbook) callbookKnowsThem()
    const view = render(strip(null, null))
    await settle()
    fireEvent.change(park(), { target: { value: 'us-1111' } })
    // KE7G's spot is clicked: the hunt and the call arrive as App sends them.
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(callBox().value).toBe('KE7G'))
    await settle()
    // Soft, so a red run also reaches the logged record below.
    expect.soft(park().value, 'the park typed for no one held against the clicked spot').toBe('US-3216')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(logged().call).toBe('KE7G')
    // The box equals the pending hunt, so the engine's own callsign-matched tag supplies US-3216.
    expect(logged().ota?.theirRef, 'KE7G was logged at the park typed for no one').toBeUndefined()
    // …and the strip that follows is empty, and stays so when the spent hunt goes (the N13 fix).
    expect(callBox().value).toBe('')
    view.rerender(strip(null, { call: 'KE7G', ts: 1 }))
    await settle()
    expect(park().value, 'the logged contact’s park came back into the empty strip').toBe('')
  })

  it('a spot for another station clicked after the call was typed fills its own park, with no callbook', async () => {
    const view = render(strip(null, null))
    await settle()
    fireEvent.change(park(), { target: { value: 'us-1111' } })
    typeCall('w1abc')
    await settle()
    expect(park().value, 'fixture: the typed park stays while its call is typed').toBe('US-1111')
    // No callbook, so nothing clears the box when the call changes: only the binding can say US-1111
    // was W1ABC's, not KE7G's.
    view.rerender(strip(KE7G, { call: 'KE7G', ts: 1 }))
    await waitFor(() => expect(callBox().value).toBe('KE7G'))
    await settle()
    expect(park().value, 'W1ABC’s park held against KE7G’s spot').toBe('US-3216')
  })

  // THE CONTROL: the park the operator typed stays theirs for the call they typed, against a spot for
  // that SAME station at another park. The binding follows the call as it is typed, W → W1XYZ.
  it('keeps the typed park for the call typed after it, against a spot for that call at another park', async () => {
    const view = render(strip(null, null))
    await settle()
    fireEvent.change(park(), { target: { value: 'us-1111' } })
    typeCall('w1xyz')
    await settle()
    view.rerender(strip({ program: 'POTA', reference: 'US-3216', call: 'W1XYZ' }, { call: 'W1XYZ', ts: 1 }))
    await settle()
    expect.soft(park().value, 'the park typed for W1XYZ was overwritten by a spot for W1XYZ').toBe('US-1111')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(logged().ota?.theirRef).toBe('US-1111')
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

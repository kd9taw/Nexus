// @vitest-environment jsdom
//
// THE LOG STRIP COMES BACK WITH THE CONTACT THE OPERATOR LEFT IN IT (`draft`, features/logDraft).
//
// A user, waiting to work a DX station, switched between pages and came back to the Voice cockpit
// to find the call gone: App draws the Phone and CW cockpits in its view switch, so a page switch
// unmounts them, and the strip held every field in its own state. Unmounting the strip and mounting
// it again is exactly what a page switch does to it, so that is what these cases do (the real App,
// by the navigation, is App.keepEntry.test.tsx).
//
// Each case asserts on the values the operator sees, and the cases after the first pin what must
// NOT come back: a contact logged, cleared, or still being logged; another cockpit's; a decoder's
// guess; a contest strip's; one with the other-radio override open. The last group pins what comes
// back WITH the contact, so the strip behaves as the one that was left: the callbook's answer stays
// bound to its call and reaches the record (grid, state and its source, country), and a park keeps
// whose it is (typed by the operator, or handed over by a Needed click).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { LogEntry } from './LogEntry'
import { logQso, qrzLookup, searchParks } from '../api'
import type { AppSnapshot, FieldDayStatus } from '../types'

// THE BUDGET: the slowest cases wait out the callbook's 700 ms debounce twice; a loaded full suite
// runs cases many times slower than one core. 15 s is the house budget.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (LogEntry.huntPrefill's pattern); the overrides are
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
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedLog = vi.mocked(logQso)
const mockedSearch = vi.mocked(searchParks)

type Hunt = { program: string; reference: string; call: string }
const snapWith = (hunt: Hunt | null = null) =>
  ({ mycall: 'KD9TAW', mygrid: 'EN52', radio: { band: '20m', dialMhz: 14.25 }, hunt }) as unknown as AppSnapshot

const fieldDay = {
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: true,
  state: '',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
} as unknown as FieldDayStatus

type StripProps = Partial<ComponentProps<typeof LogEntry>>
/** The Phone cockpit's strip, as PhoneCockpit renders it. */
const strip = (props: StripProps = {}) => (
  <LogEntry snap={snapWith()} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} draft="phone" {...props} />
)
/** A page switch and back: the strip goes, and its cockpit draws a new one. */
function comeBack(view: ReturnType<typeof render>, props: StripProps = {}) {
  view.unmount()
  return render(strip(props))
}

const input = (cls: string) => document.querySelector<HTMLInputElement>(`input.${cls}`)!
const call = () => input('le-call')
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const logButton = () => screen.getByRole('button', { name: /^log$/i })
const clearButton = () => screen.getByRole('button', { name: 'Clear' })
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

/** W1AW, as a callbook that knows it answers. */
const HIRAM = {
  call: 'W1AW',
  name: 'Hiram',
  qth: 'Newington',
  state: 'CT',
  country: 'United States',
  grid: 'FN31PR',
  image: 'https://example.invalid/w1aw.jpg',
}

beforeEach(() => {
  mockedQrz.mockReset().mockImplementation(async () => null as never)
  mockedLog.mockReset().mockImplementation(async (rec: unknown) => rec as never)
  mockedSearch.mockReset().mockImplementation(async () => [])
})

describe('the contact comes back with the strip', () => {
  it('the call, the reports, the name, the QTH, the comment and the notes', async () => {
    const view = render(strip())
    type(call(), 'EA8XX')
    const [sent, rcvd] = document.querySelectorAll<HTMLInputElement>('input.le-rst')
    type(sent, '57')
    type(rcvd, '55')
    type(input('le-name'), 'Pedro')
    type(input('le-qth'), 'Las Palmas')
    fireEvent.change(document.querySelector('textarea.le-comment')!, { target: { value: 'first QSO' } })
    fireEvent.change(document.querySelector('textarea.le-notes')!, { target: { value: 'heard on 20m' } })
    comeBack(view)
    expect(call().value, 'the call did not come back').toBe('EA8XX')
    const [sent2, rcvd2] = document.querySelectorAll<HTMLInputElement>('input.le-rst')
    expect([sent2.value, rcvd2.value]).toEqual(['57', '55'])
    expect(input('le-name').value).toBe('Pedro')
    expect(input('le-qth').value).toBe('Las Palmas')
    expect(document.querySelector<HTMLTextAreaElement>('textarea.le-comment')!.value).toBe('first QSO')
    expect(document.querySelector<HTMLTextAreaElement>('textarea.le-notes')!.value).toBe('heard on 20m')
  })

  it('and again after a second page switch', async () => {
    let view = render(strip())
    type(call(), 'EA8XX')
    view = comeBack(view)
    await settle()
    comeBack(view)
    expect(call().value, 'the contact came back once and was lost the second time').toBe('EA8XX')
  })

  it('a contact clicked to work comes back too', async () => {
    const view = render(strip({ pendingWork: { call: 'EA8XX', ts: 1 } }))
    await waitFor(() => expect(call().value).toBe('EA8XX'))
    comeBack(view)
    expect(call().value).toBe('EA8XX')
  })
})

describe('what ends the draft', () => {
  it('logged: the next visit starts empty', async () => {
    const view = render(strip())
    type(call(), 'EA8XX')
    type(input('le-name'), 'Pedro')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(call().value).toBe(''))
    comeBack(view)
    expect(call().value, 'a logged contact came back').toBe('')
    expect(input('le-name').value).toBe('')
  })

  it('cleared: the next visit starts empty', async () => {
    const view = render(strip())
    type(call(), 'EA8XX')
    type(input('le-name'), 'Pedro')
    fireEvent.click(clearButton())
    comeBack(view)
    expect(call().value, 'a cleared contact came back').toBe('')
    expect(input('le-name').value).toBe('')
  })

  // A log waits on the station, which can be busy (a radio read, a log save), and the operator can
  // be on another page and back before it answers. The contact must not come back meanwhile, for a
  // second Log to log it twice: neither the draft the strip held as Log was pressed (the first case)
  // nor one a render made while the log was out (the second, a snapshot poll's).
  it('still being logged when the strip goes: it never comes back', async () => {
    let land!: (v: unknown) => void
    mockedLog.mockImplementationOnce(() => new Promise((r) => { land = r }) as never)
    const view = render(strip())
    type(call(), 'EA8XX')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    const back = comeBack(view)
    expect(call().value, 'the contact being logged came back into the strip').toBe('')
    await act(async () => land({}))
    back.unmount()
    render(strip())
    expect(call().value).toBe('')
  })

  it('nor after a snapshot renders the strip while its log is out', async () => {
    let land!: (v: unknown) => void
    mockedLog.mockImplementationOnce(() => new Promise((r) => { land = r }) as never)
    const view = render(strip())
    type(call(), 'EA8XX')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    view.rerender(strip({ snap: snapWith() }))
    comeBack(view)
    expect(call().value, 'a render during the log kept the contact being logged').toBe('')
    await act(async () => land({}))
  })

  it('a log that fails keeps the contact: it is still on screen, and kept again', async () => {
    mockedLog.mockImplementationOnce(async () => { throw new Error('disk full') })
    const view = render(strip())
    type(call(), 'EA8XX')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    await settle()
    expect(call().value).toBe('EA8XX')
    // The next snapshot poll renders the strip, and the draft holds the contact again.
    view.rerender(strip({ snap: snapWith() }))
    comeBack(view)
    expect(call().value).toBe('EA8XX')
  })
})

describe('whose draft it is', () => {
  it('Phone’s contact never shows in CW, and is still Phone’s after it', async () => {
    let view = render(strip())
    type(call(), 'EA8XX')
    view.unmount()
    view = render(strip({ draft: 'cw', mode: 'CW', defaultRst: '599' }))
    expect(call().value, 'the Phone contact showed up in CW').toBe('')
    view.unmount()
    render(strip())
    expect(call().value).toBe('EA8XX')
  })

  it('a decoder’s unconfirmed guess is not kept; a confirmed call is', async () => {
    const cw = { draft: 'cw', mode: 'CW', defaultRst: '599' }
    let view = render(strip({ ...cw, cwLive: { call: 'K9ABC', rst: null, name: null, confirmed: false } }))
    await waitFor(() => expect(call().value).toBe('K9ABC'))
    view = comeBack(view, cw)
    expect(call().value, 'the decoder’s guess came back as the operator’s call').toBe('')
    view.unmount()
    view = render(strip({ ...cw, cwLive: { call: 'K9ABC', rst: null, name: null, confirmed: true } }))
    await waitFor(() => expect(call().value).toBe('K9ABC'))
    comeBack(view, cw)
    expect(call().value).toBe('K9ABC')
  })

  it('a contest strip keeps nothing, not even a station clicked to work: its contact is the session’s', async () => {
    // A click-to-work makes the call the operator's (settled), so this is the contest guard alone.
    const view = render(strip({ fieldDay, fdMode: 'PH', pendingWork: { call: 'EA8XX', ts: 1 } }))
    await waitFor(() => expect((screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement).value).toBe('EA8XX'))
    view.unmount()
    render(strip())
    expect(call().value, 'a contest strip’s call came back into the ordinary strip').toBe('')
  })

  it('and nothing kept before the contest comes back into it', async () => {
    const view = render(strip())
    type(call(), 'EA8XX')
    view.unmount()
    render(strip({ fieldDay, fdMode: 'PH' }))
    expect((screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement).value).toBe('')
  })

  it('with the other-radio override open nothing is kept; closed again, the contact is', async () => {
    let view = render(strip())
    type(call(), 'EA8XX')
    const toggle = () => document.querySelector<HTMLButtonElement>('.le-override-toggle')!
    fireEvent.click(toggle())
    view = comeBack(view)
    expect(call().value, 'a contact for another radio came back without its band, frequency and mode').toBe('')
    type(call(), 'EA8XX')
    fireEvent.click(toggle())
    fireEvent.click(toggle())
    comeBack(view)
    expect(call().value).toBe('EA8XX')
  })
})

describe('what comes back with the contact', () => {
  it('the callbook is not asked again for a call it already answered', async () => {
    mockedQrz.mockImplementation(async (c: string) => (c === 'W1AW' ? HIRAM : null) as never)
    const view = render(strip())
    type(call(), 'W1AW')
    await waitFor(() => expect(input('le-name').value).toBe('Hiram'), { timeout: 3000 })
    comeBack(view)
    expect(input('le-name').value).toBe('Hiram')
    expect(input('le-qth').value).toBe('Newington')
    await wait(900)
    expect(mockedQrz, 'the callbook was asked again on the way back').toHaveBeenCalledTimes(1)
  })

  it('the callbook’s answer still leaves with its call', async () => {
    mockedQrz.mockImplementation(async (c: string) => (c === 'W1AW' ? HIRAM : null) as never)
    const view = render(strip())
    type(call(), 'W1AW')
    await waitFor(() => expect(input('le-name').value).toBe('Hiram'), { timeout: 3000 })
    comeBack(view)
    type(call(), 'K1ABC')
    await settle()
    expect(input('le-name').value, 'W1AW’s callbook name stayed on K1ABC').toBe('')
    expect(input('le-qth').value).toBe('')
  })

  it('the callbook’s answer reaches the record after the trip: grid, state and its source, country, photo', async () => {
    // The grid is in no box on a Phone or CW strip: only the callbook writes it, so a contact that
    // came back without it would log with no grid (the CW contacts that logged none, 2026-08-15).
    mockedQrz.mockImplementation(async (c: string) => (c === 'W1AW' ? HIRAM : null) as never)
    const view = render(strip())
    type(call(), 'W1AW')
    await waitFor(() => expect(input('le-state').value).toBe('CT'), { timeout: 3000 })
    comeBack(view)
    expect(input('le-country').value).toBe('United States')
    expect(document.querySelector(`img[src="${HIRAM.image}"]`), 'the callbook photo did not come back').not.toBeNull()
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    const [rec, source] = mockedLog.mock.calls[0] as unknown as [Record<string, unknown>, string]
    expect({ grid: rec.grid, state: rec.state, country: rec.country, name: rec.name, qth: rec.qth }).toEqual({
      grid: 'FN31PR',
      state: 'CT',
      country: 'United States',
      name: 'Hiram',
      qth: 'Newington',
    })
    expect(source, 'the callbook’s state went to the station as the operator’s').toBe('callbook')
  })

  it('a park the operator typed for a hunted station stays theirs', async () => {
    const hunt: Hunt = { program: 'POTA', reference: 'US-0001', call: 'W1AW' }
    const park = () => input('le-park-ref')
    const view = render(strip({ snap: snapWith(hunt) }))
    type(call(), 'W1AW')
    await waitFor(() => expect(park().value).toBe('US-0001'))
    type(park(), 'US-0002')
    comeBack(view, { snap: snapWith(hunt) })
    await settle()
    expect(park().value, 'the hunt’s park replaced the one the operator typed').toBe('US-0002')
  })

  it('a park a Needed click handed over stays with its station, over a hunt at another park', async () => {
    const hunt: Hunt = { program: 'POTA', reference: 'US-0001', call: 'W1AW' }
    const park = () => input('le-park-ref')
    const needed = { call: 'W1AW', ts: 1, park: { program: 'POTA', reference: 'US-0003' } }
    const view = render(strip({ snap: snapWith(hunt), pendingWork: needed }))
    await waitFor(() => expect(park().value).toBe('US-0003'))
    comeBack(view, { snap: snapWith(hunt) })
    await settle()
    expect(park().value, 'the pending hunt’s park replaced the one the Needed row handed over').toBe('US-0003')
    fireEvent.click(logButton())
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    const rec = mockedLog.mock.calls[0][0] as unknown as { ota?: { theirRef?: string } }
    expect(rec.ota?.theirRef).toBe('US-0003')
  })

  it('a park put back opens no suggestion list', async () => {
    mockedSearch.mockImplementation(async () => [{ reference: 'US-0002', name: 'Somewhere' }] as never)
    const view = render(strip())
    type(call(), 'W1AW')
    type(input('le-park-ref'), 'US-00')
    await waitFor(() => expect(mockedSearch).toHaveBeenCalledTimes(1))
    comeBack(view)
    await wait(400)
    expect(mockedSearch, 'the park box searched again on the way back').toHaveBeenCalledTimes(1)
    expect(document.querySelector('.le-park-suggest')).toBeNull()
  })
})

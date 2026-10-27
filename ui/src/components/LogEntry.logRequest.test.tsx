// @vitest-environment jsdom
//
// A HOST'S "LOG IT" (`logRequest`, JTTY's Log QSO prompt after a TU) IS THE STRIP'S OWN LOG, FOR
// THE STATION IT NAMES AND NO OTHER. The strip logs only when it holds that call and could log it
// (what disables its Log button refuses the request too), and only after the host's `claim` says
// this screen has the answer; a strip holding another call is left alone, an empty one is filled
// for the operator to check. The cockpit's side is tested against the real strip in
// `JttyCockpit.lookup.test.tsx`; this pins the refusals, Field Day's included.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'
import * as api from '../api'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const snap = { radio: { band: '20m', dialMhz: 14.09 }, hunt: null } as unknown as AppSnapshot

/** Field Day, nothing of the received exchange typed yet: its Log is disabled. */
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

type Outcome = { kind: string; held?: string }

function strip(fd: FieldDayStatus | null) {
  const outcomes: Outcome[] = []
  const onLogRequest = (_id: number, o: Outcome) => outcomes.push(o)
  const el = (req: { id: number; call: string; claim: () => Promise<boolean> } | null) => (
    <LogEntry
      snap={snap}
      mode="JTTY"
      defaultRst="599"
      exchange="terrestrial"
      fieldDay={fd}
      fdMode="DIG"
      fdSubmode="JTTY"
      logRequest={req}
      onLogRequest={onLogRequest}
    />
  )
  const r = render(el(null))
  return { outcomes, ask: (id: number, call: string, claim = vi.fn(async () => true)) => {
    r.rerender(el({ id, call, claim }))
    return claim
  } }
}

const flush = () => act(async () => {
  await Promise.resolve()
  await Promise.resolve()
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('a host’s log request logs only the station it names', () => {
  it('the strip holding that call: claimed, then logged as its own Log logs; refused by the claim, nothing', async () => {
    const s = strip(null)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1aw' } })
    const claim = s.ask(1, 'W1AW')
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    expect(claim).toHaveBeenCalledTimes(1)
    expect(vi.mocked(api.logQso).mock.calls[0][0]).toMatchObject({ call: 'W1AW', mode: 'JTTY' })
    expect(s.outcomes).toEqual([{ kind: 'logging' }])
    // Answered on another screen first: nothing.
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'K1ABC' } })
    s.ask(2, 'K1ABC', vi.fn(async () => false))
    await flush()
    expect(api.logQso).toHaveBeenCalledTimes(1)
    expect(s.outcomes[s.outcomes.length - 1]).toEqual({ kind: 'taken' })
  })

  it('another call in the strip: nothing claimed or logged, and the strip keeps it', async () => {
    const s = strip(null)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'K1ABC' } })
    const claim = s.ask(1, 'W1AW')
    await flush()
    expect(claim).not.toHaveBeenCalled()
    expect(api.logQso).not.toHaveBeenCalled()
    expect(s.outcomes).toEqual([{ kind: 'other-call', held: 'K1ABC' }])
    expect((screen.getByPlaceholderText('Call') as HTMLInputElement).value).toBe('K1ABC')
  })

  it('Field Day with the exchange not yet copied: the disabled Log refuses the request too', async () => {
    const s = strip(fieldDay)
    const call = screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
    fireEvent.change(call, { target: { value: 'W1AW' } })
    const log = document.querySelector('.le-fd-log-btn') as HTMLButtonElement
    expect(log.disabled, 'CONTROL: the strip’s own Log cannot log this yet').toBe(true)
    const claim = s.ask(1, 'W1AW')
    await flush()
    expect(s.outcomes).toEqual([{ kind: 'not-ready' }])
    expect(claim).not.toHaveBeenCalled()
    expect(api.contestLogManual).not.toHaveBeenCalled()
  })

  it('claimed, then refused by the log itself: the strip says it logged nothing, so the host can ask again', async () => {
    const s = strip(null)
    vi.mocked(api.logQso).mockRejectedValueOnce(new Error('the log refused it'))
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'W1AW' } })
    s.ask(1, 'W1AW')
    await waitFor(() => expect(s.outcomes).toEqual([{ kind: 'logging' }, { kind: 'refused' }]))
    expect((screen.getByPlaceholderText('Call') as HTMLInputElement).value, 'the contact is still on the strip').toBe('W1AW')
    // CONTROL: the same press, logged, reports nothing after logging.
    s.ask(2, 'W1AW')
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(2))
    await flush()
    expect(s.outcomes.slice(2)).toEqual([{ kind: 'logging' }])
  })

  it('Field Day: a contest log that refuses after the claim is reported the same way', async () => {
    const s = strip(fieldDay)
    vi.mocked(api.contestLogManual).mockRejectedValueOnce(new Error('the entry moved on'))
    fireEvent.change(screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'W1AW' } })
    fireEvent.change(screen.getByLabelText(/^Class/), { target: { value: '2A' } })
    fireEvent.change(screen.getByLabelText(/^Section/), { target: { value: 'EMA' } })
    expect((document.querySelector('.le-fd-log-btn') as HTMLButtonElement).disabled, 'CONTROL: ready to log').toBe(false)
    s.ask(1, 'W1AW')
    await waitFor(() => expect(s.outcomes).toEqual([{ kind: 'logging' }, { kind: 'refused' }]))
    expect(api.contestLogManual).toHaveBeenCalledTimes(1)
  })
})

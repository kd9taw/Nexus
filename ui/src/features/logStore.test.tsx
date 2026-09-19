// @vitest-environment jsdom
//
// THE WINDOW'S ONE COPY OF THE LOG (the big-log fix, 2026-09-19). At 150k contacts one full read
// is ~100 MB of JSON, and a window used to hold four or more private copies and re-pull one on
// nearly every contact. These pin the store's promises with bare readers, so each count is the
// store's own: one request however many readers start together; a logged contact moves one row
// into the SAME array every reader holds; anything but an append replaces the log once; a
// disabled reader (remote mode) asks for nothing; a failure never retries in a loop. The real
// consumers are pinned in components/sharedLog.test.tsx.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import type { LogDelta } from '../api'
import type { LoggedQso } from '../types'
import { loadSharedLog, refreshSharedLog, useSharedLog } from './logStore'

const api = vi.hoisted(() => ({ getLogDelta: vi.fn<(since: number, have: number) => Promise<LogDelta>>() }))
vi.mock('../api', () => ({ getLogDelta: api.getLogDelta }))

const qso = (call: string) =>
  ({ call, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1_700_000_000, grid: null }) as unknown as LoggedQso

/** What each reader last rendered, by id — to prove they hold the SAME array, not copies. */
const seen = new Map<string, LoggedQso[] | null>()
function Reader({ id, tick, enabled = true }: { id: string; tick?: number; enabled?: boolean }) {
  const log = useSharedLog(tick, enabled)
  seen.set(id, log)
  return <div data-testid={id}>{log === null ? 'none' : log.map((q) => q.call).join(',')}</div>
}
const text = (id: string) => screen.getByTestId(id).textContent
/** Three readers following the tick, as the three hidden log strips do. */
const Strips = ({ tick }: { tick: number }) => (
  <>
    <Reader id="a" tick={tick} />
    <Reader id="b" tick={tick} />
    <Reader id="c" tick={tick} />
  </>
)
const settle = () => act(() => new Promise((r) => setTimeout(r, 50)))

afterEach(() => {
  cleanup()
  api.getLogDelta.mockReset()
  seen.clear()
})

describe('one copy per window', () => {
  it('readers that mount together share ONE request, and it asks for the whole log', async () => {
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows: [qso('W1AW'), qso('K1ABC')] })
    render(
      <>
        <Strips tick={5} />
        {/* A reader with no tick (Awards, Statistics) starting in the same commit. */}
        <Reader id="d" />
      </>,
    )
    await waitFor(() => expect(text('d')).toBe('W1AW,K1ABC'))
    for (const id of ['a', 'b', 'c']) expect(text(id)).toBe('W1AW,K1ABC')
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
    expect(api.getLogDelta).toHaveBeenCalledWith(0, 0)
    // …and every reader holds the one array, not a copy of it.
    expect(seen.get('b')).toBe(seen.get('a'))
    expect(seen.get('d')).toBe(seen.get('a'))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
  })

  it('a logged contact moves ONE row: one delta, and every reader sees it', async () => {
    const first = [qso('W1AW'), qso('K1ABC')]
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: first })
    const { rerender } = render(<Strips tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW,K1ABC'))

    api.getLogDelta.mockResolvedValueOnce({ revision: 8, full: false, rows: [qso('N2XYZ')] })
    rerender(<Strips tick={6} />)
    await waitFor(() => expect(text('a')).toBe('W1AW,K1ABC,N2XYZ'))
    expect(text('b')).toBe('W1AW,K1ABC,N2XYZ')
    expect(text('c')).toBe('W1AW,K1ABC,N2XYZ')
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
    // The copy's revision and length went back: the engine was asked for what is new, not the log.
    expect(api.getLogDelta).toHaveBeenLastCalledWith(7, 2)
    // The rows already held are the same objects — appended to, not reloaded — in one shared array.
    expect(seen.get('a')![0]).toBe(first[0])
    expect(seen.get('c')).toBe(seen.get('a'))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
  })

  it('anything but an append replaces the log once, for every reader', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW'), qso('K1ABC')] })
    const { rerender } = render(<Strips tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW,K1ABC'))

    // W1AW deleted (by a Remote browser, say): the engine answers with the whole log.
    const whole = [qso('K1ABC')]
    api.getLogDelta.mockResolvedValueOnce({ revision: 9, full: true, rows: whole })
    rerender(<Strips tick={6} />)
    await waitFor(() => expect(text('a')).toBe('K1ABC'))
    expect(text('b')).toBe('K1ABC')
    expect(text('c')).toBe('K1ABC')
    expect(seen.get('a')).toBe(whole)
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)

    // The replacement's revision is the one handed back next time.
    api.getLogDelta.mockResolvedValueOnce({ revision: 10, full: false, rows: [] })
    rerender(<Strips tick={7} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(3))
    expect(api.getLogDelta).toHaveBeenLastCalledWith(9, 1)
  })

  it('a disabled reader — remote mode — asks for nothing and sees nothing', async () => {
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows: [qso('W1AW')] })
    render(
      <>
        <Reader id="a" tick={5} enabled={false} />
        <Reader id="b" enabled={false} />
      </>,
    )
    await settle()
    expect(api.getLogDelta).not.toHaveBeenCalled()
    expect(text('a')).toBe('none')
    expect(text('b')).toBe('none')
  })
})

describe('what is asked, and when', () => {
  it('the same tick again asks for nothing; a burst of ticks in one turn asks once', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW')] })
    const { rerender } = render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW'))
    rerender(<Reader id="a" tick={5} />)
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)

    api.getLogDelta.mockResolvedValue({ revision: 10, full: false, rows: [] })
    rerender(<Reader id="a" tick={6} />)
    rerender(<Reader id="a" tick={7} />)
    rerender(<Reader id="a" tick={8} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(2))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
  })

  it('ticks that land while a request is out are answered by ONE follow-up', async () => {
    let release!: (d: LogDelta) => void
    api.getLogDelta.mockImplementationOnce(() => new Promise((r) => (release = r)))
    const { rerender } = render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(1))
    // Three snapshots while the first read is still out — it may predate all of them.
    for (const tick of [6, 7, 8]) {
      rerender(<Reader id="a" tick={tick} />)
      await settle()
    }
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)

    api.getLogDelta.mockResolvedValueOnce({ revision: 9, full: false, rows: [qso('N2XYZ')] })
    await act(async () => release({ revision: 8, full: true, rows: [qso('W1AW')] }))
    await waitFor(() => expect(text('a')).toBe('W1AW,N2XYZ'))
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
    expect(api.getLogDelta).toHaveBeenLastCalledWith(8, 1)
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
  })

  it('a reader at the current tick asks for nothing; one with no tick asks for what is new', async () => {
    const first = [qso('W1AW')]
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: first })
    const { rerender } = render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW'))

    rerender(
      <>
        <Reader id="a" tick={5} />
        <Reader id="b" tick={5} />
      </>,
    )
    await settle()
    expect(text('b')).toBe('W1AW')
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)

    // A view with no snapshot cannot tell a current copy from an old one, so it asks — for what
    // is new since the copy, which here is nothing, and the copy stands as it was.
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: false, rows: [] })
    rerender(
      <>
        <Reader id="a" tick={5} />
        <Reader id="b" tick={5} />
        <Reader id="c" />
      </>,
    )
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(2))
    expect(api.getLogDelta).toHaveBeenLastCalledWith(7, 1)
    await settle()
    expect(text('c')).toBe('W1AW')
    expect(seen.get('a')).toBe(first)
  })

  it('a failed read waits for the next change instead of retrying in a loop', async () => {
    api.getLogDelta.mockRejectedValueOnce(new Error('engine busy'))
    const { rerender } = render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(1))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
    expect(text('a')).toBe('none')

    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW')] })
    rerender(<Reader id="a" tick={6} />)
    await waitFor(() => expect(text('a')).toBe('W1AW'))
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
    expect(api.getLogDelta).toHaveBeenLastCalledWith(0, 0)
  })

  it('an answer without rows is refused, and the copy every reader holds stands', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW')] })
    const { rerender } = render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW'))

    api.getLogDelta.mockResolvedValueOnce({} as LogDelta)
    rerender(<Reader id="a" tick={6} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(2))
    await settle()
    expect(text('a')).toBe('W1AW')

    api.getLogDelta.mockResolvedValueOnce({ revision: 8, full: false, rows: [qso('N2XYZ')] })
    rerender(<Reader id="a" tick={7} />)
    await waitFor(() => expect(text('a')).toBe('W1AW,N2XYZ'))
    expect(api.getLogDelta).toHaveBeenLastCalledWith(7, 1)
  })
})

describe('callers outside React', () => {
  it('loadSharedLog joins the read already out; refreshSharedLog after a write asks again once it lands', async () => {
    let release!: (d: LogDelta) => void
    api.getLogDelta.mockImplementationOnce(() => new Promise((r) => (release = r)))
    render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(api.getLogDelta).toHaveBeenCalledTimes(1))

    const loaded = loadSharedLog()
    // The caller's own write landed while the first read was out — which may predate it.
    refreshSharedLog()
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)

    api.getLogDelta.mockResolvedValueOnce({ revision: 8, full: false, rows: [qso('N2XYZ')] })
    let got: LoggedQso[] = []
    await act(async () => {
      release({ revision: 7, full: true, rows: [qso('W1AW')] })
      got = await loaded
    })
    expect(got.map((q) => q.call)).toEqual(['W1AW', 'N2XYZ'])
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
    expect(api.getLogDelta).toHaveBeenLastCalledWith(7, 1)
    expect(text('a')).toBe('W1AW,N2XYZ')
  })

  it('loadSharedLog rejects when there is no copy at all to give', async () => {
    api.getLogDelta.mockRejectedValueOnce(new Error('engine busy'))
    await expect(loadSharedLog()).rejects.toThrow('engine busy')
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
  })
})

// src/test-setup.ts resets the store after every test. These two run in order: the first loads a
// log, and the second must not be answered by it.
describe('no copy survives a test', () => {
  it('(1 of 2) loads a log', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW')] })
    render(<Reader id="a" tick={5} />)
    await waitFor(() => expect(text('a')).toBe('W1AW'))
  })

  it('(2 of 2) starts from nothing: its first read asks for the whole log', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 3, full: true, rows: [qso('K1ABC')] })
    render(<Reader id="a" tick={5} />)
    expect(text('a')).toBe('none')
    await waitFor(() => expect(text('a')).toBe('K1ABC'))
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
    expect(api.getLogDelta).toHaveBeenCalledWith(0, 0)
  })
})

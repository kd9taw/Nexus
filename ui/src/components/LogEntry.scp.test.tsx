// @vitest-environment jsdom
//
// SUPER CHECK PARTIAL IN THE CONTEST STRIP, rendered with the props `PhoneCockpit` gives it and a
// `fieldDay` shaped as the engine serialises it. The station's three commands are fakes: no test
// here reaches the network, and "nothing is asked" is asserted as "not one of these was called".
//
// ⚠️ jsdom lays out nothing, so nothing here is about geometry: the line's fixed height is a
// stylesheet fact the browser probe sees, not this file.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 100, checkedAt: 100, nextCheckAt: 0, count: 4 })),
  getScpCalls: vi.fn(() => Promise.resolve(['K9AAA', 'W9XYZ', 'K9AB', 'K9ABC'])),
  getCallHistory: vi.fn(() => Promise.resolve(null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

/** A Field Day session with both aids effectively on, as the engine reports it. */
const fdStatus = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: '',
    event: 'arrlfd',
    receives: [
      { key: 'CLASS', kind: 'pattern', required: true },
      { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections', domains: ['fd_sections'] },
    ],
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
    ],
    assistanceOn: ['AI CW decoder', 'Super Check Partial', 'Call history'],
    ...over,
  }) as unknown as FieldDayStatus

const strip = (fieldDay: FieldDayStatus, remote?: Parameters<typeof LogEntry>[0]['remote']) => (
  <LogEntry
    snap={snap}
    mode="SSB"
    defaultRst="59"
    exchange="terrestrial"
    titled={false}
    onSpot={() => {}}
    pendingWork={null}
    onConsumeWork={() => {}}
    fieldDay={fieldDay}
    fdMode="PH"
    remote={remote}
  />
)

/** Let the mount's two station calls (check, then read) settle. */
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const shown = () => [...document.querySelectorAll('.le-scp button')].map((b) => b.textContent)

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('Super Check Partial in the contest strip', () => {
  it('shows the calls that contain what is typed, and leaves the Call box exactly as typed', async () => {
    render(strip(fdStatus()))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'k9a' } })
    expect(shown()).toEqual(['K9AAA', 'K9AB', 'K9ABC'])
    expect(callBox().value).toBe('K9A')
    // Nothing is asked of the station per keystroke: one check and one read, at mount.
    expect(api.scpEnsure).toHaveBeenCalledTimes(1)
    expect(api.scpEnsure).toHaveBeenCalledWith(false)
    expect(api.getScpCalls).toHaveBeenCalledTimes(1)
  })

  it('lists a call already in this log first, marked as worked', async () => {
    const log = [{ call: 'K9AB', class: '1D', section: 'IL', whenUnix: 1 }]
    render(strip(fdStatus({ log } as unknown as Partial<FieldDayStatus>)))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'K9A' } })
    expect(shown()).toEqual(['K9AB', 'K9AAA', 'K9ABC'])
    expect(document.querySelector('.le-scp button')!.className).toContain('le-scp-worked')
  })

  it('puts a clicked call in the Call box, and nothing goes there on its own', async () => {
    render(strip(fdStatus()))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'K9A' } })
    expect(callBox().value).toBe('K9A')
    fireEvent.click(screen.getByRole('button', { name: 'K9ABC' }))
    expect(callBox().value).toBe('K9ABC')
  })

  it('keeps its line while SCP is on, typed or not, and says so when there is no list', async () => {
    render(strip(fdStatus()))
    await settle()
    expect(document.querySelector('.le-scp')).not.toBeNull()
    expect(shown()).toEqual([])
    cleanup()
    api.scpEnsure.mockImplementationOnce(() =>
      Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 0 }),
    )
    render(strip(fdStatus()))
    await settle()
    expect(api.getScpCalls).toHaveBeenCalledTimes(1) // the first mount's read only
    expect(document.querySelector('.le-scp-empty')?.textContent).toContain('no list yet')
  })

  it('survives a failed download: no list, and the strip still logs', async () => {
    api.scpEnsure.mockImplementationOnce(() => Promise.reject(new Error('offline')))
    render(strip(fdStatus()))
    await settle()
    expect(document.querySelector('.le-scp-empty')).not.toBeNull()
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    const boxes = document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')
    fireEvent.change(boxes[0], { target: { value: '2A' } })
    fireEvent.change(boxes[1], { target: { value: 'IL' } })
    await act(async () => {
      fireEvent.click(document.querySelector('.le-fd-log-btn')!)
    })
    expect(api.contestLogManual).toHaveBeenCalledWith(
      'K9AAA',
      [
        ['CLASS', '2A'],
        ['SECTION', 'IL'],
      ],
      'PH',
      undefined,
    )
  })

  it('is off in Unassisted mode: no line, and nothing asked of the station', async () => {
    // The engine leaves the label out of `assistanceOn` while an unassisted entry is declared.
    const { rerender } = render(strip(fdStatus({ assistanceOn: ['PSK Reporter needs'] })))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'K9A' } })
    expect(document.querySelector('.le-scp')).toBeNull()
    expect(api.scpEnsure).not.toHaveBeenCalled()
    expect(api.getScpCalls).not.toHaveBeenCalled()
    // …and declaring it mid-entry takes the line away at the next snapshot.
    rerender(strip(fdStatus()))
    await settle()
    expect(shown()).toEqual(['K9AAA', 'K9AB', 'K9ABC'])
    rerender(strip(fdStatus({ assistanceOn: [] })))
    expect(document.querySelector('.le-scp')).toBeNull()
  })

  it('is off on Remote: the hosted page gets no Super Check Partial', async () => {
    const remote = {
      submit: () => Promise.resolve(),
      canSubmit: true,
      busy: false,
      resetKey: 0,
      recall: () => null,
    }
    render(strip(fdStatus(), remote))
    await settle()
    expect(document.querySelector('.le-scp')).toBeNull()
    expect(api.scpEnsure).not.toHaveBeenCalled()
  })
})

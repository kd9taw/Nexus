// @vitest-environment jsdom
//
// ⭐ REMOVE THE NEWEST CONTEST CONTACT FROM THE STRIP — Ctrl+D (or Remove last), PRESSED TWICE.
//
// Rendered with the props the cockpits give the strip, and a contest log of the shape the DTO
// produces. The press is a WINDOW key, so it is fired at `window`, as the browser delivers it.
//
//   - one press names the contact and changes nothing; a second within 5 s removes it, with the
//     identity that was shown;
//   - a held key (auto-repeat) is ONE press, and Alt, Shift or Cmd with D is not the key;
//   - any other key, the 5 s, or a new contact lets a press lapse; a contact that changed
//     between the presses is not removed, and the line says so;
//   - the caret never moves, and the only thing reached is the contest log;
//   - not from afar (the hosted Remote page and the native client), and not from a strip whose
//     cockpit is hidden.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'
import type { ContestRemovalAnswer } from '../api'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.025 }, hunt: null } as unknown as AppSnapshot

/** 2026-10-18 17:00Z and a minute later. */
const T0 = 1_792_342_800

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

/** An Illinois QSO Party session as the engine serialises it. */
const party = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

function strip(fieldDay: FieldDayStatus, over: { active?: boolean; remote?: boolean } = {}) {
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode="CW"
      active={over.active ?? true}
      // The hosted log adapter the CW and Phone cockpits pass when the station is driven from
      // afar (the hosted Remote page and the native client): the contest strip still renders.
      remote={
        over.remote
          ? { submit: vi.fn(), canSubmit: true, busy: false, pending: false, resetKey: 0, recall: () => null }
          : undefined
      }
    />
  )
}

/** Ctrl+D at the window, as the browser delivers it; answers whether the default was cancelled. */
const ctrlD = (extra: Partial<KeyboardEventInit> = {}) =>
  !fireEvent.keyDown(window, { key: 'd', code: 'KeyD', ctrlKey: true, ...extra })

const line = () => document.querySelector('.le-fd-remove-line')?.textContent ?? null
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const called = () =>
  Object.entries(api)
    .filter(([, f]) => f.mock.calls.length > 0)
    .map(([k]) => k)

const removed = (over: Partial<ContestRemovalAnswer> = {}): ContestRemovalAnswer =>
  ({
    outcome: 'removed',
    entry: { id: 2, removedUnix: T0 + 90, rows: [row('W9BBB', 'LAKE', T0 + 60)] },
    sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    ...over,
  }) as ContestRemovalAnswer

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  for (const f of Object.values(api)) f.mockClear()
})

const LOG = [row('K9AAA', 'COOK', T0), row('W9BBB', 'LAKE', T0 + 60)]

describe('Ctrl+D pressed twice removes the newest contest contact', () => {
  it('names the contact on the first press and changes nothing', () => {
    render(strip(party(LOG)))
    expect(ctrlD(), 'Ctrl+D is the strip’s: the browser’s own is cancelled').toBe(true)
    expect(line()).toBe('Remove W9BBB · 20m CW · 17:01 · 599 LAKE? Press Ctrl+D again.')
    expect(called(), 'one press reaches nothing').toEqual([])
  })

  it('removes, on the second press, exactly the contact it named — and says where it went', async () => {
    api.contestRemoveLast.mockResolvedValueOnce(
      removed({
        sentTo: { n3fjp: '192.168.1.20', n1mm: '192.168.1.255:12060', wsjtx: true, logbook: true, uploaded: ['qrz', 'clublog'] },
      } as never),
    )
    render(strip(party(LOG)))
    ctrlD()
    await act(async () => {
      ctrlD()
    })
    expect(api.contestRemoveLast).toHaveBeenCalledTimes(1)
    expect(api.contestRemoveLast).toHaveBeenCalledWith('W9BBB', T0 + 60)
    expect(line()).toBe(
      'Removed W9BBB · 20m CW · 17:01 · 599 LAKE from the contest log. Restore it on the contest screen. ' +
        'It was already sent to N3FJP (192.168.1.20), the N1MM broadcast, WSJT-X listeners: delete it there too. ' +
        'It stays in your logbook, and QRZ, Club Log have it: Nexus cannot take it back from them.',
    )
    expect(called(), 'Ctrl+D reaches the contest log and nothing else — never TX').toEqual(['contestRemoveLast'])
  })

  it('claims nowhere for a contact that never left', async () => {
    api.contestRemoveLast.mockResolvedValueOnce(removed())
    render(strip(party(LOG)))
    ctrlD()
    await act(async () => {
      ctrlD()
    })
    expect(line()).toBe(
      'Removed W9BBB · 20m CW · 17:01 · 599 LAKE from the contest log. Restore it on the contest screen.',
    )
  })

  it('takes a held Ctrl+D as ONE press: auto-repeat never confirms', () => {
    render(strip(party(LOG)))
    ctrlD()
    for (let i = 0; i < 5; i++) expect(ctrlD({ repeat: true }), 'still the strip’s key').toBe(true)
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
    expect(line()).toContain('Press Ctrl+D again')
  })

  it('is not the key with Alt, Shift or Cmd', () => {
    render(strip(party(LOG)))
    for (const mod of [{ altKey: true }, { shiftKey: true }, { metaKey: true }]) {
      expect(ctrlD(mod), `${Object.keys(mod)[0]}: the browser keeps it`).toBe(false)
      expect(line()).toBeNull()
    }
  })

  it('lapses on any other key — and pressing Ctrl again is not one', () => {
    render(strip(party(LOG)))
    ctrlD()
    fireEvent.keyDown(window, { key: 'Control', ctrlKey: true })
    expect(line(), 'the second press starts with Ctrl: still waiting').toContain('Press Ctrl+D again')
    fireEvent.keyDown(window, { key: 'a' })
    expect(line(), 'another key lets it lapse').toBeNull()
    ctrlD()
    expect(api.contestRemoveLast, 'after a lapse the next press only names it again').not.toHaveBeenCalled()
    expect(line()).toContain('Press Ctrl+D again')
  })

  it('lapses after 5 seconds', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
    vi.setSystemTime(T0 * 1000)
    render(strip(party(LOG)))
    ctrlD()
    act(() => {
      vi.advanceTimersByTime(5_001)
    })
    expect(line()).toBeNull()
    ctrlD()
    expect(api.contestRemoveLast, 'the press after the window names it again').not.toHaveBeenCalled()
  })

  it('removes nothing when the newest contact changed between the presses, and says so', () => {
    const { rerender } = render(strip(party(LOG)))
    ctrlD()
    rerender(strip(party([...LOG, row('N9CCC', 'WILL', T0 + 120)])))
    expect(line()).toBe('The newest contact changed, so nothing was removed.')
    ctrlD()
    expect(api.contestRemoveLast, 'the next press names the NEW contact').not.toHaveBeenCalled()
    expect(line()).toContain('N9CCC')
  })

  it('names a county line as one contact', () => {
    render(strip(party([row('K9AAA', 'COOK', T0), row('K9NR', 'COOK', T0 + 60), row('K9NR', 'DUPG', T0 + 60)])))
    ctrlD()
    expect(line()).toBe('Remove K9NR · 20m CW · 17:01 · 599 COOK/DUPG? Press Ctrl+D again.')
  })

  it('says why, when club sync refuses it', async () => {
    api.contestRemoveLast.mockResolvedValueOnce({ outcome: 'refused', refusal: 'clubSync' })
    render(strip(party(LOG)))
    ctrlD()
    await act(async () => {
      ctrlD()
    })
    expect(line()).toBe(
      "Club sync is on, so this contact is already in the club log. Nothing was removed. Edit the club's Cabrillo file before you send it in.",
    )
  })

  it('has nothing to remove from an empty log', () => {
    render(strip(party([])))
    ctrlD()
    expect(line()).toBe('There is no contest contact to remove.')
    expect((screen.getByText('Remove last') as HTMLButtonElement).disabled).toBe(true)
  })
})

describe('the caret never moves', () => {
  it('stays in the call box through both presses and the button', async () => {
    api.contestRemoveLast.mockResolvedValue(removed())
    render(strip(party(LOG)))
    callBox().focus()
    expect(document.activeElement).toBe(callBox())
    ctrlD()
    await act(async () => {
      ctrlD()
    })
    expect(document.activeElement, 'Ctrl+D').toBe(callBox())
    const button = screen.getByText('Remove last')
    // A real click is a mousedown then a click: the mousedown's default (taking focus) is
    // cancelled, which is what keeps the caret where it was.
    expect(fireEvent.mouseDown(button), 'the click never takes focus').toBe(false)
    fireEvent.click(button)
    expect(document.activeElement, 'Remove last').toBe(callBox())
  })

  it('the button presses twice like the key', async () => {
    api.contestRemoveLast.mockResolvedValue(removed())
    render(strip(party(LOG)))
    const button = screen.getByText('Remove last')
    fireEvent.click(button)
    expect(button.getAttribute('aria-pressed')).toBe('true')
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(button)
    })
    expect(api.contestRemoveLast).toHaveBeenCalledWith('W9BBB', T0 + 60)
  })
})

describe('only where the strip shows, and never from afar', () => {
  it('does nothing from a strip whose cockpit is hidden', () => {
    render(strip(party(LOG), { active: false }))
    expect(ctrlD(), 'a hidden strip leaves the key alone').toBe(false)
    expect(line()).toBeNull()
  })

  it('offers nothing on the hosted Remote page or in the native client', () => {
    render(strip(party(LOG), { remote: true }))
    expect(screen.queryByText('Remove last')).toBeNull()
    expect(ctrlD()).toBe(false)
    expect(line()).toBeNull()
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
  })
})

// @vitest-environment jsdom
//
// A contest box's blur closes its type-ahead list a moment later, so that a click on the list is
// taken first. That timer belongs to the strip: an unmount inside its 150 ms clears it. Left
// running, it fires on a strip that is gone; in a test, after the page itself has gone, as
// "window is not defined", an unhandled error that fails the whole run though every test passed.
import { it, expect, vi, afterEach } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

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

const snap = { radio: { band: '20m', dialMhz: 14.025 }, hunt: null } as unknown as AppSnapshot

/** An Illinois QSO Party session as the engine serialises it, with nothing logged yet. */
const party = {
  running: true,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'ilqp',
  role: 'in_state',
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: '', domain: 'il_counties' },
  ],
} as unknown as FieldDayStatus

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

it("a box's closing timer goes with the strip", () => {
  vi.useFakeTimers()
  const { unmount, container } = render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={party}
      fdMode="CW"
      active
    />,
  )
  const boxes = Array.from(container.querySelectorAll<HTMLInputElement>('.le-fd-big input'))
  const qth = boxes[boxes.length - 1]
  expect(qth).toBeDefined()
  const before = vi.getTimerCount()
  fireEvent.blur(qth)
  // The control: the blur did schedule its close.
  expect(vi.getTimerCount()).toBe(before + 1)
  unmount()
  expect(vi.getTimerCount()).toBe(0)
})

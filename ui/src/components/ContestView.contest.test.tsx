// @vitest-environment jsdom
//
// Two things §9 and §18.1 owe the operator, rendered with the props App gives the view.
//
//  1. THE MULTIPLIER DISPLAY is one block per board the SESSION declares — not a
//     hardcoded sections grid. Field Day still gets its sections board, because Field
//     Day's session declares exactly one board over its one closed-value slot.
//
//  2. ⭐ THE CLUBLOG SWEEP LIMITATION IS ON SCREEN, BESIDE THE SWITCH IT QUALIFIES.
//     Batch 5 shipped `UPLOAD_CLUBLOG_SWEEP_HINT` as data with no renderer. ClubLog's
//     catch-up sweep re-queues every contact ClubLog never accepted the next time a
//     ClubLog password is saved — regardless of this switch. An operator who reads
//     "upload: off" and gets a ClubLog upload anyway has been misled by us. The hint
//     travels ON the control's own DTO for exactly this reason, and the test that
//     matters is not "the string exists" but "it is in the document, next to the
//     switch, whichever way the switch is set".
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup, fireEvent } from '@testing-library/react'
import { ContestView } from './ContestView'
import { fdSetUpload } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

/** The engine's own sentence, verbatim — `contest::UPLOAD_CLUBLOG_SWEEP_HINT`. It is a
 *  BACKEND string on purpose: a catalog copy could drift from the limitation the engine
 *  actually has, and then the screen would be reassuring about the wrong thing. */
const HINT =
  "Off means this session's contacts are not queued for upload when you merge them " +
  'into your logbook. One exception, and it is not ours to switch off: saving a ClubLog ' +
  'password re-queues every contact ClubLog has not accepted, including these.'

/** …and "verbatim" is CHECKED, not asserted in a comment. The hint is the one string
 *  on this screen with no catalog entry, precisely so the screen cannot promise
 *  something the engine does not do — which is worth nothing if the copy above drifts
 *  from the constant. Read the Rust and compare. */
it('the fixture above is the engine\'s constant, character for character', () => {
  // `import.meta.url` is an http URL under the jsdom environment, so resolve from the
  // vitest root (`ui/`) instead of from this module.
  const rs = readFileSync(
    resolve(process.cwd(), '../crates/tempo-core/src/contest/session.rs'),
    'utf8',
  )
  const m = /pub const UPLOAD_CLUBLOG_SWEEP_HINT: &str = "((?:[^"\\]|\\.)*)"/s.exec(rs)
  expect(m, 'the constant moved or was renamed').not.toBeNull()
  // Rust's `\`-newline continuation swallows the newline and the indent that follows.
  expect(m![1].replace(/\\\n\s*/g, '')).toBe(HINT)
})

const FD = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    myClass: '1A',
    mySection: 'IL',
    running: true,
    state: 'sp',
    qsoCount: 2,
    sections: 2,
    points: 3,
    workedSections: ['EMA', 'IL'],
    log: [],
    boards: [
      {
        id: 'SECTION',
        slot: 'SECTION',
        domain: 'fd_sections',
        scope: 'perLog',
        worked: ['EMA', 'IL'],
      },
    ],
    upload: {
      enabled: false,
      destinations: [],
      available: ['qrz', 'clublog', 'wrl'],
      hint: HINT,
    },
    ...over,
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => {
  cleanup()
  vi.mocked(fdSetUpload).mockClear()
})

describe('the multiplier display is the session’s boards', () => {
  it('renders one block per board, and Field Day’s is its sections board', async () => {
    render(<ContestView fieldDay={FD()} onSetMode={() => {}} />)
    await settle()
    const boards = screen.getAllByLabelText('Worked sections board')
    expect(boards).toHaveLength(1)
    // The worked set comes from the BOARD, not from a second derivation of the log.
    const worked = [...boards[0].querySelectorAll('span[aria-label$=", worked"]')].map(
      (n) => n.textContent,
    )
    expect(worked.sort()).toEqual(['✓EMA', '✓IL'])
  })

  it('renders a second block when the session declares a second board', async () => {
    render(
      <ContestView
        fieldDay={FD({
          boards: [
            {
              id: 'SECTION',
              slot: 'SECTION',
              domain: 'fd_sections',
              scope: 'perLog',
              worked: ['EMA'],
            },
            // A board over a domain this build carries no value set for: it shows what
            // was worked and claims no total, rather than inventing a universe.
            { id: 'county', slot: 'QTH', domain: 'tn_counties', scope: 'perBand', worked: ['DAV'] },
          ],
        })}
        onSetMode={() => {}}
      />,
    )
    await settle()
    expect(screen.getAllByLabelText('Worked sections board')).toHaveLength(1)
    const county = screen.getByLabelText('county multiplier board')
    expect(county.textContent).toContain('DAV')
    expect(county.textContent).toContain('1/1 worked')
  })
})

describe('⭐ the ClubLog sweep limitation renders beside the control', () => {
  it('is on screen with the switch OFF — the state that would otherwise mislead', async () => {
    render(<ContestView fieldDay={FD()} onSetMode={() => {}} />)
    await settle()
    const sw = screen.getByRole('switch', { name: "Upload this session's merged contacts" })
    expect(sw.getAttribute('aria-checked')).toBe('false')
    const hint = screen.getByText(HINT)
    // BESIDE, not merely present: the same block holds both, so a layout that dropped
    // the hint could not keep the switch.
    expect(hint.closest('.fd-upload')).not.toBeNull()
    expect(hint.closest('.fd-upload')!.contains(sw)).toBe(true)
  })

  it('is still on screen with the switch ON', async () => {
    render(
      <ContestView
        fieldDay={FD({
          upload: { enabled: true, destinations: ['wrl'], available: ['qrz', 'wrl'], hint: HINT },
        })}
        onSetMode={() => {}}
      />,
    )
    await settle()
    expect(screen.getByRole('switch', { name: "Upload this session's merged contacts" })
      .getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText(HINT)).not.toBeNull()
  })

  it('writes the switch and the destination through the per-session command', async () => {
    render(<ContestView fieldDay={FD()} onSetMode={() => {}} />)
    await settle()
    fireEvent.click(screen.getByRole('switch', { name: "Upload this session's merged contacts" }))
    await settle()
    expect(vi.mocked(fdSetUpload).mock.calls).toEqual([[true, []]])
    // A destination is the OTHER half — neither alone enqueues anything.
    vi.mocked(fdSetUpload).mockClear()
    fireEvent.click(screen.getByRole('checkbox', { name: 'clublog' }))
    await settle()
    expect(vi.mocked(fdSetUpload).mock.calls).toEqual([[false, ['clublog']]])
  })
})

// @vitest-environment jsdom
//
// THE SCORE CHIP — the running contest at a glance in every contest log strip, read BY VALUE.
//
// Every fixture below is built so that each number in the chip would come out DIFFERENT if
// it were read from the wrong field: `qsoCount` is not the log's length, `totalScore` is not
// `points`, `multCount` is not `sections`. A chip that summed its own score, or counted the
// log itself, fails here with a wrong number rather than passing with a plausible one.
//
// The last block renders the REAL Contest screen from the same session and reads its tiles:
// the chip and the screen must say the same thing, because they are meant to be one reading
// in two places, never two scoring paths.
//
// ⚠️ jsdom lays out nothing. That the chip never wraps the strip or changes its height is a
// measurement in headless Chrome (the report's table), not an assertion here.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { ContestScoreChip } from './ContestScoreChip'
import { ContestView } from './ContestView'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({})),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

/** The chip reads the real clock, so the logs are built backwards from it. */
const nowSecs = () => Math.floor(Date.now() / 1000)

const qso = (whenUnix: number, i: number): FieldDayQso =>
  ({ call: `K9A/${i}`, band: '20m', mode: 'CW', whenUnix }) as FieldDayQso

/** Ten contacts, the oldest `span` seconds ago and the newest now: the last-10 window's
 *  reading is 9 intervals over `span` seconds, per hour. */
const ten = (span: number): FieldDayQso[] => {
  const now = nowSecs()
  return Array.from({ length: 10 }, (_, i) => qso(now - span + Math.round((span * i) / 9), i))
}

const session = (over: Partial<FieldDayStatus>): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    workedSections: [],
    role: '',
    receives: [],
    composing: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    ...over,
  }) as unknown as FieldDayStatus

/** An Illinois QSO Party session: 790 s over the last ten contacts is 41.0 an hour. */
const ilqp = (over: Partial<FieldDayStatus> = {}) =>
  session({
    event: 'ilqp',
    role: 'in_state',
    qsoCount: 48,
    points: 106,
    totalScore: 212,
    multCount: 19,
    sections: 3,
    log: ten(790),
    ...over,
  })

const chipText = () => document.querySelector('.le-fd-score')?.textContent ?? null

afterEach(cleanup)

describe('the chip reads the session the Contest screen reads', () => {
  it('shows a QSO party as contest · points · Q · mults · rate', () => {
    render(<ContestScoreChip fieldDay={ilqp()} />)
    expect(chipText()).toBe('ILQP · 212 pts · 48 Q · 19 mults · 41/hr')
  })

  it('shows Field Day with its worked sections, the second tile Field Day has', () => {
    // No multipliers in the ruleset: the screen's second tile is Sections, and so is the chip's.
    render(
      <ContestScoreChip
        fieldDay={session({
          event: 'arrlfd',
          qsoCount: 60,
          points: 120,
          totalScore: 340,
          multCount: null,
          sections: 45,
          log: ten(623),
        })}
      />,
    )
    expect(chipText()).toBe('Field Day · 340 pts · 60 Q · 45 sections · 52/hr')
  })

  it('names Winter Field Day by its short name, as the screen header does', () => {
    render(
      <ContestScoreChip
        fieldDay={session({ event: 'wfd', qsoCount: 2, totalScore: 8, sections: 1, log: ten(790) })}
      />,
    )
    expect(chipText()).toBe('WFD · 8 pts · 2 Q · 1 section · 41/hr')
  })

  it('leaves the points out when the engine sent none, rather than working out a second score', () => {
    // A Remote view of an older station: its DTO has no `totalScore`. The screen falls back
    // to settings this strip does not hold, so a chip total could disagree with it.
    render(<ContestScoreChip fieldDay={ilqp({ totalScore: undefined })} />)
    expect(chipText()).toBe('ILQP · 48 Q · 19 mults · 41/hr')
  })

  it('says one of each in the singular, and shows the screen’s em dash when there is no rate yet', () => {
    // One contact is not a rate: the screen's tile reads "—", and so does the chip.
    render(
      <ContestScoreChip
        fieldDay={ilqp({ qsoCount: 1, totalScore: 1, multCount: 1, log: [qso(nowSecs() - 30, 0)] })}
      />,
    )
    expect(chipText()).toBe('ILQP · 1 pt · 1 Q · 1 mult · —/hr')
  })

  it('reads the same in a pane head, the face a framed cockpit uses', () => {
    render(<ContestScoreChip fieldDay={ilqp()} place="head" />)
    expect(document.querySelector('.contest-score-chip')?.textContent ?? null).toBe('ILQP · 212 pts · 48 Q · 19 mults · 41/hr')
    expect(document.querySelector('.le-fd-score')).toBeNull()
  })

  it('reads the last-ten window, the screen’s first rate tile, not the hundred behind it', () => {
    // Twelve contacts: two from three hours ago, then ten over the last 790 s. The last ten run at
    // 41 an hour; the window over all twelve reads about 3.6, so the two cannot be confused.
    const old = [qso(nowSecs() - 3 * 3600 - 60, 90), qso(nowSecs() - 3 * 3600, 91)]
    render(<ContestScoreChip fieldDay={ilqp({ log: [...old, ...ten(790)] })} />)
    expect(chipText()).toBe('ILQP · 212 pts · 48 Q · 19 mults · 41/hr')
  })

  it('carries the whole reading in its title, for when the row clips it', () => {
    render(<ContestScoreChip fieldDay={ilqp()} />)
    const chip = document.querySelector('.le-fd-score') as HTMLElement
    expect(chip.title).toContain('ILQP · 212 pts · 48 Q · 19 mults · 41/hr')
  })

  it('decays to now while nothing is logged, on the scoreboard’s five-second clock', () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] })
    try {
      vi.setSystemTime(new Date('2026-10-18T18:00:00Z'))
      render(<ContestScoreChip fieldDay={ilqp({ log: ten(790) })} />)
      expect(chipText()).toBe('ILQP · 212 pts · 48 Q · 19 mults · 41/hr')
      // Fifteen minutes later with no contact: 9 intervals over 790 + 900 s is 19.2 an hour.
      act(() => {
        vi.setSystemTime(new Date('2026-10-18T18:15:00Z'))
        vi.advanceTimersByTime(5000)
      })
      expect(chipText()).toBe('ILQP · 212 pts · 48 Q · 19 mults · 19/hr')
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('the chip and the Contest screen say the same thing', () => {
  const settle = async () => {
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
  }
  /** The number inside the screen's tile with this accessible name or label. */
  const tile = (label: string) =>
    [...document.querySelectorAll('.fd-scoreboard .fd-score')].find((el) =>
      el.querySelector('.fd-score-label')?.textContent === label,
    )?.querySelector('.fd-score-val')?.textContent

  for (const [name, fd] of [
    ['a QSO party', ilqp()],
    [
      'Field Day',
      session({
        event: 'arrlfd',
        qsoCount: 60,
        points: 120,
        totalScore: 340,
        sections: 45,
        log: ten(623),
        composing: [
          { key: 'CLASS', raw: '1A' },
          { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
        ],
      } as Partial<FieldDayStatus>),
    ],
  ] as const) {
    it(`for ${name}: total, contacts, mults or sections, and the first rate tile`, async () => {
      render(
        <>
          <ContestView fieldDay={fd} onSetMode={() => {}} fdActive fdRuleset={null} tier="none" />
          <ContestScoreChip fieldDay={fd} />
        </>,
      )
      await settle()
      const total = document.querySelector('.fd-scoreboard .fd-score-total')?.textContent
      const second = fd.multCount != null ? tile('Mults') : tile('Sections')
      const rate = screen.getByLabelText('Rate over the last 10 contacts').querySelector('.fd-score-val')?.textContent
      const short = document.querySelector('.fd-header h2')?.textContent
      const counted = fd.multCount != null ? `${second} mults` : `${second} sections`
      expect(chipText()).toBe(`${short} · ${total} pts · ${tile('QSOs')} Q · ${counted} · ${rate}/hr`)
    })
  }
})

// ⭐ A CONTACT-COUNT-ONLY CONTEST — not Field Day and no multiplier declared (the WAE DX Contest,
// whose QTCs and band-weighted multipliers this build does not compute). No multiplier count
// means Field Day everywhere else, so without this the chip and the screen dressed it in Field
// Day's clothes: "N pts", a Sections tile, the power-and-bonus line and the ARRL Sections board.
describe('a contest whose screen is its contact count alone', () => {
  const settle = async () => {
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
  }
  const wae = (over: Partial<FieldDayStatus> = {}) =>
    session({
      event: 'waedc_cw',
      qsoCount: 48,
      points: 48,
      totalScore: 48,
      multCount: null,
      sections: 3,
      scoreNoteKey: 'settings.contestScore.contactsOnly',
      log: ten(790),
      ...over,
    } as Partial<FieldDayStatus>)

  it('the chip shows the contact count and the rate, and no points or sections', () => {
    render(<ContestScoreChip fieldDay={wae()} />)
    expect(chipText()).toBe('WAE CW · 48 Q · 41/hr')
  })

  it('the screen shows the count and its note, and none of Field Day’s score, tile or board', async () => {
    render(<ContestView fieldDay={wae()} onSetMode={() => {}} fdActive fdRuleset={null} tier="none" />)
    await settle()
    const labels = [...document.querySelectorAll('.fd-scoreboard .fd-score-label')].map((el) => el.textContent)
    expect(labels).toContain('QSOs')
    expect(labels).not.toContain('Sections')
    expect(document.querySelector('.fd-score-math')).toBeNull()
    expect(screen.queryByLabelText('Worked sections board')).toBeNull()
    expect(screen.getAllByRole('note').some((n) => /your contact count, not your score/.test(n.textContent ?? ''))).toBe(true)
  })

  it('CONTROL: Field Day with the same empty multiplier count keeps its Sections tile, line and board', async () => {
    render(<ContestView fieldDay={wae({ event: 'arrlfd', scoreNoteKey: '' })} onSetMode={() => {}} fdActive fdRuleset={null} tier="none" />)
    await settle()
    const labels = [...document.querySelectorAll('.fd-scoreboard .fd-score-label')].map((el) => el.textContent)
    expect(labels).toContain('Sections')
    expect(document.querySelector('.fd-score-math')).not.toBeNull()
    expect(screen.getByLabelText('Worked sections board')).toBeTruthy()
  })
})

import { describe, expect, it, vi, beforeEach } from 'vitest'
import type { LoggedQso, PathGrade } from '../types'

vi.mock('../api', () => ({ logPathGrades: vi.fn() }))
import { logPathGrades } from '../api'
import {
  PATH_GRADE_READS,
  askPathGrade,
  forgetPathGrades,
  gradeAsk,
  gradeCell,
  gradeKey,
  gradeSentence,
  reasonText,
  validationLines,
} from './pathGrade'

const row = (over: Partial<LoggedQso> = {}): LoggedQso =>
  ({
    id: 'a1',
    call: 'VK2AA',
    grid: 'QF56',
    myGrid: 'FN42',
    band: '20m',
    whenUnix: 1_778_150_000,
    extra: [
      ['SFI', '150'],
      ['CNTY', 'MA,MIDDLESEX'],
      ['APP_NEXUS_PATH', 'S'],
    ],
    ...over,
  }) as LoggedQso

const grade = (over: Partial<PathGrade> = {}): PathGrade => ({
  verdict: 'short',
  confidence: 'likely',
  why: ['MARGIN_S'],
  tier: 'modelled',
  polar: false,
  ssnSource: 'logged',
  ...over,
})

describe('what a grade is asked from', () => {
  it('sends only the fields a grade reads', () => {
    const a = gradeAsk(row())
    expect(a).toEqual({
      whenUnix: 1_778_150_000,
      band: '20m',
      grid: 'QF56',
      myGrid: 'FN42',
      propMode: null,
      extra: [
        ['SFI', '150'],
        ['APP_NEXUS_PATH', 'S'],
      ],
    })
    for (const [k] of a.extra) expect(PATH_GRADE_READS).toContain(k)
  })

  it('keys a row by what its grade depends on, so an edit asks again', () => {
    expect(gradeKey(row())).toBe(gradeKey(row({ extra: [['SFI', '150'], ['CNTY', 'X'], ['APP_NEXUS_PATH', 'S']] })))
    expect(gradeKey(row())).not.toBe(gradeKey(row({ grid: 'QF57' })))
    expect(gradeKey(row())).not.toBe(gradeKey(row({ band: '40m' })))
    expect(gradeKey(row())).not.toBe(gradeKey(row({ extra: [['SFI', '151']] })))
    expect(gradeKey(row())).not.toBe(gradeKey(row({ propMode: 'SAT' })))
    expect(gradeAsk(row({ propMode: 'SAT' })).propMode).toBe('SAT')
  })
})

describe('the Path cell', () => {
  it('shows the route and how sure, the polar cap, and nothing for no grade', () => {
    expect(gradeCell(grade()).text).toBe('▸ likely')
    expect(gradeCell(grade({ verdict: 'long', confidence: 'possible', polar: true })).text).toBe('◂ possible ⊕')
    expect(gradeCell(grade({ verdict: 'either', confidence: undefined })).text).toBe('▸◂ either')
    expect(gradeCell(grade({ verdict: 'unknown', confidence: undefined })).text).toBe('? outside')
    expect(gradeCell(null).text).toBe('—')
  })

  it('dims a grade the model made afterwards, never one stamped when logged', () => {
    expect(gradeCell(grade({ tier: 'modelled' })).dim).toBe(true)
    expect(gradeCell(grade({ tier: 'logged' })).dim).toBe(false)
  })

  it('says in the hover where the grade stands: modelled afterwards, or graded when logged', () => {
    const modelled = gradeCell(grade()).title
    expect(modelled).toMatch(/Short path, likely/)
    expect(modelled).toMatch(/modelled afterwards/i)
    expect(modelled).toMatch(/at least 6 dB better/)
    const logged = gradeCell(grade({ tier: 'logged', why: ['MARGIN_S', 'HEARD_S:2'] })).title
    expect(logged).toMatch(/other receivers along the short path heard the same signal: 2/)
    expect(logged).toMatch(/when it was logged/i)
    // A beam heading is never a reason: a token for it is not put into words.
    expect(reasonText('HEADING_S')).toBeNull()
    // The long path's figures are unvalidated, and the hover says so.
    expect(gradeCell(grade({ verdict: 'long', confidence: 'possible' })).title).toMatch(/unvalidated for long path/)
    expect(gradeCell(grade()).title).not.toMatch(/unvalidated/)
    // An edited contact's stale stamp is named.
    expect(gradeCell(grade({ stampStale: true })).title).toMatch(/another grid or band/)
  })
})

describe('the reasons in words', () => {
  it('reads every token the shack writes, and nothing it does not', () => {
    expect(reasonText('OPEN_L')).toMatch(/only the long path/)
    expect(reasonText('RELIAB_S')).toMatch(/short path was modelled much more reliable/)
    expect(reasonText('WITHIN')).toMatch(/within 6 dB/)
    expect(reasonText('CLOSED')).toMatch(/both paths were modelled closed/)
    expect(reasonText('GREY_S')).toMatch(/grey line along the short path/)
    expect(reasonText('HEARD_L:3')).toMatch(/long path.*: 3/)
    expect(reasonText('LATEDT:60/12')).toMatch(/60 ms.*12/)
    expect(reasonText('LATEDT:60/12')).toMatch(/not evidence on its own/)
    expect(reasonText('SIDEWAYS')).toBeNull()
    expect(reasonText('HEARD_L:x')).toBeNull()
  })

  it('leads the contact card with one sentence', () => {
    expect(gradeSentence(grade({ verdict: 'short', confidence: 'likely', tier: 'logged' }))).toBe('Short path, likely.')
    expect(gradeSentence(grade({ verdict: 'long', confidence: 'possible', polar: true }))).toBe(
      'Long path, possible — across the polar cap.',
    )
    expect(gradeSentence(grade({ verdict: 'unknown', confidence: undefined }))).toMatch(/outside the model/i)
  })
})

describe('asking for grades', () => {
  beforeEach(() => {
    forgetPathGrades()
    vi.mocked(logPathGrades).mockReset()
  })

  it('asks once for the rows shown together, and keeps the answers', async () => {
    vi.mocked(logPathGrades).mockImplementation(async (rows) => rows.map((r) => (r.band === '20m' ? grade() : null)))
    const a = askPathGrade(row())
    const b = askPathGrade(row({ id: 'a2', band: '40m' }))
    expect(await a).toEqual(grade())
    expect(await b).toBeNull()
    expect(logPathGrades).toHaveBeenCalledTimes(1)
    expect(vi.mocked(logPathGrades).mock.calls[0][0]).toHaveLength(2)
    // Kept: the same row again asks nothing.
    expect(await askPathGrade(row())).toEqual(grade())
    expect(logPathGrades).toHaveBeenCalledTimes(1)
  })

  it('a failed ask answers nothing and asks again next time', async () => {
    vi.mocked(logPathGrades).mockRejectedValueOnce(new Error('gone'))
    expect(await askPathGrade(row())).toBeNull()
    vi.mocked(logPathGrades).mockResolvedValueOnce([grade()])
    expect(await askPathGrade(row())).toEqual(grade())
  })
})

describe('the success measures, for the expert', () => {
  it('puts the 30 days into percentages, and says when there is nothing yet', () => {
    const lines = validationLines({
      running: false,
      contacts: 120,
      graded: 100,
      ungradable: 20,
      cellsDone: 80,
      cellsTotal: 80,
      last30Days: {
        all: { graded: 40, open: 34, outside: 4 },
        polar: { graded: 10, open: 6, outside: 3 },
        nonPolar: { graded: 30, open: 28, outside: 1 },
        byBand: [],
        headingJudged: 8,
        headingAgree: 6,
      },
    })
    expect(lines.join('\n')).toMatch(/85%/)
    expect(lines.join('\n')).toMatch(/10%/)
    expect(lines.join('\n')).toMatch(/60%.*93%/)
    expect(lines.join('\n')).toMatch(/75%/)
    expect(
      validationLines({ running: true, contacts: 0, graded: 0, ungradable: 0, cellsDone: 3, cellsTotal: 9 }).join('\n'),
    ).toMatch(/3 .* 9/)
  })
})

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import type { ContactPathDay, LoggedQso, PathGrade } from '../../types'

vi.mock('../../api', () => ({ contactPathDay: vi.fn(), logPathGrades: vi.fn(async () => [null]) }))
import { contactPathDay, logPathGrades } from '../../api'
import { ContactPath } from './ContactPath'
import { forgetPathGrades } from '../../features/pathGrade'

const q = {
  id: 'a', call: 'VK2AA', grid: 'QF56', myGrid: 'FN42', band: '20m', whenUnix: 1_778_150_000,
  extra: [['SFI', '150']],
} as unknown as LoggedQso

const day = (grade?: PathGrade): ContactPathDay => ({
  ssnSource: 'logged',
  grade,
  pred: {
    engine: 'p533', bands: [], mufNow: 20, mufHourly: [],
    day: {
      nowHour: 10, mufShort: Array(24).fill(20), mufLong: Array(24).fill(18),
      bands: [{ band: '20m', cells: Array.from({ length: 24 }, () => ({ shortPct: 60, longPct: 20, verdict: 'short', confidence: 'likely' })) }],
      hours: Array.from({ length: 24 }, () => ({ kp: null, kpSource: 'unknown', polarShort: false, polarLong: false, greyMe: false, greyDx: false })),
    },
  } as unknown as ContactPathDay['pred'],
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  forgetPathGrades()
})

describe("the contact card's Path", () => {
  it('leads with one sentence, keeps the reasons one click away, and draws the day', async () => {
    vi.mocked(contactPathDay).mockResolvedValue(
      day({ verdict: 'long', confidence: 'possible', why: ['OPEN_L'], tier: 'modelled', polar: false, ssnSource: 'logged' }),
    )
    const { container } = render(<ContactPath q={q} canPick picked={null} onPick={() => {}} />)
    await screen.findByText(/Long path, possible\./)
    expect(container.querySelector('.contact-card-path-line .route-lp-badge')?.textContent).toBe('modelled, unvalidated for long path')
    const why = container.querySelector('details.contact-card-path-why') as HTMLDetailsElement
    expect(why.open).toBe(false)
    expect(why.textContent).toMatch(/only the long path was modelled open/)
    expect(why.textContent).toMatch(/Modelled afterwards/)
    expect(container.querySelector('.path-day')).not.toBeNull()
    expect(vi.mocked(contactPathDay).mock.calls[0][0]).toEqual({
      whenUnix: 1_778_150_000, band: '20m', grid: 'QF56', myGrid: 'FN42', extra: [['SFI', '150']],
    })
  })

  it('says why there is no path, and never invents one', async () => {
    vi.mocked(contactPathDay).mockResolvedValue(null)
    render(<ContactPath q={q} canPick={false} picked={null} onPick={() => {}} />)
    await screen.findByText(/needs both grid squares/)
    expect(logPathGrades).toHaveBeenCalled()
  })

  it("offers the operator's own antenna path only where the card can save and none is set", async () => {
    vi.mocked(contactPathDay).mockResolvedValue(null)
    const onPick = vi.fn()
    const { rerender } = render(<ContactPath q={q} canPick picked={null} onPick={onPick} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Long path' }))
    expect(onPick).toHaveBeenCalledWith('L')
    rerender(<ContactPath q={q} canPick picked="L" onPick={onPick} />)
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Long path' })).toBeNull())
    rerender(<ContactPath q={q} canPick={false} picked={null} onPick={onPick} />)
    expect(screen.queryByRole('button', { name: 'Short path' })).toBeNull()
  })
})

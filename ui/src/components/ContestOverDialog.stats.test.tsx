// @vitest-environment jsdom
//
// THE CONTEST-OVER CHECKLIST OFFERS THE STATS — "See the contest stats" hands the operator to the
// Contest screen's Stats tab and does nothing else: no merge, no export, no save, Contest mode left on.
// Offered only by a host that can show the tab, and only with contacts to count.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { ContestOverDialog, type ContestOverOffer } from './ContestOverDialog'
import { t } from '../i18n'

vi.mock('../api', () => ({
  fdMergeToGeneral: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => ''),
  setUnassistedMode: vi.fn(async () => ({})),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const OFFER: ContestOverOffer = { contacts: 212, event: 'ilqp', putBack: [], clubQueued: 0 }

afterEach(cleanup)

describe('the checklist’s Stats entry', () => {
  it('opens the stats and does nothing else', () => {
    const onStats = vi.fn()
    const onTurnOff = vi.fn(async () => {})
    const onNotNow = vi.fn()
    render(<ContestOverDialog offer={OFFER} onNotNow={onNotNow} onTurnOff={onTurnOff} onStats={onStats} />)
    fireEvent.click(screen.getByRole('button', { name: t('contestOver.stats') }))
    expect(onStats).toHaveBeenCalledTimes(1)
    expect(onTurnOff).not.toHaveBeenCalled()
    expect(onNotNow).not.toHaveBeenCalled()
    for (const f of Object.values(api)) expect(f).not.toHaveBeenCalled()
  })

  it('is not offered by a host without the tab, nor with no contacts', () => {
    render(<ContestOverDialog offer={OFFER} onNotNow={() => {}} onTurnOff={async () => {}} />)
    expect(screen.queryByRole('button', { name: t('contestOver.stats') })).toBeNull()
    cleanup()
    render(
      <ContestOverDialog offer={{ ...OFFER, contacts: 0, clubQueued: 1 }} onNotNow={() => {}} onTurnOff={async () => {}} onStats={() => {}} />,
    )
    expect(screen.queryByRole('button', { name: t('contestOver.stats') })).toBeNull()
  })
})

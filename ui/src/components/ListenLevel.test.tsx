// @vitest-environment jsdom
//
// #468: THE LISTENING LEVEL ON THE MAIN WINDOW. Settings ▸ Radio ▸ "Receive audio on this computer"
// ▸ Listening level was the only way to change how loud Nexus plays the band, which is every
// adjustment for a station whose only receive audio is Nexus. The top bar now carries a slider for
// the same setting (`monitorLevel`), shown while that audio is switched on.
//
// A drag is many changes and each save writes the settings file, so the slider follows the pointer
// at once and saves the value it settles on (250 ms after the last change), once.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { ListenLevel } from './ListenLevel'
import { t } from '../i18n'

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})
const slider = (c: HTMLElement) => c.querySelector('input[type=range]') as HTMLInputElement

describe('the top bar’s listening level (#468)', () => {
  it('shows the saved level and names itself', () => {
    const { container } = render(<ListenLevel level={0.4} onCommit={() => {}} />)
    expect(slider(container).value).toBe('0.4')
    expect(slider(container).getAttribute('aria-label')).toBe(t('settings.headphoneMonitor.level.aria'))
    expect(container.querySelector('.listen-level')?.getAttribute('title')).toBe(t('topbar.listen.title', { pct: 40 }))
  })

  it('a drag follows the pointer and saves once, the value it settles on', () => {
    const onCommit = vi.fn()
    const { container } = render(<ListenLevel level={0.4} onCommit={onCommit} />)
    for (const v of ['0.5', '0.6', '0.7']) fireEvent.change(slider(container), { target: { value: v } })
    expect(slider(container).value).toBe('0.7')
    expect(onCommit).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(300))
    expect(onCommit).toHaveBeenCalledTimes(1)
    expect(onCommit).toHaveBeenCalledWith(0.7)
  })

  it('follows a level changed elsewhere (Settings) when not being dragged', () => {
    const { container, rerender } = render(<ListenLevel level={0.4} onCommit={() => {}} />)
    rerender(<ListenLevel level={0.8} onCommit={() => {}} />)
    expect(slider(container).value).toBe('0.8')
  })
})

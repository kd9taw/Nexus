// @vitest-environment jsdom
//
// #468: THE LISTENING LEVEL ON THE MAIN WINDOW. Settings ▸ Radio ▸ "Receive audio on this computer"
// ▸ Listening level was the only way to change how loud Nexus plays the band. The top bar now carries
// a speaker chip that opens a slider for the same setting (`monitorLevel`).
//
// THE BAR ITSELF GETS ONLY THE CHIP: the slider is in a portaled popover. An inline slider wrapped the
// bar onto a second row at 1366×768 and 1024×768, which moved the cockpit and Stop TX down.
//
// A drag is many changes and each save writes the settings file, so the slider follows the pointer at
// once and saves the value it settles on (250 ms after the last change), once.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { ListenLevel } from './ListenLevel'
import { t } from '../i18n'

beforeAll(() => {
  globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const chip = (c: HTMLElement) => c.querySelector('button.listen-chip') as HTMLButtonElement
/** Open the popover from the keyboard, as ui/Menu's tests do, and return its slider. */
async function open(c: HTMLElement): Promise<HTMLInputElement> {
  fireEvent.keyDown(chip(c), { key: 'Enter' })
  const pop = await screen.findByRole('dialog', { name: t('settings.headphoneMonitor.level.aria') })
  expect(c.contains(pop), 'portaled out of the bar: it takes no room in it').toBe(false)
  expect((pop.firstElementChild as HTMLElement).style.zoom, 'the app zoom re-applied').toBe('var(--ui-zoom, 1)')
  return pop.querySelector('input[type=range]') as HTMLInputElement
}

describe('the top bar’s listening level (#468)', () => {
  it('the bar holds only an icon chip, named and titled with the level; no slider in the bar', () => {
    const { container } = render(<ListenLevel level={0.4} onCommit={() => {}} />)
    expect(chip(container).getAttribute('aria-label')).toBe(t('settings.headphoneMonitor.level.aria'))
    expect(chip(container).title).toBe(t('topbar.listen.title', { pct: 40 }))
    expect(container.querySelector('input')).toBeNull()
  })

  it('the chip opens the slider on the saved level', async () => {
    const { container } = render(<ListenLevel level={0.4} onCommit={() => {}} />)
    const s = await open(container)
    expect(s.value).toBe('0.4')
    expect(s.getAttribute('aria-label')).toBe(t('settings.headphoneMonitor.level.aria'))
  })

  it('a drag follows the pointer and saves once, the value it settles on', async () => {
    const onCommit = vi.fn()
    const { container } = render(<ListenLevel level={0.4} onCommit={onCommit} />)
    const s = await open(container)
    vi.useFakeTimers()
    for (const v of ['0.5', '0.6', '0.7']) fireEvent.change(s, { target: { value: v } })
    expect(s.value).toBe('0.7')
    expect(onCommit).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(300))
    expect(onCommit).toHaveBeenCalledTimes(1)
    expect(onCommit).toHaveBeenCalledWith(0.7)
  })

  it('follows a level changed elsewhere (Settings) when not being dragged', () => {
    const { container, rerender } = render(<ListenLevel level={0.4} onCommit={() => {}} />)
    rerender(<ListenLevel level={0.8} onCommit={() => {}} />)
    expect(chip(container).title).toBe(t('topbar.listen.title', { pct: 80 }))
  })
})

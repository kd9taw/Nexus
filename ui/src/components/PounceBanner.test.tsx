// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { PounceBanner } from './PounceBanner'
import type { PounceAlert } from '../usePounce'

const alert: PounceAlert = {
  call: '3Y0X',
  band: '20m',
  mode: 'CW',
  freqMhz: 14.025,
  tags: ['NewEntity'] as never,
  entity: 'Bouvet Island',
  atUnix: 1000,
}

afterEach(cleanup)

describe('PounceBanner', () => {
  it('renders nothing when there is no alert', () => {
    const { container } = render(
      <PounceBanner alert={null} onDismiss={() => {}} onWork={() => {}} />,
    )
    expect(container.querySelector('.pounce-banner')).toBeNull()
  })

  it('shows the call, the entity and where to find it', () => {
    render(<PounceBanner alert={alert} onDismiss={() => {}} onWork={() => {}} />)
    expect(screen.getByText('3Y0X')).toBeTruthy()
    expect(screen.getByText('Bouvet Island')).toBeTruthy()
    // The frequency is the actionable part — the band alone would not let you QSY.
    expect(screen.getByText(/14\.025 MHz/)).toBeTruthy()
  })

  // It interrupts on purpose, so it must announce assertively rather than politely — a
  // screen-reader user gets the same "drop what you're doing" that the earcon gives.
  it('announces assertively', () => {
    const { container } = render(
      <PounceBanner alert={alert} onDismiss={() => {}} onWork={() => {}} />,
    )
    const el = container.querySelector('.pounce-banner')
    expect(el?.getAttribute('role')).toBe('alert')
    expect(el?.getAttribute('aria-live')).toBe('assertive')
  })

  it('hands the whole alert to onWork so the caller can QSY to the spot', () => {
    const onWork = vi.fn()
    render(<PounceBanner alert={alert} onDismiss={() => {}} onWork={onWork} />)
    fireEvent.click(screen.getByRole('button', { name: /work it/i }))
    expect(onWork).toHaveBeenCalledWith(alert)
  })

  it('dismisses without working', () => {
    const onDismiss = vi.fn()
    const onWork = vi.fn()
    render(<PounceBanner alert={alert} onDismiss={onDismiss} onWork={onWork} />)
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }))
    expect(onDismiss).toHaveBeenCalled()
    expect(onWork).not.toHaveBeenCalled()
  })

  // OPERATOR RULING 2026-07-26: Work is ALWAYS available. It previously disabled itself and
  // showed "In a QSO with <call>" or "Transmitting" in place of the button.
  //
  // That was wrong for what Pounce is. It exists because working a rare one is a race against a
  // pileup that has not formed yet — and whether to drop the current contact to chase it is the
  // operator's judgement in that moment, not something the app should refuse on their behalf.
  // Worse, the refusal text replaced the very control they were reaching for, so an alert that
  // interrupted them could not be acted on at all.
  //
  // Safe because the refusal was UI-only: `work_spot_split` sets the mode then the dial, and the
  // radio loop already defers a retune while a slot is actively transmitting (rigs reject
  // VFO/mode changes mid-TX), so a click during our own over lands at the next safe moment.
  it('offers Work unconditionally — mid-QSO and mid-transmission included', () => {
    const onWork = vi.fn()
    render(<PounceBanner alert={alert} onDismiss={() => {}} onWork={onWork} />)
    const btn = screen.getByRole('button', { name: /work it/i })
    expect((btn as HTMLButtonElement).disabled).toBe(false)
    fireEvent.click(btn)
    expect(onWork).toHaveBeenCalledWith(alert)
  })

  // Still notify-never-act: the banner offers a button, it does not move the radio by itself.
  it('does nothing until the operator actually presses Work', () => {
    const onWork = vi.fn()
    render(<PounceBanner alert={alert} onDismiss={() => {}} onWork={onWork} />)
    expect(onWork).not.toHaveBeenCalled()
  })

  it('can be dismissed without working it', () => {
    const onDismiss = vi.fn()
    const onWork = vi.fn()
    render(<PounceBanner alert={alert} onDismiss={onDismiss} onWork={onWork} />)
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }))
    expect(onDismiss).toHaveBeenCalled()
    expect(onWork).not.toHaveBeenCalled()
  })

  // A band-only alert (no frequency) must still render rather than crashing or showing "null MHz".
  it('falls back to the band when no frequency is known', () => {
    render(
      <PounceBanner
        alert={{ ...alert, freqMhz: null }}
        onDismiss={() => {}}
        onWork={() => {}}
      />,
    )
    expect(screen.getByText(/20m · CW/)).toBeTruthy()
  })
})

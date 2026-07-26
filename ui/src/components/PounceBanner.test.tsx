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

  // THE MIS-CLICK GUARD. The banner appears unbidden over whatever you are doing, and
  // `work_spot` has no in-QSO guard of its own — so without this, one stray click QSYs away from
  // a live contact. An alert must never become the way you lose the QSO you were already in.
  it('refuses to work a spot while a QSO is in progress, and says why', () => {
    const onWork = vi.fn()
    render(
      <PounceBanner
        alert={alert}
        onDismiss={() => {}}
        onWork={onWork}
        blockReason="In a QSO with K1ABC"
      />,
    )
    const btn = screen.getByRole('button', { name: /in a qso with k1abc/i })
    expect((btn as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(btn)
    expect(onWork).not.toHaveBeenCalled()
  })

  it('refuses while transmitting', () => {
    const onWork = vi.fn()
    render(
      <PounceBanner alert={alert} onDismiss={() => {}} onWork={onWork} blockReason="Transmitting" />,
    )
    fireEvent.click(screen.getByRole('button', { name: /transmitting/i }))
    expect(onWork).not.toHaveBeenCalled()
  })

  // Refusing to WORK it must never mean refusing to TELL you. The alert still shows and can
  // still be dismissed — you just cannot lose your current contact to a stray click.
  it('still shows the alert and allows dismiss while blocked', () => {
    const onDismiss = vi.fn()
    render(
      <PounceBanner
        alert={alert}
        onDismiss={onDismiss}
        onWork={() => {}}
        blockReason="Transmitting"
      />,
    )
    expect(screen.getByText('3Y0X')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }))
    expect(onDismiss).toHaveBeenCalled()
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

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { FrequencyReadout, formatDialMhz } from './FrequencyReadout'

afterEach(cleanup)

describe('FrequencyReadout', () => {
  it('formats to 10 Hz resolution (4 decimals)', () => {
    expect(formatDialMhz(14.074)).toBe('14.0740')
    expect(formatDialMhz(7.1)).toBe('7.1000')
  })

  it('renders the value, unit, and an optional band chip', () => {
    const { container, rerender } = render(<FrequencyReadout dialMhz={14.074} band="20m" />)
    expect(container.querySelector('.readout-val')?.textContent).toBe('14.0740')
    expect(container.textContent).toContain('MHz')
    expect(container.querySelector('.band-chip')?.textContent).toBe('20m')
    rerender(<FrequencyReadout dialMhz={14.074} />)
    expect(container.querySelector('.band-chip')).toBeNull()
  })

  it('applies size + blocked classes', () => {
    const { container, rerender } = render(<FrequencyReadout dialMhz={14.074} size="hero" />)
    expect(container.querySelector('.readout')?.className).toContain('hero')
    rerender(<FrequencyReadout dialMhz={14.074} size="compact" txBlocked />)
    const cls = container.querySelector('.readout')?.className ?? ''
    expect(cls).toContain('compact')
    expect(cls).toContain('blocked')
  })

  it('edits on click and commits the parsed MHz on Enter', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    const input = container.querySelector('input') as HTMLInputElement
    expect(input.value).toBe('14.07400') // seeded at 10 Hz from the current dial
    fireEvent.change(input, { target: { value: '14.250' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onCommit).toHaveBeenCalledWith(14.25)
  })

  it('skips a no-op commit (opened then Enter with no change) — no spurious QSY', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={7.19985} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    fireEvent.keyDown(container.querySelector('input')!, { key: 'Enter' })
    expect(onCommit).not.toHaveBeenCalled()
  })

  // #511/#454: typing a frequency and clicking away used to throw it away without a word, on
  // every live-rig readout (Enter was the only commit). Leaving the field with a VALID value now
  // commits it exactly as Enter does; Esc is still the way to cancel.
  it('leaving the field with a typed frequency commits it, as Enter does', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    fireEvent.change(container.querySelector('input')!, { target: { value: '14,2' } })
    fireEvent.blur(container.querySelector('input')!)
    expect(onCommit).toHaveBeenCalledTimes(1)
    expect(onCommit).toHaveBeenCalledWith(14.2)
    expect(container.querySelector('input')).toBeNull()
  })

  it('leaving the field with something that is not a frequency commits nothing', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    for (const bad of ['', 'abc', '14.0.74', '0', '-7']) {
      fireEvent.click(screen.getByRole('button'))
      fireEvent.change(container.querySelector('input')!, { target: { value: bad } })
      fireEvent.blur(container.querySelector('input')!)
      expect(container.querySelector('input'), `"${bad}" left the field open`).toBeNull()
    }
    expect(onCommit).not.toHaveBeenCalled()
  })

  it('Enter commits once, even when the field then loses focus', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    const input = container.querySelector('input')!
    fireEvent.change(input, { target: { value: '14.2' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.blur(input)
    expect(onCommit).toHaveBeenCalledTimes(1)
  })

  it('Esc cancels, even when the field then loses focus', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    const input = container.querySelector('input')!
    fireEvent.change(input, { target: { value: '14.2' } })
    fireEvent.keyDown(input, { key: 'Escape' })
    fireEvent.blur(input)
    expect(onCommit).not.toHaveBeenCalled()
  })

  it('Space activation does NOT propagate (guards the cockpit spacebar-PTT)', () => {
    const parentSpy = vi.fn()
    window.addEventListener('keydown', parentSpy)
    try {
      render(<FrequencyReadout dialMhz={14.074} editable onCommit={vi.fn()} />)
      fireEvent.keyDown(screen.getByRole('button'), { key: ' ' })
      expect(parentSpy).not.toHaveBeenCalled() // stopPropagation kept Space from reaching PTT
      expect(document.querySelector('input')).not.toBeNull() // ...while still entering edit mode
    } finally {
      window.removeEventListener('keydown', parentSpy)
    }
  })

  it('cancels on Escape without committing', () => {
    const onCommit = vi.fn()
    const { container } = render(<FrequencyReadout dialMhz={14.074} editable onCommit={onCommit} />)
    fireEvent.click(screen.getByRole('button'))
    fireEvent.change(container.querySelector('input')!, { target: { value: '99' } })
    fireEvent.keyDown(container.querySelector('input')!, { key: 'Escape' })
    expect(onCommit).not.toHaveBeenCalled()
    expect(container.querySelector('input')).toBeNull() // reverted to display
  })

  it('is not interactive when disabled', () => {
    const onCommit = vi.fn()
    const { container } = render(
      <FrequencyReadout dialMhz={14.074} editable disabled onCommit={onCommit} />,
    )
    expect(container.querySelector('[role="button"]')).toBeNull()
    fireEvent.click(container.querySelector('.readout')!)
    expect(container.querySelector('input')).toBeNull()
  })
})

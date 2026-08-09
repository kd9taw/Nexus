// @vitest-environment jsdom
// The Field chip (outdoor/POTA readability). The whole point is REACHABILITY: the remedy for
// an unreadable screen cannot live in Settings, because the operator who needs it cannot read
// the screen to find it. It sits beside Light/Dark — always on screen — and one tap each way.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { ThemeSwitcher } from './ThemeSwitcher'

afterEach(cleanup)

describe('the Field chip', () => {
  it('renders beside the theme chips and round-trips', () => {
    const onFieldChange = vi.fn()
    render(<ThemeSwitcher theme="dark" onChange={() => {}} field={false} onFieldChange={onFieldChange} />)
    const chip = screen.getByRole('button', { name: 'Field' })
    expect(chip.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(chip)
    expect(onFieldChange).toHaveBeenCalledWith(true)
  })

  it('reads as ON and offers the way back', () => {
    const onFieldChange = vi.fn()
    render(<ThemeSwitcher theme="dark" onChange={() => {}} field onFieldChange={onFieldChange} />)
    const chip = screen.getByRole('button', { name: 'Field' })
    expect(chip.getAttribute('aria-pressed')).toBe('true')
    expect(chip.title, 'the ON state must say how to get out').toMatch(/turn off/i)
    fireEvent.click(chip)
    expect(onFieldChange).toHaveBeenCalledWith(false)
  })

  it('suggests light for daylight without flipping the theme itself', () => {
    // The physics favours light outdoors, but silently changing the operator's theme is an
    // invisible decision — the chip informs, in its tooltip, and touches nothing.
    const onChange = vi.fn()
    render(<ThemeSwitcher theme="dark" onChange={onChange} field={false} onFieldChange={() => {}} />)
    const chip = screen.getByRole('button', { name: 'Field' })
    expect(chip.title).toMatch(/light theme reads best/i)
    fireEvent.click(chip)
    expect(onChange, 'field mode must never change the theme').not.toHaveBeenCalled()
  })

  it('is absent entirely when the host does not wire it', () => {
    // Old render sites keep exactly their old chips — the prop is the opt-in.
    render(<ThemeSwitcher theme="dark" onChange={() => {}} />)
    expect(screen.queryByRole('button', { name: 'Field' })).toBeNull()
  })
})

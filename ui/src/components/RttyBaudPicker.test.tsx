// @vitest-environment jsdom
//
// #488: the RTTY screen's baud figure is a picker. Switching 45.45 and 50 baud no longer means a
// trip to Settings ▸ RTTY: it saves the same setting (`rttyBaud`) from the screen. Not while a
// transmission is on the air: the baud is the transmitter's bit clock, so the picker is disabled
// while sending or latched, and nothing changes mid-over.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { RttyBaudPicker } from './RttyBaudPicker'
import { t } from '../i18n'

afterEach(cleanup)
const select = (c: HTMLElement) => c.querySelector('select.rtty-baud-pick') as HTMLSelectElement

describe('the RTTY baud picker (#488)', () => {
  it('shows the baud in use and offers the three standard rates', () => {
    const { container } = render(<RttyBaudPicker baud={45.45} onPick={() => {}} />)
    const s = select(container)
    expect(s.value).toBe('45.45')
    expect([...s.options].map((o) => o.value)).toEqual(['45.45', '50', '75'])
    expect(s.title).toBe(t('rtty.header.baud.title'))
  })

  it('picking a rate saves it', () => {
    const onPick = vi.fn()
    const { container } = render(<RttyBaudPicker baud={45.45} onPick={onPick} />)
    fireEvent.change(select(container), { target: { value: '50' } })
    expect(onPick).toHaveBeenCalledWith(50)
  })

  it('a rate set elsewhere that is not one of the three is still shown, not replaced', () => {
    const { container } = render(<RttyBaudPicker baud={100} onPick={() => {}} />)
    expect(select(container).value).toBe('100')
  })

  it('disabled while transmitting: nothing changes the bit clock mid-over', () => {
    const onPick = vi.fn()
    const { container } = render(<RttyBaudPicker baud={45.45} disabled onPick={onPick} />)
    expect(select(container).disabled).toBe(true)
  })
})

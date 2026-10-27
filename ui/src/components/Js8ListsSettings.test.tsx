// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8's lists, marks and sounds (components/Js8ListsSettings): JS8Call's
// defaults shown for a form from before them, each list parsed as JS8Call reads it, the marks as
// typed, and each sound on or off in JS8Call's table order with a Test that plays it.
import { describe, expect, it, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { Js8ListsSettings, parseJs8Words } from './Js8ListsSettings'
import type { Settings } from '../types'

vi.mock('../alerts', () => ({ beep: vi.fn(), doubleBeep: vi.fn() }))
afterEach(cleanup)

const mount = (form: Partial<Settings>) => {
  const setList = vi.fn()
  const setMark = vi.fn()
  const setSounds = vi.fn()
  render(<Js8ListsSettings form={form as Settings} setList={setList} setMark={setMark} setSounds={setSounds} />)
  return { setList, setMark, setSounds }
}

describe('the JS8 lists, marks and sounds in Settings', () => {
  it('shows JS8Call’s defaults for a form from before them: empty lists, ♢ and ……, every sound off', () => {
    mount({})
    expect((screen.getByLabelText('Hide these callsigns in the RX pane') as HTMLInputElement).value).toBe('')
    expect((screen.getByLabelText('End of transmission mark') as HTMLInputElement).value).toBe('♢')
    expect((screen.getByLabelText('Missing frame mark') as HTMLInputElement).value).toBe('……')
    const switches = screen.getAllByRole('switch')
    expect(switches).toHaveLength(7)
    expect(switches.every((s) => s.getAttribute('aria-checked') === 'false')).toBe(true)
  })

  it('reads a list as JS8Call does — comma separated, trimmed, upper-cased — and a mark as typed', () => {
    expect(parseJs8Words(' w1aw, ,K1abc ')).toEqual(['W1AW', 'K1ABC'])
    const { setList, setMark } = mount({ js8RxBlocklist: ['N0BAD'], js8Eot: '♢' })
    fireEvent.change(screen.getByLabelText('Highlight these words or callsigns (primary colour)'), { target: { value: 'net, k1abc' } })
    expect(setList).toHaveBeenCalledWith('js8HighlightPrimary', ['NET', 'K1ABC'])
    fireEvent.change(screen.getByLabelText('End of transmission mark'), { target: { value: '#' } })
    expect(setMark).toHaveBeenCalledWith('js8Eot', '#')
  })

  it('turns a sound on in JS8Call’s table order, off again, and tests one', async () => {
    const alerts = await import('../alerts')
    const { setSounds } = mount({ js8Sounds: ['inbox'] })
    fireEvent.click(screen.getByRole('switch', { name: 'A CQ' }))
    expect(setSounds).toHaveBeenLastCalledWith(['cq', 'inbox'])
    fireEvent.click(screen.getByRole('switch', { name: 'A message filed in your Inbox' }))
    expect(setSounds).toHaveBeenLastCalledWith([])
    fireEvent.click(screen.getByRole('button', { name: 'Test the sound for A heartbeat' }))
    expect(alerts.beep).toHaveBeenCalledTimes(1)
  })
})

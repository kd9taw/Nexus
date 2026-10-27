// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 ▸ Colours and fonts (components/Js8PaneStyleSettings): JS8Call's colour for
// each element and a font for each pane, every colour starting as the theme's, a pick kept for the
// theme in use, a sample of each pane drawn with what is picked, the hard-to-read line and Reset.
import { describe, expect, it, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { Js8PaneStyleSettings } from './Js8PaneStyleSettings'
import type { Settings } from '../types'
import type { Js8PaneStyle, Js8Theme } from '../features/js8PaneStyle'

afterEach(cleanup)

const mount = (style: Partial<Js8PaneStyle> | undefined, theme: Js8Theme = 'dark') => {
  const onChange = vi.fn()
  const form = { js8PaneStyle: style === undefined ? undefined : { light: {}, dark: {}, fonts: {}, ...style } } as unknown as Settings
  const r = render(<Js8PaneStyleSettings form={form} theme={theme} onChange={onChange} />)
  return { onChange, ...r }
}
const sample = (pane: string) => document.querySelector(`[data-pane-style="${pane}"] .js8-style-sample`) as HTMLElement
const picker = (name: string) => screen.getByLabelText(name) as HTMLInputElement

describe('Colours and fonts in Settings ▸ JS8', () => {
  it('offers JS8Call’s colours and fonts per pane, every one the theme’s until picked', () => {
    mount(undefined)
    expect(document.querySelectorAll('input[type="color"]')).toHaveLength(12)
    for (const name of ['RX pane: Background', 'RX pane: Received text', 'RX pane: Transmitted text', 'Band activity and the call list: CQ messages', 'Compose box: Text'])
      expect(picker(name).type).toBe('color')
    expect(screen.getAllByRole('button', { name: /^Reset / })).toHaveLength(3)
    for (const pane of ['rx', 'lists', 'compose']) expect(sample(pane).getAttribute('style'), `${pane}: nothing of the operator’s`).toBeNull()
  })

  it('keeps a picked colour for the theme in use, and draws the sample with it', () => {
    const { onChange } = mount(undefined, 'dark')
    fireEvent.change(picker('RX pane: Background'), { target: { value: '#ffeaa7' } })
    expect(onChange).toHaveBeenLastCalledWith({ light: {}, dark: { rxBg: '#ffeaa7' }, fonts: {} })
    cleanup()
    mount({ dark: { rxBg: '#ffeaa7' } }, 'dark')
    expect(sample('rx').style.getPropertyValue('--js8-rx-bg')).toBe('#ffeaa7')
    expect(picker('RX pane: Background').value).toBe('#ffeaa7')
    cleanup()
    mount({ dark: { rxBg: '#ffeaa7' } }, 'light')
    expect(sample('rx').style.getPropertyValue('--js8-rx-bg'), 'a Dark pick never paints Light').toBe('')
  })

  it('takes a font as a family and a size in points, for both themes', () => {
    const { onChange } = mount(undefined, 'light')
    fireEvent.change(screen.getByLabelText('Compose box, Font: family'), { target: { value: 'Consolas' } })
    expect(onChange).toHaveBeenLastCalledWith({ light: {}, dark: {}, fonts: { compose: { family: 'Consolas', sizePt: 0 } } })
    cleanup()
    const again = mount({ fonts: { compose: { family: 'Consolas', sizePt: 0 } } }, 'light')
    fireEvent.change(screen.getByLabelText('Compose box, Font: size in points'), { target: { value: '12' } })
    expect(again.onChange).toHaveBeenLastCalledWith({ light: {}, dark: {}, fonts: { compose: { family: 'Consolas', sizePt: 12 } } })
    expect(sample('compose').style.getPropertyValue('--js8-compose-font-family')).toBe('"Consolas", var(--font-mono)')
  })

  it('Reset gives one pane back to the theme: its colours for the theme in use, and its fonts', () => {
    const { onChange } = mount(
      { light: { rxBg: '#fafafa' }, dark: { rxBg: '#000000', txFg: '#ff0000', cq: '#1f6f3f' }, fonts: { rx: { family: 'Consolas', sizePt: 0 }, table: { family: '', sizePt: 11 } } },
      'dark',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Reset RX pane' }))
    expect(onChange).toHaveBeenLastCalledWith({ light: { rxBg: '#fafafa' }, dark: { cq: '#1f6f3f' }, fonts: { table: { family: '', sizePt: 11 } } })
  })

  it('says when a text would be hard to read on its background, and nothing when it reads', () => {
    mount({ dark: { rxBg: '#ffffff', rxFg: '#eeeeee', txFg: '#000000' } })
    expect(screen.getAllByRole('status').map((s) => s.textContent)).toEqual([
      'Hard to read: “Received text” on “Background” is 1.2:1, where 4.5:1 or more reads well.',
    ])
    cleanup()
    mount({ dark: { rxBg: '#ffffff', rxFg: '#000000', txFg: '#000000' } })
    expect(screen.queryAllByRole('status'), 'control: both read').toEqual([])
  })
})

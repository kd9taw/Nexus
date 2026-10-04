// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ScaleStrip } from './ScaleStrip'
import { loadScaleSettings, scaleDefaults, useScaleSettings, type ScopeCockpit } from './scaleSettings'

const strip = () => screen.getByRole('group', { name: 'Scope display settings' })
const resolution = () => within(strip()).queryByRole('group', { name: 'Resolution' })
const detector = () => within(strip()).getByRole('group', { name: 'Detector' })
const averaging = () => within(strip()).getByRole('combobox', { name: /Smooth/ }) as HTMLSelectElement

beforeEach(() => window.localStorage.clear())

describe('the scope’s ⚙ strip', () => {
  it('shows the record it is given and reports each change', () => {
    const onChange = vi.fn()
    render(
      <ScaleStrip
        settings={{ ...scaleDefaults('phone'), window: 'sharp', detector: 'average', averageMs: 1000 }}
        onChange={onChange}
        windowControl
        control
      />,
    )
    const windows = within(resolution()!).getAllByRole('button')
    expect(windows.map((b) => b.textContent)).toEqual(['47 Hz', '23 Hz', '12 Hz'])
    expect(windows.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'false', 'true'])
    expect(averaging().value).toBe('1000')
    expect(Array.from(averaging().options, (o) => o.textContent)).toEqual(['Off', '50 ms', '100 ms', '250 ms', '500 ms', '1 s', '2 s'])
    const detectors = within(detector()).getAllByRole('button')
    expect(detectors.map((b) => [b.textContent, b.getAttribute('aria-pressed')])).toEqual([
      ['Peak', 'false'],
      ['Avg', 'true'],
    ])

    fireEvent.click(windows[0])
    fireEvent.change(averaging(), { target: { value: '0' } })
    fireEvent.click(detectors[0])
    expect(onChange.mock.calls).toEqual([[{ window: 'fast' }], [{ averageMs: 0 }], [{ detector: 'peak' }]])
  })

  it('offers no window where the row’s window is fixed, and disables it where this window does not drive the station', () => {
    const { rerender } = render(<ScaleStrip settings={scaleDefaults('operate')} onChange={() => {}} windowControl={false} control />)
    expect(resolution()).toBeNull()
    rerender(<ScaleStrip settings={scaleDefaults('phone')} onChange={() => {}} windowControl control={false} />)
    for (const b of within(resolution()!).getAllByRole('button')) {
      expect(b).toHaveProperty('disabled', true)
      expect(b.getAttribute('title')).toMatch(/follows Nexus at the shack/)
    }
    // The drawing is this window's own: never disabled.
    expect(averaging().disabled).toBe(false)
    for (const b of within(detector()).getAllByRole('button')) expect(b).toHaveProperty('disabled', false)
  })

  it('bound to a cockpit’s record, persists what the operator picks', () => {
    function Host({ cockpit }: { cockpit: ScopeCockpit }) {
      const [s, update] = useScaleSettings(cockpit)
      return <ScaleStrip settings={s} onChange={update} windowControl control />
    }
    render(<Host cockpit="cw" />)
    expect(averaging().value).toBe('0')
    fireEvent.change(averaging(), { target: { value: '100' } })
    fireEvent.click(within(detector()).getByRole('button', { name: 'Avg' }))
    fireEvent.click(within(resolution()!).getByRole('button', { name: '47 Hz' }))
    expect(loadScaleSettings('cw')).toEqual({ ...scaleDefaults('cw'), averageMs: 100, detector: 'average', window: 'fast' })
    expect(averaging().value).toBe('100')
  })
})

// @vitest-environment jsdom
// Units is on the Remote writable allow-list while the rest of Operator & radio is read-only from
// a browser, so it is the one control in this section that must stay operable under `readOnly`.
//
// The trap this guards, and why the guard is worth having: the section is a `<fieldset disabled>`,
// and per HTML a form control inside a disabled fieldset is disabled WHATEVER its own `disabled`
// attribute says — a nested fieldset cannot re-enable it either. So `disabled={unitsLocked ??
// readOnly}` on the select was inert: it read `false` and the control was still dead. The fix is
// structural (Units renders outside the disabled group), so the test has to be structural too:
// it asks the DOM whether the control is actually disabled, not what its attribute says.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { SettingsStation } from './SettingsStation'
import type { Settings } from '../types'

afterEach(cleanup)

const form = {
  mycall: 'KD9TAW', gridsquare: 'EN61', opName: 'Op', licenseClass: 'general',
  units: 'auto', dialMhz: 14.074, band: '20m', sideband: 'USB',
} as unknown as Settings

function mount(props: { readOnly?: boolean; unitsLocked?: boolean }) {
  const onUpdate = vi.fn()
  render(<SettingsStation form={form} error={null} bandPlan={[]} onUpdate={onUpdate}
    onSetFreq={vi.fn()} {...props} />)
  const units = screen.getByLabelText('Units') as HTMLSelectElement
  // The station fields wrap their input in a <label> that also holds the hint, so their accessible
  // name is not the bare word. The callsign input is found by the value it is rendered with.
  const callsign = screen.getByDisplayValue('KD9TAW')
  return { onUpdate, units, callsign }
}
/** What the browser would actually do, not what the attribute claims. */
const dead = (el: Element) => el.matches(':disabled')

it('a Remote browser that may write units can change it, while the station fields stay read-only', () => {
  const mounted = mount({ readOnly: true, unitsLocked: false })
  const { onUpdate, units } = mounted
  // Positive control: the section really is read-only, so a pass here cannot be "nothing is disabled".
  expect(dead(mounted.callsign), 'a station field must still be read-only').toBe(true)
  expect(dead(units), 'units must be operable when the browser may write it').toBe(false)
  fireEvent.change(units, { target: { value: 'imperial' } })
  expect(onUpdate).toHaveBeenCalledWith('units', 'imperial')
})

it('units is read-only when the browser may not write it, and when there is no Remote at all', () => {
  const locked = mount({ readOnly: true, unitsLocked: true })
  expect(dead(locked.units), 'units off the allow-list').toBe(true)
  cleanup()

  // Off Remote the panel passes no `unitsLocked`; `readOnly` alone then decides, as it always did.
  const readOnly = mount({ readOnly: true })
  expect(dead(readOnly.units), 'readOnly with no unitsLocked').toBe(true)
  cleanup()

  const open = mount({})
  expect(dead(open.units), 'the desktop panel').toBe(false)
  expect(dead(open.callsign), 'the desktop panel').toBe(false)
})

// The section keeps its identity: the settings registry, the deep link and the generated manual
// all resolve `#settings-operator-radio`, and Units must still be inside it.
it('keeps the section id, its legend and Units inside it', () => {
  mount({ readOnly: true, unitsLocked: false })
  const section = document.querySelector('#settings-operator-radio') as HTMLElement
  expect(section, 'the section id must survive the restructure').toBeTruthy()
  expect(section.classList.contains('settings-section')).toBe(true)
  expect(section.querySelector('legend')?.textContent).toBeTruthy()
  expect(within(section).getByLabelText('Units')).toBeTruthy()
  expect(within(section).getByDisplayValue('KD9TAW'), 'the station fields stay in the section').toBeTruthy()
})

// @vitest-environment jsdom
//
// #409: −B4 can hide only the stations worked on THIS band. "Worked before" on any band is what
// the chip has always hidden, and it still does by default; the choice beside it keeps a station
// worked only on another band on screen, so a new band slot can still be chased with −B4 on.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { loadDecodeHideB4Scope } from '../operateFilters'
import { t } from '../i18n'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

const rows = [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31', worked: false }),
  // Worked on 40 m, never on this band.
  decode({ from: 'PD2BS', message: 'CQ PD2BS JO21', freqHz: 900, worked: true, workedBand: false }),
  // Worked here.
  decode({ from: 'JA1XYZ', message: 'CQ JA1XYZ PM95', freqHz: 1500, worked: true, workedBand: true }),
]

function mount(props: Partial<React.ComponentProps<typeof OperateDecodes>> = {}) {
  return render(
    <OperateDecodes
      decodes={rows}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
      {...props}
    />,
  )
}

const scope = () =>
  screen.getByRole('combobox', { name: t('operate.decodes.hideB4Scope.aria') }) as HTMLSelectElement
const shown = (call: string) => screen.queryAllByText(new RegExp(call)).length > 0

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('−B4 on this band only (#409)', () => {
  it('defaults to any band, as −B4 always was', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    mount()
    expect(scope().value).toBe('any')
    expect(shown('W1AW')).toBe(true)
    expect(shown('PD2BS'), 'worked on another band hides under the default').toBe(false)
    expect(shown('JA1XYZ')).toBe(false)
  })

  it('"This band" keeps a station worked only on another band', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    mount()
    fireEvent.change(scope(), { target: { value: 'band' } })
    expect(shown('W1AW')).toBe(true)
    expect(shown('PD2BS'), 'worked only on 40 m must show on 20 m').toBe(true)
    expect(shown('JA1XYZ'), 'worked on this band still hides').toBe(false)
  })

  it('changes nothing while −B4 is off', () => {
    mount()
    fireEvent.change(scope(), { target: { value: 'band' } })
    for (const call of ['W1AW', 'PD2BS', 'JA1XYZ']) expect(shown(call), call).toBe(true)
  })

  it('persists, and an unknown stored value reads as any band', () => {
    mount()
    fireEvent.change(scope(), { target: { value: 'band' } })
    expect(loadDecodeHideB4Scope()).toBe('band')
    cleanup()
    mount()
    expect(scope().value).toBe('band')
    cleanup()
    localStorage.setItem('nexus.decodes.hideB4Scope', 'mode')
    expect(loadDecodeHideB4Scope()).toBe('any')
  })

  it('the Rx Frequency pane follows a change made in Band Activity, live', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    // The Rx Frequency pane shows only what is near the RX offset: put PD2BS there.
    const rxRows = [decode({ from: 'PD2BS', message: 'CQ PD2BS JO21', freqHz: 1200, worked: true, workedBand: false })]
    render(
      <OperateDecodes
        decodes={rxRows}
        slot={100}
        rxOffsetHz={1200}
        band="20m"
        tier="FT8"
        harqRescues={0}
        onCall={() => {}}
        title="Rx Frequency"
        lockedFilter="rx"
        compact
      />,
    )
    expect(shown('PD2BS')).toBe(false)
    mount({ decodes: [] })
    fireEvent.change(scope(), { target: { value: 'band' } })
    expect(shown('PD2BS'), 'the Rx Frequency pane kept the old scope').toBe(true)
  })

  it("the Rx Frequency pane's −B4 says when it hides only this band's", () => {
    const rxPane = () =>
      render(
        <OperateDecodes
          decodes={[]}
          slot={100}
          rxOffsetHz={1200}
          band="20m"
          tier="FT8"
          harqRescues={0}
          onCall={() => {}}
          title="Rx Frequency"
          lockedFilter="rx"
          compact
        />,
      )
    const chip = () => screen.getByRole('button', { name: '−B4' })
    rxPane()
    expect(screen.queryByRole('combobox'), 'premise: the pane has no scope menu').toBeNull()
    expect(chip().title, 'control: any band reads as −B4 always did').toBe(t('operate.decodes.hideB4.title'))
    cleanup()
    localStorage.setItem('nexus.decodes.hideB4Scope', 'band')
    rxPane()
    expect(chip().title).toBe(t('operate.decodes.hideB4.title.band'))
  })
})

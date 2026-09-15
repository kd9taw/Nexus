// @vitest-environment jsdom
//
// #287 — TUNE HAS ONE FIXED PLACE: RIGHT AFTER THE BAND PICKER.
//
// The report: on a busy screen, band hopping with an auto-ATU, finding Tune was a game of
// "where's Waldo", because it sat at the end of the right-pinned actions cluster and slid
// sideways with whatever each mode put in front of it (Operate's depth chips and ⊞ Panels,
// the power slider, the TX pill, the amplifier strip). A band change is exactly when you tune,
// so the operator's ask — and WSJT-X's layout — is Tune beside the band control.
//
// What is pinned, structurally rather than by pixel (jsdom does not lay out; the geometry is
// checked in the headless-Chrome harness):
//   · Tune is in the frequency cluster, immediately after the band control — nothing a cockpit
//     injects (`frequencyExtras`, `children`, `actions`, `power`) can sit between them;
//   · the rig's ATU stays Tune's sibling (CockpitHeader.atu.test.tsx: it appears exactly where
//     Tune does);
//   · Stop TX and the CAT pill stay in the actions cluster. Only Tune moves.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), haltTx: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const snap = {
  radio: { dialMhz: 14.29, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true, atu: true },
} as unknown as AppSnapshot

function mount() {
  return render(
    <CockpitHeader
      snap={snap}
      modeIndicator={<span>Phone</span>}
      bandControl={<span data-testid="band">20m</span>}
      frequencyExtras={<span data-testid="extras">step</span>}
      actions={<button type="button">Depth</button>}
      power={{ value: 50, unit: '%', onChange: () => {} }}
      onTune={() => {}}
      onAtuTune={() => {}}
      onStopTx={() => {}}
    >
      <button type="button">Mode extra</button>
    </CockpitHeader>,
  )
}

afterEach(cleanup)

describe('#287 Tune sits beside the band picker', () => {
  it('Tune follows the band control directly, inside the frequency cluster', () => {
    mount()
    const tune = screen.getByRole('button', { name: 'Tune' })
    const band = screen.getByTestId('band').closest('.ch-band')!
    expect(tune.closest('.ch-freq'), 'Tune is not in the frequency cluster').not.toBeNull()
    expect(tune.closest('.ch-actions'), 'Tune is still in the right-pinned actions cluster').toBeNull()
    // The very next sibling of the band slot is Tune's own slot.
    expect(band.nextElementSibling?.contains(tune), 'something sits between the band control and Tune').toBe(true)
  })

  it('the ATU stays beside Tune', () => {
    mount()
    const tune = screen.getByRole('button', { name: 'Tune' })
    const atu = screen.getByRole('button', { name: 'ATU' })
    expect(atu.parentElement).toBe(tune.parentElement)
  })

  it('Stop TX and the CAT pill stay in the actions cluster', () => {
    const { container } = mount()
    expect(screen.getByRole('button', { name: 'Stop TX' }).closest('.ch-actions')).not.toBeNull()
    expect(container.querySelector('.cockpit-cat')?.closest('.ch-actions')).not.toBeNull()
  })

  it('a cockpit that passes no Tune gets no empty Tune slot', () => {
    const { container } = render(
      <CockpitHeader snap={snap} modeIndicator={<span>FT8</span>} bandControl={<span>20m</span>} />,
    )
    expect(container.querySelector('.ch-tune')).toBeNull()
  })
})

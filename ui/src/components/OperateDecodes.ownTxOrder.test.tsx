// @vitest-environment jsdom
//
// The operator's own over lists AFTER the decode it answered, in both orientations of Band
// Activity. A station's RR73 heard in period P-1 reaches the pane a second or two into P; the 73
// sent in P is stamped with its transmit second, which can be earlier than that arrival. Ordered
// by those two clocks the 73 drew ABOVE the RR73 it replied to.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'
import { EN } from '../i18n'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const P = 100
const START = P * 15_000

const rr73: DecodeRow = {
  from: 'W9XYZ',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'KD9TAW W9XYZ RR73',
  isCq: false,
  directedToMe: true,
  worked: false,
  tier: 'FT8',
  rv: 0,
}
const my73: DecodeRow = {
  from: 'KD9TAW',
  snr: 0,
  dtSec: 0,
  freqHz: 1500,
  message: 'W9XYZ KD9TAW 73',
  isCq: false,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: -1,
  mine: true,
  txAt: START / 1000,
}

function Pane(props: { decodes: DecodeRow[]; slot: number }) {
  return (
    <OperateDecodes
      decodes={props.decodes}
      slot={props.slot}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
    />
  )
}

/** The RR73 arrives 1.8 s into P; the 73 went out at P+0 and shows on the next poll. */
function mountExchange() {
  vi.setSystemTime(START + 1_800)
  const view = render(<Pane decodes={[rr73]} slot={P} />)
  vi.setSystemTime(START + 2_000)
  view.rerender(<Pane decodes={[rr73, my73]} slot={P} />)
  return view
}

const messages = () =>
  Array.from(document.querySelectorAll('.decode-row')).map((r) => r.querySelector('.decode-msg')?.getAttribute('title'))

beforeEach(() => {
  localStorage.clear()
  vi.useFakeTimers({ toFake: ['Date'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('Band Activity: an own over lists after the decode it answered', () => {
  it('oldest first: the RR73, then the 73', () => {
    mountExchange()
    expect(messages()).toEqual(['KD9TAW W9XYZ RR73', 'W9XYZ KD9TAW 73'])
  })

  it('newest on top: the 73, then the RR73', () => {
    mountExchange()
    fireEvent.click(screen.getByRole('button', { name: EN['operate.decodes.newestTop.label'] }))
    expect(messages()).toEqual(['W9XYZ KD9TAW 73', 'KD9TAW W9XYZ RR73'])
  })
})

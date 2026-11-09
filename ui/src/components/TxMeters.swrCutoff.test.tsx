// @vitest-environment jsdom
//
// #476: THE SWR TOOLTIP NAMES THE OPERATOR'S OWN CUTOFF. "Keep it under 2:1" is a fixed rule of
// thumb; an operator who has turned on "Stop transmitting when SWR is high" and set their own
// number was being told a different number by the meter than the one Nexus will actually act on.
//
// The cutoff only exists where the engine arms it: the switch on AND a verified SWR scale
// (`Settings::swr_stop_ratio`). Each case below is paired with the render it differs from.
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { act } from 'react'
import { TxMeters } from './TxMeters'
import { setSwrCutoffMirror, readSwrCutoff } from '../swrCutoff'
import type { RadioStatus } from '../types'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

function radio(swrScaleVerified: boolean): RadioStatus {
  return { rigKeyed: true, txSwr: 1.8, swrScaleVerified } as unknown as RadioStatus
}
const title = (root: HTMLElement) => root.querySelector('.ph-txmeter')?.getAttribute('title') ?? ''

describe('the SWR tooltip follows the operator’s cutoff (#476)', () => {
  it('CONTROL: cutoff off, the rule of thumb stays', () => {
    setSwrCutoffMirror({ swrStopEnabled: false, swrStopThreshold: 3 })
    expect(title(render(<TxMeters radio={radio(true)} />).container)).toContain('2:1')
  })

  it('cutoff on: the tooltip names the operator’s number and where it is set', () => {
    setSwrCutoffMirror({ swrStopEnabled: true, swrStopThreshold: 3 })
    const t = title(render(<TxMeters radio={radio(true)} />).container)
    expect(t).toContain('3.0:1')
    expect(t).not.toContain('2:1')
    expect(t).toContain('Transmit limits')
  })

  it('follows a save without a remount', () => {
    setSwrCutoffMirror({ swrStopEnabled: true, swrStopThreshold: 3 })
    const { container } = render(<TxMeters radio={radio(true)} />)
    act(() => setSwrCutoffMirror({ swrStopEnabled: true, swrStopThreshold: 2.2 }))
    expect(title(container)).toContain('2.2:1')
  })

  it('an unverified rig never shows a cutoff: the engine will not arm one there', () => {
    setSwrCutoffMirror({ swrStopEnabled: true, swrStopThreshold: 3 })
    const t = title(render(<TxMeters radio={radio(false)} />).container)
    expect(t).not.toContain('3.0:1')
    expect(t).toContain('no verified scale')
  })

  it('clamped on load: a hand-edited value outside 1-99 or garbage reads as the engine reads it', () => {
    localStorage.setItem('nexus-swr-cutoff', '0')
    expect(readSwrCutoff()).toBe(1)
    localStorage.setItem('nexus-swr-cutoff', '500')
    expect(readSwrCutoff()).toBe(99)
    localStorage.setItem('nexus-swr-cutoff', 'abc')
    expect(readSwrCutoff()).toBeNull()
    localStorage.setItem('nexus-swr-cutoff', '')
    expect(readSwrCutoff()).toBeNull()
  })
})

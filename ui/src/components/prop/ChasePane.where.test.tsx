// @vitest-environment jsdom
//
// A CHASE ROW SAYS WHERE TO TUNE, NOT ONLY WHICH BAND. A need heard on a cluster or RBN spot carries its
// frequency and the mode it was heard in; both chase boxes printed only "20m", so the operator had to open
// the spot to know where on 20 m. A row with a spot frequency now reads "14.074 FT8" (a no-break space
// between them, so a narrow box wraps the line around the pair, never through it); a row with none (a
// DXpedition card, a band-level need) still reads its band.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { NeedAlert } from '../../types'
import type { PaneContext } from '../connect/paneContext'
import { ChasePane } from './ChasePane'
import { ChaseFeedPane } from './ChaseFeedPane'

vi.mock('../../api', () => ({ rotatorBearingToCall: () => Promise.reject(new Error('unset')) }))

function need(over: Partial<NeedAlert>): NeedAlert {
  return {
    call: 'FK8DX', entity: 'New Caledonia', band: '20m', zone: 32, tags: ['NewEntity'], priority: 90,
    headline: 'needed', mode: 'Digital', exactMode: 'FT8', freqMhz: 14.074,
    ...over,
  } as NeedAlert
}

function ctx(...alerts: NeedAlert[]): PaneContext {
  return {
    myGrid: 'JO21EV', entityCentroids: new Map(), needAlerts: alerts, bandOutlook: null, prop: null,
    dxpedWindows: new Map(), onSelectCall: () => {},
  } as unknown as PaneContext
}

const lines = () => [...document.querySelectorAll<HTMLElement>('.chase-open')].map((el) => el.textContent)

afterEach(cleanup)

describe('a chase row names the frequency and mode it was heard on', () => {
  it('the Chase box reads 14.074 FT8 for a spot heard there', () => {
    render(<ChasePane ctx={ctx(need({}))} />)
    expect(lines()).toEqual(['14.074\u00a0FT8'])
  })

  it('the Chase feed reads heard on 14.074 FT8', () => {
    render(<ChaseFeedPane ctx={ctx(need({}))} />)
    expect(lines()).toEqual(['heard on 14.074\u00a0FT8'])
  })

  it('a spot whose source named only the class reads its class after the frequency', () => {
    render(<ChasePane ctx={ctx(need({ mode: 'CW', exactMode: null, freqMhz: 14.0256 }))} />)
    expect(lines()).toEqual(['14.026\u00a0CW'])
  })

  it('a need with no spot frequency still reads its band', () => {
    render(<ChaseFeedPane ctx={ctx(need({ freqMhz: null }))} />)
    expect(lines()).toEqual(['heard on 20m'])
  })
})

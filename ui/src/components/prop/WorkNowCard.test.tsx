// @vitest-environment jsdom
// FIELD REPORT 2026-08-05: every WORK NOW card read "Best shot: 60m" — including
// cards whose own band was 12/15/20/30/40/80/160 m. Two separate wrongs in one line:
// the window sweep ranks every HF band on the PATH (so a per-band card was headlined
// with another band), and it never receives the expedition's ANNOUNCED band list (so
// the band it named may be one the operation never runs — 60 m earns no DXCC credit
// and most DXpeditions skip it).
//
// The rule these pin: a card headlines with ITS OWN band or with nothing.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { WorkNowCard } from './WorkNowCard'
import type { BandOutlook, DxpedWindow, WorkableCard } from '../../types'

afterEach(cleanup)

function outlook(band: string, over: Partial<BandOutlook> = {}): BandOutlook {
  return {
    band,
    workability: 'Good',
    score: 0.6,
    window: '0200–0400Z',
    grayline: false,
    hourly: Array(24).fill(0.6),
    reliability: 50,
    ...over,
  }
}

function card(over: Partial<WorkableCard> = {}): WorkableCard {
  return {
    call: 'J38K',
    entity: 'Grenada',
    need: 'Atno',
    band: '20m',
    bearingDeg: 150,
    octant: 'SSE',
    distanceKm: 4284,
    status: 'WorkNow',
    likelihood: 'Good',
    likelihoodScore: 0.6,
    liveConfirmed: false,
    howToCall: 'Standard FT8 — call at your offset',
    windowHint: 'best 0300–0500Z',
    priority: 100,
    modes: ['FT8'],
    ...over,
  }
}

function win(over: Partial<DxpedWindow> = {}): DxpedWindow {
  return {
    call: 'J38K',
    engine: 'heuristic',
    best: '60m Excellent 2258–1128Z',
    outlook: [outlook('60m', { workability: 'Excellent', window: '2258–1128Z', score: 0.94 })],
    ...over,
  }
}

describe('WorkNowCard best-shot line', () => {
  it('headlines the card with its OWN band, not the path-best band', () => {
    render(
      <WorkNowCard
        card={card({ band: '20m' })}
        window={win({
          outlook: [
            outlook('60m', { workability: 'Excellent', window: '2258–1128Z' }),
            outlook('20m', { workability: 'Fair', window: '1400–1600Z' }),
          ],
        })}
      />,
    )
    const line = screen.getByText(/Best shot:/).textContent ?? ''
    expect(line).toContain('20m')
    expect(line).toContain('1400–1600Z')
    // The reported bug: a 20m card naming 60m.
    expect(line).not.toContain('60m')
    expect(line).not.toContain('2258–1128Z')
  })

  // The fallback matters MORE than the happy path: the backend truncates the outlook
  // to 4 bands, so a card's own band is routinely absent. Showing another band's
  // window under this band's heading is the whole bug in miniature.
  it('falls back to the per-band windowHint when its own band is absent', () => {
    render(
      <WorkNowCard
        card={card({ band: '15m', windowHint: 'no opening in next 24 h' })}
        window={win({ outlook: [outlook('60m'), outlook('40m'), outlook('80m')] })}
      />,
    )
    expect(screen.getByText('no opening in next 24 h')).toBeTruthy()
    expect(screen.queryByText(/Best shot:/)).toBeNull()
    // and it must not have leaked ANY other band's window into the card.
    expect(document.body.textContent).not.toContain('0200–0400Z')
  })

  it('still shows the per-band line when 60m IS the card band', () => {
    // 60 m is demoted from headlines, never hidden — an operation that announces it
    // still gets an honest window on its own card.
    render(
      <WorkNowCard
        card={card({ band: '60m' })}
        window={win({ outlook: [outlook('60m', { window: '0100–0300Z' })] })}
      />,
    )
    const line = screen.getByText(/Best shot:/).textContent ?? ''
    expect(line).toContain('60m')
    expect(line).toContain('0100–0300Z')
  })

  it('shows the plain window hint when there is no window at all', () => {
    render(<WorkNowCard card={card({ windowHint: 'open now' })} />)
    expect(screen.getByText('open now')).toBeTruthy()
  })
})

// @vitest-environment jsdom
//
// The TV page: transport + chrome.
//
// Two surfaces, tested at their honest boundaries:
//   1. THE TRANSPORT — with `__NEXUS_TV_RPC__` set, api.ts's invoke() must go over
//      HTTP with the same command names and args the desktop bridge uses, and a 404
//      must surface as a rejection (the allowlist refusing is "feed unavailable",
//      the same degradation every pane already handles offline).
//   2. THE CHROME — ConnectTv's own bar and its props contract with ConnectView.
//      ConnectView itself is stubbed: it is the desktop's own component with its own
//      coverage, and jsdom cannot lay it out anyway (the layout harness owns that).
//      What must not regress HERE is what the TV hands it: an EMPTY roster and an
//      EMPTY needs map — operating state stays off the LAN.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'

const seen = vi.hoisted(() => ({ props: [] as Array<Record<string, unknown>> }))
vi.mock('../components/ConnectView', () => ({
  ConnectView: (p: Record<string, unknown>) => {
    seen.props.push(p)
    return <div data-testid="connect-view" />
  },
}))

import { ConnectTv } from './ConnectTv'
import { getKpForecast } from '../api'

const PROP = { advisory: { headline: 'h', bands: [], banners: [] }, openings: [], source: 'live' }

function mockFetch(routes: Record<string, unknown>) {
  const calls: string[] = []
  vi.stubGlobal('fetch', (url: string) => {
    calls.push(String(url))
    const path = String(url).split('?')[0]
    const cmd = path.split('/').pop() ?? ''
    if (cmd in routes) {
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve(routes[cmd]),
        text: () => Promise.resolve(''),
      })
    }
    return Promise.resolve({ ok: false, status: 404, text: () => Promise.resolve('nope') })
  })
  return calls
}

beforeEach(() => {
  window.__NEXUS_TV_RPC__ = '/connect/rpc'
  seen.props.length = 0
})
afterEach(() => {
  delete window.__NEXUS_TV_RPC__
  vi.unstubAllGlobals()
  cleanup()
})

describe('the transport', () => {
  it('routes invoke over HTTP with the args URL-encoded as JSON', async () => {
    const calls = mockFetch({ get_kp_forecast: { points: [] } })
    const out = await getKpForecast()
    expect(out).toEqual({ points: [] })
    expect(calls[0]).toBe('/connect/rpc/get_kp_forecast')
  })

  it('surfaces a 404 as a rejection, never as fabricated data', async () => {
    mockFetch({})
    await expect(getKpForecast()).rejects.toThrow(/404/)
  })
})

describe('the chrome', () => {
  it('shows the station, the read-only chip, and hands ConnectView NO operating state', async () => {
    mockFetch({ get_propagation: PROP, tv_station: { call: 'KD9TAW', grid: 'EN52' } })
    render(<ConnectTv />)
    await waitFor(() => expect(screen.getByText('KD9TAW')).toBeTruthy())
    expect(screen.getByText('read-only')).toBeTruthy()
    await waitFor(() => expect(seen.props.length).toBeGreaterThan(0))
    const p = seen.props[seen.props.length - 1]
    // ⚠️ THE CONTRACT: the roster and the needs board are operating state and must
    // never reach the LAN page — empty here is the module threat model holding.
    expect(p.stations).toEqual([])
    expect((p.needByCall as Map<string, unknown>).size).toBe(0)
    // …and no work handler exists for ConnectView to render an affordance from.
    expect(p.onWorkSpot).toBeUndefined()
  })

  it('says "no link" instead of freezing on a stale screen', async () => {
    mockFetch({ tv_station: { call: 'KD9TAW', grid: 'EN52' } }) // get_propagation 404s
    render(<ConnectTv />)
    await waitFor(() => expect(screen.getByText('no link to Nexus')).toBeTruthy())
  })
})

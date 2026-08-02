import { describe, it, expect } from 'vitest'

import { tleRefreshMessage } from './tleMessages'
import type { TleStatus } from '../api'

const NOW = 1_900_000_000

/** A landed mirror refresh; tests override the fields under trial. */
function status(over: Partial<TleStatus>): TleStatus {
  return {
    count: 97,
    usableCount: 97,
    agingCount: 0,
    heldBackCount: 0,
    fetchedAt: NOW,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.3,
    blockedUntil: 0,
    ...over,
  }
}

describe('tleRefreshMessage', () => {
  // The one composer both the Update-now toast and the Settings "Last
  // refresh" line run — so the case table lives here, once. The standing
  // rule across every case: the HEADLINE speaks operator; HTTP codes and
  // fetch internals ride `raw` (tooltip material) only.

  it('a landed mirror refresh: plain success', () => {
    const m = tleRefreshMessage(status({}))
    expect(m.kind).toBe('success')
    expect(m.text).toBe('Orbital elements updated — 97 birds (mirror)')
    expect(m.raw).toBeUndefined()
  })

  it('mirror down, Celestrak fetched (the manual escalation): says so, with the count', () => {
    const m = tleRefreshMessage(status({ source: 'celestrak' }))
    expect(m.kind).toBe('success')
    expect(m.text).toBe('Mirror unreachable — fetched from Celestrak: 97 birds')
    expect(m.raw).toBeUndefined()
  })

  // THE PRE-LAUNCH REALITY: the mirror endpoint 404s until the next site
  // release ships it. With current elements that is not a failure the
  // operator can or should act on — the copy says exactly that, and the age.
  it("mirror down, floor not met, elements current: honest and calm — not an error", () => {
    const m = tleRefreshMessage(
      status({
        lastError: 'TLE mirror fetch failed: HTTP 404',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.kind).toBe('info')
    expect(m.text).toBe(
      "The element mirror isn't reachable (it goes live with the next release); your elements are 0.3 d old — current.",
    )
    expect(m.text).not.toMatch(/HTTP|404/i)
    expect(m.raw).toBe('TLE mirror fetch failed: HTTP 404')
  })

  // THE FIELD REPORT (KD9TAW, 2026-08-01): the shipped catalog is current —
  // median 0.2 d, 337 birds usable — while a slow-cadence tail (AO-7 and the
  // SatNOGS birds re-observed every few weeks) puts 30 birds past the 30 d
  // ceiling. That must reach the CALM branch, and the excluded birds must be
  // accounted for rather than smuggled into the age.
  it('the shipped catalog: current, with a tail sitting out — calm, and the tail is named', () => {
    const m = tleRefreshMessage(
      status({
        count: 367,
        usableCount: 337,
        heldBackCount: 30,
        elementAgeDays: 0.2,
        lastError: 'TLE mirror fetch failed: HTTP 404',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.kind).toBe('info')
    expect(m.text).toBe(
      "The element mirror isn't reachable (it goes live with the next release); your elements are 0.2 d old — current. 30 birds are past 30 days and sit out until fresh elements arrive.",
    )
    // The old readout reported the oldest usable bird (26 d) as the operator's
    // element age: an error headline, and nothing to act on.
    expect(m.text).not.toMatch(/stale|import a fresh/i)
  })

  // THE CLAIM THE CHANGELOG MAKES — "counted and reported in their own
  // right" — has to hold on every branch, not only the two the pre-launch
  // mirror happens to take. `lastErrorKind: 'celestrakBlocked'` and a LANDED
  // refresh (the state the day the mirror goes live) named nothing.
  it('every branch accounts for the birds sitting out', () => {
    const held = { count: 367, usableCount: 337, heldBackCount: 30 }
    const sitOut = /30 birds are past 30 days and sit out until fresh elements arrive\./
    expect(tleRefreshMessage(status(held)).text).toMatch(sitOut)
    expect(tleRefreshMessage(status({ ...held, source: 'celestrak' })).text).toMatch(sitOut)
    expect(
      tleRefreshMessage(
        status({ ...held, blockedUntil: NOW + 20 * 3600, lastError: 'x', lastErrorKind: 'celestrakBlocked' }),
      ).text,
    ).toMatch(sitOut)
    expect(
      tleRefreshMessage(status({ ...held, lastError: 'x', lastErrorKind: 'failed' })).text,
    ).toMatch(sitOut)
  })

  // The 14–30 d band is the shape the median cannot show: the typical bird is
  // current AND half the catalog is drifting, both true at once.
  it('the drifting band is named beside the age, not folded into it', () => {
    const m = tleRefreshMessage(
      status({
        count: 140,
        usableCount: 100,
        agingCount: 49,
        heldBackCount: 40,
        elementAgeDays: 0.2,
        lastError: 'TLE mirror fetch failed: HTTP 404',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.text).toBe(
      "The element mirror isn't reachable (it goes live with the next release); your elements are 0.2 d old — current. 49 birds are past the 14-day line and drifting. 40 birds are past 30 days and sit out until fresh elements arrive.",
    )
  })

  it('one bird sitting out reads as one bird', () => {
    const m = tleRefreshMessage(
      status({
        agingCount: 1,
        heldBackCount: 1,
        lastError: 'TLE mirror fetch failed: HTTP 404',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.text).toContain('1 bird is past the 14-day line and drifting.')
    expect(m.text).toContain('1 bird is past 30 days and sits out until fresh elements arrive.')
  })

  it('mirror down with STALE elements: a real failure, with the age and the way out', () => {
    const m = tleRefreshMessage(
      status({
        elementAgeDays: 16.4,
        lastError: 'TLE mirror fetch failed: network: dns error',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.kind).toBe('error')
    expect(m.text).toBe(
      'The element mirror is unreachable and your elements are 16 d old — import a fresh element file or retry later.',
    )
    expect(m.raw).toBe('TLE mirror fetch failed: network: dns error')
  })

  it('mirror down with NOTHING usable cached: the import escape hatch is the headline', () => {
    const m = tleRefreshMessage(
      status({
        usableCount: 0,
        elementAgeDays: null,
        lastError: 'TLE mirror fetch failed: HTTP 404',
        lastErrorKind: 'mirrorUnreachable',
      }),
    )
    expect(m.kind).toBe('error')
    expect(m.text).toBe(
      'The element mirror is unreachable and no usable elements are cached — import an element file to get the satellite surfaces running.',
    )
  })

  it('403-blocked: the existing blocked message, HTTP code demoted to the tooltip', () => {
    const raw =
      'TLE mirror fetch failed: HTTP 404; Celestrak refused (HTTP 403) — direct fetches stopped for 24 h; the mirror keeps retrying'
    const m = tleRefreshMessage(
      status({
        blockedUntil: NOW + 20 * 3600,
        lastError: raw,
        lastErrorKind: 'celestrakBlocked',
      }),
    )
    expect(m.kind).toBe('error')
    expect(m.text).toBe(
      'Celestrak refused direct element fetches — direct attempts are stopped for 24 h; the mirror keeps retrying.',
    )
    expect(m.text).not.toMatch(/HTTP|403/i)
    expect(m.raw).toBe(raw)
  })

  it('genuinely failed (both legs / refused set): what failed and what to do', () => {
    const raw = 'TLE mirror fetch failed: HTTP 404; Celestrak TLE fetch failed: network: timeout'
    const m = tleRefreshMessage(status({ lastError: raw, lastErrorKind: 'failed' }))
    expect(m.kind).toBe('error')
    expect(m.text).toBe(
      'Element update failed — no source delivered a usable set; retry shortly or import an element file.',
    )
    expect(m.text).not.toMatch(/HTTP/i)
    expect(m.raw).toBe(raw)
  })

  it('a raw error with no kind (defensive) reads as genuinely failed', () => {
    const m = tleRefreshMessage(status({ lastError: 'mirror TLE set refused: only 12 birds' }))
    expect(m.kind).toBe('error')
    expect(m.raw).toBe('mirror TLE set refused: only 12 birds')
  })
})

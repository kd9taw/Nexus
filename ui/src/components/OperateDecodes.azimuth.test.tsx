// @vitest-environment jsdom
//
// The beam heading on a Band Activity row — the surface the tester asked for ("the AZ
// azimuth in degrees after the country name").
//
// Three states have to be visible in one file, because each one is only meaningful
// beside the others: a decode that carried a grid gets an exact heading, one that did
// not gets the entity centre marked `~`, and one we cannot place at all gets NOTHING.
// The last is the assertion that could pass for the wrong reason — a pane rendering no
// headings at all would satisfy it — so every negative case here is asserted in the
// same render, or the same file, as a positive one that must be present.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'

// cty.dat names verbatim — the centroid lookup is an exact-string join on the same
// `country` value a decode carries, so a fixture that said "Germany" would test a
// join the shipping code never performs.
vi.mock('../api', () => ({
  openQrzPage: vi.fn(),
  getDxccEntityLocations: vi.fn(async () => [
    ['Fed. Rep. of Germany', 51.0, 10.0],
    ['France', 46.5, 2.5],
  ]),
}))

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

/** A German CQ that CARRIED its grid — the exact-heading case. */
const withGrid = decode({
  from: 'DL1ABC',
  message: 'CQ DL1ABC JO31',
  freqHz: 800,
  country: 'Fed. Rep. of Germany',
  grid: 'JO31',
})

/** A French station heard in mid-QSO traffic — a country, but no grid on the wire. */
const noGrid = decode({
  from: 'F5XYZ',
  message: 'F5XYZ W1AW -12',
  freqHz: 1500,
  country: 'France',
  isCq: false,
})

/** A country cty.dat has no coordinate for — nothing can be pointed at. */
const unplaceable = decode({
  from: 'XX1ZZ',
  message: 'XX1ZZ W1AW -09',
  freqHz: 1700,
  country: 'Nowhereland',
  isCq: false,
})

function mount(props: Partial<React.ComponentProps<typeof OperateDecodes>> = {}) {
  return render(
    <OperateDecodes
      decodes={[withGrid, noGrid, unplaceable]}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
      myGrid="EN52"
      {...props}
    />,
  )
}

/** The heading shown on the row whose message names `call`, or null if it has none. */
function azimuthOf(call: string): string | null {
  const row = screen
    .queryAllByRole('option')
    .find((r) => (r.textContent ?? '').includes(call))
  if (!row) throw new Error(`no row for ${call} — the fixture is not rendering`)
  return row.querySelector('.decode-az')?.textContent ?? null
}

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('Band Activity puts a heading after the country', () => {
  it('shows an exact heading for a decode that carried a grid', async () => {
    mount()
    await waitFor(() => expect(azimuthOf('DL1ABC')).toMatch(/^\d+°$/))
    // Plain, not `~`: this one was measured to the station's own square.
    expect(azimuthOf('DL1ABC')).not.toContain('~')
  })

  it('falls back to the entity centre, marked ~, when no grid was heard', async () => {
    mount()
    await waitFor(() => expect(azimuthOf('F5XYZ')).toMatch(/^~\d+°$/))
  })

  it('renders the heading right after the country, in that order', async () => {
    const { container } = mount()
    await waitFor(() => expect(container.querySelector('.decode-az')).not.toBeNull())
    const row = screen.queryAllByRole('option').find((r) => (r.textContent ?? '').includes('DL1ABC'))!
    const country = row.querySelector('.decode-country')!
    const az = row.querySelector('.decode-az')!
    // Node.DOCUMENT_POSITION_FOLLOWING — the heading comes after the country name,
    // which is the whole of what was asked for.
    expect(country.compareDocumentPosition(az) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('says short path, and says an approximate one is only the country’s centre', async () => {
    mount()
    await waitFor(() => expect(azimuthOf('F5XYZ')).not.toBeNull())
    const row = screen.queryAllByRole('option').find((r) => (r.textContent ?? '').includes('F5XYZ'))!
    const title = row.querySelector('.decode-az')!.getAttribute('title') ?? ''
    expect(title).toContain('short path')
    expect(title).toContain('France')
    expect(title).toContain('not to this station')
  })
})

describe('Band Activity shows no heading rather than a wrong one', () => {
  it('leaves a row blank when its country has no coordinate — while its neighbours do not', async () => {
    mount()
    // The control FIRST: if these two were also blank the assertion below would pass
    // on a pane that had simply failed to render any heading at all.
    await waitFor(() => expect(azimuthOf('DL1ABC')).not.toBeNull())
    expect(azimuthOf('F5XYZ')).not.toBeNull()
    expect(azimuthOf('XX1ZZ')).toBeNull()
  })

  it('shows no heading anywhere when the operator has not set a grid', async () => {
    // The negative: same three decodes, no origin to measure from.
    const { container } = mount({ myGrid: '' })
    // Wait for the centroid table to land, so this is "no origin" and not "not yet
    // loaded" — otherwise the assertion would pass before the feature could act.
    // Counted off `.decode-row`, not the `option` role: the pane's filter controls are
    // native <select>s whose <option>s carry that role too.
    await waitFor(() => expect(container.querySelectorAll('.decode-row').length).toBe(3))
    expect(container.querySelectorAll('.decode-az').length).toBe(0)

    // The control, in the same test: with a grid set and nothing else changed, two of
    // the same three rows DO get a heading. The blank above is the missing grid.
    cleanup()
    const withHome = mount({ myGrid: 'EN52' })
    await waitFor(() =>
      expect(withHome.container.querySelectorAll('.decode-az').length).toBe(2),
    )
  })

  it('shows no heading before the centroid table arrives, then fills the gridless rows in', async () => {
    const { container } = mount()
    // The grid-derived heading needs no table and is there immediately; the centroid
    // ones cannot be, which is why the fallback rows must not flash a wrong number.
    expect(container.querySelectorAll('.decode-az').length).toBe(1)
    await waitFor(() => expect(container.querySelectorAll('.decode-az').length).toBe(2))
  })
})

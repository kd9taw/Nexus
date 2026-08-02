// @vitest-environment jsdom
//
// Band Activity under the country exclusion. The four behaviours that make this a filter an
// experienced chaser can trust rather than a why-is-the-band-empty trap:
//   1. an excluded entity's CQ goes away,
//   2. the station we are IN A QSO WITH never does,
//   3. a genuinely needed one still surfaces (needed outranks excluded),
//   4. the hiding is VISIBLE and one click from off.
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { COUNTRY_EXCLUDE_KEY } from '../features/countryExclude'
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

/** A German CQ — the entity `cty.dat` calls "Fed. Rep. of Germany", not "Germany". */
const dl = (over: Partial<DecodeRow> = {}) =>
  decode({
    from: 'DL1ABC',
    message: 'CQ DL1ABC JO31',
    freqHz: 800,
    country: 'Fed. Rep. of Germany',
    ...over,
  })

const fr = (over: Partial<DecodeRow> = {}) =>
  decode({ from: 'F5XYZ', message: 'CQ F5XYZ JN18', freqHz: 1500, country: 'France', ...over })

function mount(props: Partial<React.ComponentProps<typeof OperateDecodes>> = {}) {
  return render(
    <OperateDecodes
      decodes={[dl(), fr()]}
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

/** The decode rows actually on screen, by the callsign each one shows. */
const callsShown = () =>
  screen
    .queryAllByRole('option')
    .map((r) => r.textContent ?? '')
    .filter(Boolean)

const showsCall = (call: string) => callsShown().some((t) => t.includes(call))

/** Germany ticked, as the operator would leave it. */
const excludeGermany = () => localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl"]')

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('an excluded country leaves Band Activity', () => {
  it('shows both CQs when nothing is excluded', () => {
    mount()
    expect(showsCall('DL1ABC')).toBe(true)
    expect(showsCall('F5XYZ')).toBe(true)
  })

  it('drops the excluded entity and keeps everyone else', () => {
    excludeGermany()
    mount()
    expect(showsCall('DL1ABC')).toBe(false)
    expect(showsCall('F5XYZ')).toBe(true)
  })

  it('matches the RESOLVED entity, never a prefix of the callsign', () => {
    // `DL` as a text prefix would also swallow this US call, and would MISS a German
    // operator signing DA/DB/DJ/DK. The row's `country` is what cty.dat resolved.
    excludeGermany()
    mount({ decodes: [decode({ from: 'DL7ZZ', country: 'United States', message: 'CQ DL7ZZ FN31' })] })
    expect(showsCall('DL7ZZ')).toBe(true)
  })

  it('keeps a decode whose callsign never resolved to an entity', () => {
    excludeGermany()
    mount({ decodes: [decode({ from: '1ABC', country: null, message: 'CQ 1ABC' })] })
    expect(showsCall('1ABC')).toBe(true)
  })
})

describe('the exclusion never reaches the QSO', () => {
  it('keeps the station we are working, mid-exchange', () => {
    excludeGermany()
    mount({
      decodes: [dl({ message: 'KD9TAW DL1ABC -12', isCq: false })],
      // The Tx panel's DX call — OperateCockpit passes `selectedCall={dxCall || null}`.
      selectedCall: 'DL1ABC',
    })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('keeps a message addressed to us from an excluded country', () => {
    excludeGermany()
    mount({ decodes: [dl({ message: 'KD9TAW DL1ABC JO31', isCq: false, directedToMe: true })] })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('keeps our OWN transmission', () => {
    excludeGermany()
    mount({
      decodes: [dl({ from: 'KD9TAW', message: 'DL1ABC KD9TAW EN61', mine: true, isCq: false })],
    })
    expect(showsCall('KD9TAW')).toBe(true)
  })
})

describe('needed outranks excluded', () => {
  it('still shows an all-time new entity from an excluded country', () => {
    excludeGermany()
    mount({ decodes: [dl({ newDxcc: true })] })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('still shows a new band slot from an excluded country', () => {
    excludeGermany()
    mount({ decodes: [dl({ newBand: true })] })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('does NOT keep a merely-new GRID — that would defeat the filter in a dense country', () => {
    // The operator's constraint was density: in a country you have worked for years, an
    // unworked GRID is routine, so exempting it would leave the pane exactly as full as
    // before. New ENTITY and new BAND SLOT are the claims worth interrupting an exclusion.
    excludeGermany()
    mount({ decodes: [dl({ newGrid: true })] })
    expect(showsCall('DL1ABC')).toBe(false)
  })
})

describe('the hiding is visible, and one click from off', () => {
  it('says nothing when nothing is hidden', () => {
    mount()
    expect(screen.queryByTestId('od-hidden')).toBeNull()
  })

  it('counts the COUNTRIES hidden, not the rows', () => {
    localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl","f","ea"]')
    mount()
    expect(screen.getByTestId('od-hidden').textContent).toContain('3 countries hidden')
  })

  it('says "1 country hidden" for a single one', () => {
    excludeGermany()
    mount()
    expect(screen.getByTestId('od-hidden').textContent).toContain('1 country hidden')
  })

  it('clears every exclusion in one click, and the rows come straight back', () => {
    excludeGermany()
    mount()
    expect(showsCall('DL1ABC')).toBe(false)
    fireEvent.click(screen.getByRole('button', { name: /clear country filter/i }))
    expect(showsCall('DL1ABC')).toBe(true)
    expect(screen.queryByTestId('od-hidden')).toBeNull()
    expect(localStorage.getItem(COUNTRY_EXCLUDE_KEY)).toBe('[]')
  })

  it('keeps the per-pane heard count honest — it counts what is SHOWN', () => {
    excludeGermany()
    mount()
    expect(screen.getByText('1 heard')).toBeTruthy()
  })

})

describe('the Rx Frequency pane is deliberately NOT filtered', () => {
  // Band Activity answers "who is on the band that I want to work" — a chase list, and the
  // right place to thin out countries. The Rx Frequency pane answers "what is happening on
  // MY operating frequency", which is situational awareness: an excluded-country station
  // sitting on top of us is exactly the thing we must see to understand why we are being
  // covered up. Hiding it would make our own frequency read as clear when it is not.
  const rxPane = { compact: true as const, lockedFilter: 'rx' as const, title: 'Rx Frequency' }

  it('shows an excluded country that is on our RX frequency', () => {
    excludeGermany()
    mount({ ...rxPane, rxOffsetHz: 800, hideExcludedCountries: false })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('shows no hidden-count chip, because it hides nothing', () => {
    excludeGermany()
    mount({ ...rxPane, rxOffsetHz: 800, hideExcludedCountries: false })
    expect(screen.queryByRole('button', { name: 'All' })).toBeNull()
    expect(screen.queryByTestId('od-hidden')).toBeNull()
  })
})

describe('the picker', () => {
  // Radix Popper observes its elements with a ResizeObserver jsdom lacks (the shim
  // portal-zoom.test.tsx uses for the same reason).
  beforeAll(() => {
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver
  })

  /** Open the menu from the keyboard — a Radix trigger opens on pointerdown or Enter, not
   *  on a synthetic click, and the keyboard path is the one worth pinning anyway. */
  const openMenu = () => {
    fireEvent.keyDown(screen.getByRole('button', { name: /countries/i }), { key: 'Enter' })
    return screen.getByRole('menu')
  }

  it('offers the 18 countries, labelled with their prefixes', () => {
    mount()
    const items = within(openMenu()).getAllByRole('menuitemcheckbox')
    expect(items).toHaveLength(18)
    const labels = items.map((i) => i.textContent)
    // The prefix is operator vocabulary; the entity behind it is what matches.
    expect(labels).toContain('Germany (DL)')
    expect(labels).toContain('United States (K/W/N)')
    expect(labels).toContain('Slovenia (S5)')
    expect(labels).toContain('Mexico (XE)')
    expect(labels).toContain('Canada (VE)')
  })

  it('ticks a country and hides it, without closing the menu', () => {
    mount()
    const menu = openMenu()
    expect(showsCall('DL1ABC')).toBe(true)
    fireEvent.click(within(menu).getByRole('menuitemcheckbox', { name: 'Germany (DL)' }))
    expect(localStorage.getItem(COUNTRY_EXCLUDE_KEY)).toBe('["dl"]')
    expect(showsCall('DL1ABC')).toBe(false)
    // The operator is choosing a SET — a tick must not dismiss the picker.
    expect(screen.queryByRole('menu')).not.toBeNull()
  })

  it('reports the ticked state on the checkbox itself', () => {
    excludeGermany()
    mount()
    const menu = openMenu()
    expect(
      within(menu).getByRole('menuitemcheckbox', { name: 'Germany (DL)' }).getAttribute('aria-checked'),
    ).toBe('true')
    expect(
      within(menu).getByRole('menuitemcheckbox', { name: 'France (F)' }).getAttribute('aria-checked'),
    ).toBe('false')
  })
})

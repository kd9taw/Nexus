// @vitest-environment jsdom
//
// The Call Roster under the country exclusion. Same rules as Band Activity — they share one
// predicate precisely so the two panes cannot drift on what "protected" means — plus the
// roster's own honesty requirement: its row count must state what is SHOWN.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { COUNTRY_EXCLUDE_KEY } from '../features/countryExclude'
import type { NeedAlert, NeedTag, Station } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn(), getDeclination: vi.fn().mockResolvedValue(0) }))

const station = (over: Partial<Station> = {}): Station => ({
  call: 'W1AW',
  grid: 'FN31',
  snr: -10,
  lastHeardSlot: 100,
  heardCount: 1,
  presence: 'active',
  worked: false,
  ...over,
})

const dl = (over: Partial<Station> = {}) =>
  station({ call: 'DL1ABC', grid: 'JO31', country: 'Fed. Rep. of Germany', ...over })

const fr = (over: Partial<Station> = {}) =>
  station({ call: 'F5XYZ', grid: 'JN18', country: 'France', ...over })

/** A need alert on THIS roster's band and mode class, so `alertsForSurface` passes it
 *  through and the row's fate is decided by the country filter alone. */
const alert = (call: string, tags: NeedTag[]): NeedAlert => ({
  call,
  entity: 'Fed. Rep. of Germany',
  band: '20m',
  zone: 14,
  tags,
  priority: 50,
  headline: `${call} needed`,
  mode: 'FT8',
  freqMhz: 14.074,
})

function mount(props: Partial<React.ComponentProps<typeof OperateRoster>> = {}) {
  return render(
    <OperateRoster
      stations={[dl(), fr()]}
      myGrid="EN61"
      currentSlot={100}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
      {...props}
    />,
  )
}

const rowCalls = () =>
  screen
    .queryAllByRole('row')
    .map((r) => r.getAttribute('aria-label') ?? '')
    .filter(Boolean)

const showsCall = (call: string) => rowCalls().some((l) => l.startsWith(call))

const excludeGermany = () => localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl"]')

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('an excluded country leaves the roster', () => {
  it('lists both stations when nothing is excluded', () => {
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

  it('matches the RESOLVED entity, not a prefix of the callsign', () => {
    excludeGermany()
    mount({ stations: [station({ call: 'DL7ZZ', country: 'United States' })] })
    expect(showsCall('DL7ZZ')).toBe(true)
  })
})

describe('the roster never hides the QSO', () => {
  it('keeps the selected station — the one we are working', () => {
    excludeGermany()
    mount({ selectedCall: 'DL1ABC' })
    expect(showsCall('DL1ABC')).toBe(true)
  })
})

describe('needed outranks excluded, on the same two claims as Band Activity', () => {
  it('keeps a new ENTITY from an excluded country', () => {
    excludeGermany()
    mount({
      needAlertsByCall: new Map([['DL1ABC', [alert('DL1ABC', ['NewEntity'])]]]),
      band: '20m',
      feedMode: 'FT8',
    })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('keeps a new BAND slot from an excluded country', () => {
    excludeGermany()
    mount({
      needAlertsByCall: new Map([['DL1ABC', [alert('DL1ABC', ['NewBand'])]]]),
      band: '20m',
      feedMode: 'FT8',
    })
    expect(showsCall('DL1ABC')).toBe(true)
  })

  it('does NOT keep a mere Confirm — routine in a country you have worked for years', () => {
    excludeGermany()
    mount({
      needAlertsByCall: new Map([['DL1ABC', [alert('DL1ABC', ['Confirm'])]]]),
      band: '20m',
      feedMode: 'FT8',
    })
    expect(showsCall('DL1ABC')).toBe(false)
  })
})

describe('the roster says what it is hiding', () => {
  it('counts only the rows it SHOWS', () => {
    excludeGermany()
    mount()
    expect(screen.getByText('1', { selector: '.or-count' })).toBeTruthy()
  })

  it('shows the hidden-country chip, and clears it in one click', () => {
    excludeGermany()
    mount()
    expect(screen.getByTestId('or-hidden').textContent).toContain('1 country hidden')
    fireEvent.click(screen.getByRole('button', { name: /clear country filter/i }))
    expect(showsCall('DL1ABC')).toBe(true)
    expect(screen.queryByTestId('or-hidden')).toBeNull()
  })

  it('says nothing when nothing is excluded', () => {
    mount()
    expect(screen.queryByTestId('or-hidden')).toBeNull()
  })
})

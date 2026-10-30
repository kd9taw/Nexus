// @vitest-environment jsdom
//
// THE BAND ACTIVITY SEARCH BOX. Typing narrows the list to the decode lines that contain what was
// typed, matched without regard to case against the line as the pane draws it: the message
// (callsigns, grid, report, any CQ modifier), the country, and the columns beside it. ✕ or Esc
// empties the box and every line comes back.
//
// It is a DISPLAY filter and nothing else: a filtered row double-clicks exactly as the same row
// does unfiltered, the Rx Frequency pane (the same component, locked) never applies it, and a
// pane with no box (compact) never applies it either — a filter with no control on screen is
// how a list comes to look like a quiet band.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { useState } from 'react'
import { OperateDecodes } from './OperateDecodes'
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

const ROWS: DecodeRow[] = [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31', freqHz: 1200, country: 'United States' }),
  decode({ from: 'JA1XYZ', message: 'CQ DX JA1XYZ PM95', freqHz: 900, snr: -18, country: 'Japan' }),
  decode({ from: 'K1ABC', message: 'K1ABC DL1QQ R-07', freqHz: 1500, isCq: false, country: 'Germany' }),
  decode({ from: 'N0POT', message: 'CQ POTA N0POT EN34', freqHz: 2100, country: 'United States' }),
  decode({ from: 'G4XYZ', message: 'W9ZZZ G4XYZ IO91', freqHz: 2400, isCq: false, country: 'England' }),
]

function mount(props: Partial<React.ComponentProps<typeof OperateDecodes>> = {}) {
  return render(
    <OperateDecodes
      decodes={ROWS}
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

const box = () => screen.getByRole('searchbox', { name: 'Search Band Activity' })
const type = (text: string) => fireEvent.change(box(), { target: { value: text } })
/** The callsigns of the rows on screen, in drawing order. */
const calls = () =>
  Array.from(document.querySelectorAll('.decode-row')).map(
    (r) => (r.getAttribute('aria-label') ?? '').split(',')[0],
  )

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('the search box narrows Band Activity to the lines that match', () => {
  it('is in the full pane, and absent from compact panes (Rx Frequency, the Tempo rail)', () => {
    mount()
    expect(box()).toBeTruthy()
    cleanup()
    mount({ compact: true, lockedFilter: 'rx' })
    expect(screen.queryByRole('searchbox')).toBeNull()
    cleanup()
    mount({ compact: true })
    expect(screen.queryByRole('searchbox')).toBeNull()
  })

  it('shows every line while the box is empty', () => {
    mount()
    expect(calls()).toEqual(['W1AW', 'JA1XYZ', 'K1ABC', 'N0POT', 'G4XYZ'])
  })

  it('matches a callsign anywhere in the line, whatever the case — sender or addressee', () => {
    mount()
    type('dl1qq')
    expect(calls()).toEqual(['K1ABC'])
    type('w9zzz')
    expect(calls()).toEqual(['G4XYZ'])
  })

  it('matches a grid, a report and a CQ modifier', () => {
    mount()
    type('pm95')
    expect(calls()).toEqual(['JA1XYZ'])
    type('R-07')
    expect(calls()).toEqual(['K1ABC'])
    type('pota')
    expect(calls()).toEqual(['N0POT'])
    type('cq dx')
    expect(calls()).toEqual(['JA1XYZ'])
  })

  it('matches the country Nexus shows beside the message', () => {
    mount()
    type('japan')
    expect(calls()).toEqual(['JA1XYZ'])
    type('united')
    expect(calls()).toEqual(['W1AW', 'N0POT'])
  })

  it('several words must all match (a narrowing search), and a wildcard is a whole word', () => {
    mount()
    type('cq united')
    expect(calls()).toEqual(['W1AW', 'N0POT'])
    type('cq england')
    expect(calls()).toEqual([])
    type('JA*')
    expect(calls()).toEqual(['JA1XYZ'])
  })

  it('a line that does not match is gone, not dimmed', () => {
    mount()
    type('k1abc')
    expect(screen.queryByText('CQ W1AW FN31')).toBeNull()
    expect(screen.queryByText('K1ABC DL1QQ R-07')).not.toBeNull()
  })
})

describe('a filtered list says so, with a count', () => {
  it('shows "N of M match" while the box has text, and nothing while it is empty', () => {
    mount()
    expect(screen.queryByTestId('od-search-count')).toBeNull()
    type('united')
    expect(screen.getByTestId('od-search-count').textContent).toBe('2 of 5 match')
    type('')
    expect(screen.queryByTestId('od-search-count')).toBeNull()
  })

  it('counts against what the chips leave, not the raw history', () => {
    mount()
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    type('united')
    // CQ keeps W1AW, JA1XYZ and N0POT; two of those three match.
    expect(screen.getByTestId('od-search-count').textContent).toBe('2 of 3 match')
  })

  it('says the SEARCH is hiding everything when nothing matches, never "no decodes yet"', () => {
    mount()
    type('zz9zz')
    expect(screen.queryByText('No decodes yet')).toBeNull()
    expect(screen.getByText('Nothing matches “zz9zz”')).toBeTruthy()
    expect(screen.getByTestId('od-search-count').textContent).toBe('0 of 5 match')
  })
})

describe('clearing the search', () => {
  it('✕ empties the box and every line comes back', () => {
    mount()
    type('japan')
    expect(calls()).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: 'Clear the search' }))
    expect(box()).toHaveProperty('value', '')
    expect(calls()).toHaveLength(5)
  })

  it('there is no ✕ while the box is empty', () => {
    mount()
    expect(screen.queryByRole('button', { name: 'Clear the search' })).toBeNull()
  })

  it('Esc in the box empties it', () => {
    mount()
    type('japan')
    fireEvent.keyDown(box(), { key: 'Escape' })
    expect(box()).toHaveProperty('value', '')
    expect(calls()).toHaveLength(5)
  })

  it('other keys in the box do not clear it', () => {
    mount()
    type('japan')
    fireEvent.keyDown(box(), { key: 'Enter' })
    fireEvent.keyDown(box(), { key: 'F6' })
    expect(box()).toHaveProperty('value', 'japan')
  })
})

describe('the search is display only', () => {
  it('a filtered row double-clicks exactly as the same row does unfiltered', () => {
    const plain = vi.fn()
    mount({ onCall: plain })
    fireEvent.doubleClick(screen.getByText('K1ABC DL1QQ R-07'))
    cleanup()
    const searched = vi.fn()
    mount({ onCall: searched })
    type('dl1qq')
    fireEvent.doubleClick(screen.getByText('K1ABC DL1QQ R-07'))
    expect(searched).toHaveBeenCalledTimes(1)
    expect(searched.mock.calls[0]).toEqual(plain.mock.calls[0])
    expect(searched.mock.calls[0]).toEqual(['K1ABC', undefined, 'K1ABC DL1QQ R-07', -12, 1500])
  })

  it('a filtered row single-clicks (selects) exactly as unfiltered', () => {
    const plain = vi.fn()
    mount({ onSelectDecode: plain })
    fireEvent.click(screen.getByText('CQ DX JA1XYZ PM95'))
    cleanup()
    const searched = vi.fn()
    mount({ onSelectDecode: searched })
    type('japan')
    fireEvent.click(screen.getByText('CQ DX JA1XYZ PM95'))
    expect(searched.mock.calls).toEqual(plain.mock.calls)
  })

  it('Shift+Enter on a filtered row works THAT row (the keyboard addresses what is drawn)', () => {
    const onCall = vi.fn()
    mount({ onCall })
    type('england')
    const row = document.querySelector('.decode-row') as HTMLElement
    fireEvent.focus(row)
    fireEvent.keyDown(row, { key: 'Enter', shiftKey: true })
    expect(onCall).toHaveBeenCalledWith('G4XYZ', undefined, 'W9ZZZ G4XYZ IO91', -12, 2400)
  })

  it('a locked pane (Rx Frequency) ignores a search handed to it', () => {
    mount({ compact: true, lockedFilter: 'rx', search: 'zz9zz', rxOffsetHz: 1200 })
    expect(calls()).toEqual(['W1AW'])
  })

  it('new decodes keep arriving and are filtered as they land', () => {
    const { rerender } = mount()
    type('japan')
    const next = [
      decode({ from: 'JR2AAA', message: 'CQ JR2AAA PM85', freqHz: 700, country: 'Japan' }),
      decode({ from: 'F5ZZZ', message: 'CQ F5ZZZ JN18', freqHz: 1700, country: 'France' }),
    ]
    rerender(
      <OperateDecodes decodes={next} slot={101} rxOffsetHz={1200} band="20m" tier="FT8" harqRescues={0} onCall={() => {}} />,
    )
    expect(calls()).toEqual(['JA1XYZ', 'JR2AAA'])
    expect(screen.getByTestId('od-search-count').textContent).toBe('2 of 7 match')
  })
})

describe('where the search text lives', () => {
  it('is not written to storage (a restart opens on the whole band)', () => {
    mount()
    type('japan')
    expect(Object.keys(localStorage).filter((k) => /search|query/i.test(k))).toEqual([])
    expect(Object.keys(sessionStorage).filter((k) => /search|query/i.test(k))).toEqual([])
    cleanup()
    mount()
    expect(box()).toHaveProperty('value', '')
    expect(calls()).toHaveLength(5)
  })

  it('a host can own it (search / onSearch), so it survives the pane remounting', () => {
    function Host({ show }: { show: boolean }) {
      const [q, setQ] = useState('')
      return show ? (
        <OperateDecodes
          decodes={ROWS}
          slot={100}
          rxOffsetHz={1200}
          band="20m"
          tier="FT8"
          harqRescues={0}
          onCall={() => {}}
          search={q}
          onSearch={setQ}
        />
      ) : (
        <span>{q}</span>
      )
    }
    const { rerender } = render(<Host show />)
    type('japan')
    rerender(<Host show={false} />)
    rerender(<Host show />)
    expect(box()).toHaveProperty('value', 'japan')
    expect(calls()).toEqual(['JA1XYZ'])
  })
})

describe('the pinned scroll follows the search', () => {
  it('a new search re-pins a pane the operator had scrolled up in', () => {
    mount()
    const list = screen.getByRole('listbox')
    // jsdom lays nothing out: give the list a scrolled-up geometry so onScroll reads "reading".
    Object.defineProperty(list, 'scrollHeight', { configurable: true, value: 2000 })
    Object.defineProperty(list, 'clientHeight', { configurable: true, value: 200 })
    list.scrollTop = 100
    fireEvent.scroll(list)
    expect(screen.getByText(/reviewing/)).toBeTruthy()
    type('japan')
    expect(screen.queryByText(/reviewing/)).toBeNull()
  })
})

// @vitest-environment jsdom
//
// THE QSO DETAIL VIEW (#313) — "It's a 'Log List' not a Log Book… There's no View capability."
//
// The claims worth pinning are about what the pane REFUSES to show, not what it shows: a
// field the contact does not carry is omitted, never rendered blank, because a row of empty
// labels reads as "unknown" while meaning "not recorded". And the fields it exists for are
// the ones the 17-column table cannot reach.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { QsoDetail } from './QsoDetail'
import type { LoggedQso } from '../types'

afterEach(cleanup)

const qso = (over: Record<string, unknown> = {}): LoggedQso =>
  ({
    call: 'DL1ABC',
    grid: null,
    band: '20m',
    freqMhz: 14.074,
    mode: 'FT8',
    rstSent: null,
    rstRcvd: null,
    whenUnix: 1_782_583_500,
    confirmed: false,
    awardConfirmed: false,
    ...over,
  }) as unknown as LoggedQso

const shown = () => document.body.textContent ?? ''

describe('the QSO detail view', () => {
  it('shows what the table cannot — starting with the OTHER station\'s grid', () => {
    // The table has a `myGrid` column and no column for theirs, which is the one you want
    // when looking a contact up.
    render(<QsoDetail qso={qso({ grid: 'JO31AB', myGrid: 'EN52' })} onClose={() => {}} />)
    expect(shown()).toContain('JO31AB')
    expect(shown()).toContain('EN52')
  })

  it('omits a field the contact does not carry, rather than printing it blank', () => {
    // ⭐ THE CLAIM. "No grid recorded" and "grid unknown" are different facts and a blank
    // row states the second. Control below proves the label appears when there IS a value,
    // so its absence here is the omission and not a broken render.
    const { rerender } = render(<QsoDetail qso={qso({ grid: null })} onClose={() => {}} />)
    const withoutGrid = shown()
    rerender(<QsoDetail qso={qso({ grid: 'JO31AB' })} onClose={() => {}} />)
    expect(shown()).toContain('Their grid')
    expect(withoutGrid).not.toContain('Their grid')
  })

  it('drops a whole section when nothing in it is recorded', () => {
    // A bare FT8 contact has no QSL, no award credit and no park refs. None of those
    // headings should be on screen.
    render(<QsoDetail qso={qso()} onClose={() => {}} />)
    expect(shown()).not.toContain('QSL and awards')
    expect(shown()).not.toContain('Park and summit references')
  })

  it('surfaces ADIF fields Nexus kept from an import but models nowhere', () => {
    // These round-trip through the app and were previously invisible everywhere. They are
    // the operator's own data.
    render(
      <QsoDetail
        qso={qso({ extra: [['MY_ANTENNA', '3 el yagi'], ['SFI', '142']] })}
        onClose={() => {}}
      />,
    )
    expect(shown()).toContain('MY_ANTENNA')
    expect(shown()).toContain('3 el yagi')
    expect(shown()).toContain('SFI')
  })

  it('says a date-only contact has no time, instead of implying midnight', () => {
    // `timeKnown: false` means the source carried a date and no time. A bare 00:00 read as
    // fact is what leaves LoTW and eQSL holding those contacts unmatched forever.
    render(<QsoDetail qso={qso({ timeKnown: false })} onClose={() => {}} />)
    expect(shown()).toContain('date only')
    // Control: a contact WITH a time says no such thing.
    cleanup()
    render(<QsoDetail qso={qso({ timeKnown: true })} onClose={() => {}} />)
    expect(shown()).not.toContain('date only')
  })

  // ⚠️ NO TEST FOR "nothing recorded at all": that state is unreachable, because every
  // record carries `whenUnix` and so always has at least a time to show. The component
  // says so where the branch would have been, rather than carrying a branch no test can
  // reach.
  it('renders nothing at all when no contact is selected', () => {
    const { container } = render(<QsoDetail qso={null} onClose={() => {}} />)
    expect(container.textContent).toBe('')
    expect(document.body.textContent).toBe('')
  })
})

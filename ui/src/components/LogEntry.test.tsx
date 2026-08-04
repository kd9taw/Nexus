// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { fdLogManual, logQso, qrzLookup, lookupPark } from '../api'
import type { AppSnapshot, FieldDayStatus } from '../types'

// The FD log + standard-log seams matter here; the other api functions are imported by the
// component but never reached on these render paths, so stub them harmlessly.
vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

const mockedFdLog = vi.mocked(fdLogManual)
const mockedLogQso = vi.mocked(logQso)
const mockedQrz = vi.mocked(qrzLookup)
const mockedLookupPark = vi.mocked(lookupPark)

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

const fieldDay = {
  myClass: '',
  mySection: '',
  running: true,
  state: '',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
} as unknown as FieldDayStatus

function renderFd() {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={fieldDay}
      fdMode="PH"
    />,
  )
}

const call = () => screen.getByPlaceholderText('W1AW')
const klass = () => screen.getByPlaceholderText('1D')
const section = () => screen.getByPlaceholderText('WI')
const logBtn = () => screen.getByRole('button', { name: /log fd/i }) as HTMLButtonElement

beforeEach(() => {
  mockedFdLog.mockClear()
  mockedLogQso.mockClear()
})
afterEach(() => cleanup())

describe('LogEntry Field Day exchange gate', () => {
  it('blocks logging (button disabled, no fdLogManual) when the section is blank', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2a' } })
    // section left blank — the old code would have logged it as the literal '?'
    expect(logBtn().disabled).toBe(true)
    fireEvent.click(logBtn())
    expect(mockedFdLog).not.toHaveBeenCalled()
  })

  it('blocks logging when the section is not a real ARRL/RAC code', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2A' } })
    fireEvent.change(section(), { target: { value: 'ZZ' } })
    expect(logBtn().disabled).toBe(true)
    fireEvent.click(logBtn())
    expect(mockedFdLog).not.toHaveBeenCalled()
  })

  it('logs the real class + section once both are valid (never a "?" substitution)', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2a' } })
    fireEvent.change(section(), { target: { value: 'wi' } })
    expect(logBtn().disabled).toBe(false)
    fireEvent.click(logBtn())
    expect(mockedFdLog).toHaveBeenCalledWith('W1AW', '2A', 'WI', 'PH')
  })
})

describe('LogEntry standard variant — State + Country', () => {
  function renderStd() {
    render(
      <LogEntry
        snap={snap}
        mode="PH"
        defaultRst="59"
        exchange="terrestrial"
        fieldDay={null}
        fdMode={undefined}
      />,
    )
  }

  it('shows editable State and Country fields in the main area', () => {
    renderStd()
    // They were previously write-only: auto-filled from QRZ and visible only in the summary
    // line, so an operator who heard the state on air had to open the logbook to fix it.
    expect(screen.getByPlaceholderText('State')).toBeTruthy()
    expect(screen.getByPlaceholderText('Country')).toBeTruthy()
  })

  it('accepts operator edits to State and Country', () => {
    renderStd()
    const st = screen.getByPlaceholderText('State') as HTMLInputElement
    const co = screen.getByPlaceholderText('Country') as HTMLInputElement
    fireEvent.change(st, { target: { value: 'WI' } })
    fireEvent.change(co, { target: { value: 'United States' } })
    expect(st.value).toBe('WI')
    expect(co.value).toBe('United States')
  })
})

describe('LogEntry standard variant — the accidental-log guard', () => {
  // The operator logged several contacts by mistake reaching for the callbook lookup
  // (2026-08-02). Two mechanisms fed it: the lookup button sat in the SAME `.le-row` as Log,
  // and that row WRAPS (the log pane goes as narrow as 24em) — so Log's position on screen
  // moves with the pane width, and a mid-QSO glance lands on whatever is there. Committing
  // the contact now happens from its own row directly above the callsign card, away from
  // the field cluster the operator touches while working the station.
  function renderStd() {
    render(
      <LogEntry
        snap={snap}
        mode="SSB"
        defaultRst="59"
        exchange="terrestrial"
        onSpot={() => {}}
        fieldDay={null}
        fdMode={undefined}
      />,
    )
    // Three characters is what makes the callsign card (RecallPanel) render at all.
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1aw' } })
  }
  const logBtn = () => screen.getByRole('button', { name: 'Log' })
  const lookupBtn = () => screen.getByRole('button', { name: 'Lookup' })

  it('names the callbook button "Lookup" — it answers from HamQTH too, not only QRZ', () => {
    renderStd()
    expect(screen.queryByRole('button', { name: 'QRZ' })).toBeNull()
    expect(lookupBtn()).toBeTruthy()
  })

  it('commits from its own row: Log + Spot adjacent, above the callsign card', () => {
    renderStd()
    const log = logBtn()
    const spot = screen.getByRole('button', { name: /spot/i })
    // Kept side by side — the operator spots more because Spot sits beside Log.
    expect(log.parentElement).toBe(spot.parentElement)
    expect(log.nextElementSibling).toBe(spot)
    // Out of the row that holds the call field and the lookup button.
    expect(log.parentElement).not.toBe(lookupBtn().parentElement)
    // And above the callsign card, in the same scroll flow (no new scroll owner).
    const card = document.querySelector('.recall-card')
    expect(card).toBeTruthy()
    expect(log.compareDocumentPosition(card!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('gives Lookup none of the primary Log button styling', () => {
    renderStd()
    const log = logBtn()
    const lookup = lookupBtn()
    expect(log.classList.contains('le-log-btn')).toBe(true) // the accent-filled commit pill
    expect(lookup.classList.contains('le-log-btn')).toBe(false)
    // No shared class at all: the two cannot drift back into the same look.
    const shared = [...lookup.classList].filter((c) => log.classList.contains(c))
    expect(shared).toEqual([])
  })
})

describe('LogEntry standard variant — other-radio override (band/freq/mode/UTC time)', () => {
  // snap.radio is the LIVE (HF) rig: 20m / 14.2 MHz. mode="SSB" is the cockpit's live mode.
  function renderStd() {
    render(
      <LogEntry
        snap={snap}
        mode="SSB"
        defaultRst="59"
        exchange="terrestrial"
        fieldDay={null}
        fdMode={undefined}
      />,
    )
  }
  const overrideToggle = () => screen.getByRole('button', { name: /another radio/i })
  const logBtn = () => screen.getByRole('button', { name: 'Log' })

  it('logs the hand-entered band / freq / mode / UTC time when the override is open', () => {
    renderStd()

    // Opt in, then set a contact made on the 2 m rig that Nexus can't see.
    fireEvent.click(overrideToggle())
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '2m' } })
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '146.520' } })
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'FM' } })
    fireEvent.change(screen.getByLabelText('Date (UTC)'), { target: { value: '2026-03-15' } })
    fireEvent.change(screen.getByLabelText('Time (UTC)'), { target: { value: '14:30' } })
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'k9xyz' } })

    fireEvent.click(logBtn())

    // The exact UTC instant — NOT a local-zone reading of the inputs, NOT "now".
    const expectedWhen = Math.floor(Date.UTC(2026, 2, 15, 14, 30, 0) / 1000)
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    expect(mockedLogQso).toHaveBeenCalledWith(
      expect.objectContaining({
        call: 'K9XYZ',
        band: '2m',
        freqMhz: 146.52,
        mode: 'FM',
        whenUnix: expectedWhen,
      }),
    )
    // Proves the live-rig defaults were genuinely overridden, not merely added alongside.
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).not.toBe('20m')
    expect(rec.freqMhz).not.toBe(14.2)
  })

  it('picking a band fills a consistent in-band frequency (never "2m band / 14.2 MHz")', () => {
    renderStd()
    fireEvent.click(overrideToggle())
    // Open seeds from the live 20 m rig; switching the band must move the frequency with it.
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '2m' } })
    expect((screen.getByLabelText('Freq (MHz)') as HTMLInputElement).value).toBe('146.52')
    // And typing a frequency snaps the band to the plan it lands in.
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '446.000' } })
    expect((screen.getByLabelText('Band') as HTMLSelectElement).value).toBe('70cm')
  })

  it('closed override (the common flow) still logs the live rig + now, unchanged', () => {
    renderStd()
    const before = Math.floor(Date.now() / 1000)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1aw' } })
    fireEvent.click(logBtn())
    const after = Math.floor(Date.now() / 1000)

    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).toBe('20m')
    expect(rec.freqMhz).toBe(14.2)
    expect(rec.mode).toBe('SSB')
    expect(rec.whenUnix).toBeGreaterThanOrEqual(before)
    expect(rec.whenUnix).toBeLessThanOrEqual(after)
  })

  it('BLOCKS logging a mismatched record when the override is open but the freq is invalid', () => {
    // Review finding: with the override open and the freq cleared, band+freq once fell back to
    // the live rig while mode+time stayed the override — logging e.g. 20m/14.2 tagged FM at a
    // past time. It must refuse to log at all until the freq is valid or the override is closed.
    renderStd()
    fireEvent.click(overrideToggle())
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'FM' } })
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '' } }) // fumbled
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'k9xyz' } })

    fireEvent.click(logBtn())
    expect(mockedLogQso).not.toHaveBeenCalled() // no mismatched record written

    // Fix the frequency → it logs, fully consistent (2 m, in-band freq, FM).
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '146.520' } })
    fireEvent.click(logBtn())
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).toBe('2m')
    expect(rec.freqMhz).toBe(146.52)
    expect(rec.mode).toBe('FM')
  })

  it('states what will be logged without an empty slot when the dial has no band', () => {
    // `bandplan::band_for_dial` stops at 23 cm, so QO-100 (10.489 GHz) and the
    // IC-905 microwave birds arrive with band === '' — and the satellite path
    // now TUNES them rather than refusing, which is what makes this reachable.
    // Printing the slot anyway read "as SSB ·  · 10489.550 MHz": a separator
    // with a hole in it. The frequency is the truth either way.
    const noBand = {
      radio: { band: '', dialMhz: 10489.55 },
      hunt: null,
    } as unknown as AppSnapshot
    render(
      <LogEntry
        snap={noBand}
        mode="SSB"
        defaultRst="59"
        exchange="terrestrial"
        fieldDay={null}
        fdMode={undefined}
      />,
    )
    const hint = screen.getByText(/Logs to the shared logbook/).textContent ?? ''
    expect(hint).toContain('as SSB · 10489.550 MHz')
    expect(hint).not.toMatch(/·\s+·/)

    // …and a dial the plan CAN name still names it.
    cleanup()
    renderStd()
    expect(screen.getByText(/Logs to the shared logbook/).textContent).toContain(
      'as SSB · 20m · 14.200 MHz',
    )
  })

  it('offers no USB/LSB in the mode picker — those are ADIF submodes TQSL rejects as a MODE', () => {
    renderStd()
    fireEvent.click(overrideToggle())
    const modes = Array.from(
      (screen.getByLabelText('Mode') as HTMLSelectElement).options,
      (o) => o.value,
    )
    expect(modes).toContain('SSB')
    expect(modes).not.toContain('USB')
    expect(modes).not.toContain('LSB')
  })
})

// ---------------------------------------------------------------------------
// THE EXCHANGE — one prop, and the one thing it decides.
//
// Operator, 0.28.1, on the Satellites section: "that section still has a
// pota/sota section, which is shouldnt and what about the sat, gridsquares
// features to log".
//
// `LogEntry` is shared by three consumers (Phone, CW, Satellites) and staying
// shared is the constraint ("Lets not reinvent anything"). So the consumer names
// its EXCHANGE and the strip renders that — no fork, and no pile of optional
// booleans. TWO fields hang off it, and they are the two halves of one sentence
// about what the stations pass each other: terrestrial has a park reference and
// no grid, satellite has a grid and no park.
describe('LogEntry — the exchange each consumer asks for', () => {
  function renderAs(exchange: 'terrestrial' | 'satellite') {
    render(
      <LogEntry
        snap={snap}
        mode="SSB"
        defaultRst="59"
        exchange={exchange}
        fieldDay={null}
        fdMode={undefined}
      />,
    )
  }

  it('terrestrial (Phone + CW): the POTA/SOTA row is part of the exchange', () => {
    // Hunting an activator IS part of the exchange on HF/VHF: the park reference
    // goes to ADIF (POTA→SIG_INFO, SOTA→SOTA_REF). Nothing here changed.
    renderAs('terrestrial')
    expect(document.querySelector('.le-park-row'), 'the park row went missing').not.toBeNull()
    expect(document.querySelector('.le-park-prog')).not.toBeNull()
    expect(screen.getByPlaceholderText(/^Park \(/)).toBeTruthy()
  })

  it('satellite: the park row is NOT ASKED FOR — not hidden, not rendered', () => {
    // There is no park on a bird. "Not asked for" rather than "hidden": the
    // section passes an exchange that has no park in it, so there is no picker,
    // no reference search and no park-detail chip to suppress — and no vertical
    // space taken in a column the operator shares with the sky dome.
    renderAs('satellite')
    expect(document.querySelector('.le-park-row'), 'the park row is still rendered').toBeNull()
    expect(document.querySelector('.le-park-prog')).toBeNull()
    expect(document.querySelector('.le-park-search')).toBeNull()
    expect(screen.queryByPlaceholderText(/^Park \(/)).toBeNull()
    expect(screen.queryByPlaceholderText(/^Summit \(/)).toBeNull()
  })

  it('everything else about the strip is identical across the two exchanges', () => {
    // The proof this is one shared component and not a fork: the call field, the
    // reports, the callbook lookup, the notes, the other-radio override and the
    // commit row are the same in both. Only the two exchange fields differ.
    const SURFACE = [
      '.le-call',
      '.le-rst',
      '.le-name',
      '.le-qth',
      '.le-state',
      '.le-country',
      '.le-comment',
      '.le-notes',
      '.le-lookup',
      '.le-override-toggle',
      '.le-log-btn',
    ]
    const surface = () => SURFACE.filter((s) => document.querySelector(s) != null)

    renderAs('terrestrial')
    const terrestrial = surface()
    expect(terrestrial, 'the shared surface itself moved').toEqual(SURFACE)
    cleanup()
    renderAs('satellite')
    expect(surface()).toEqual(terrestrial)
  })

  it('the QTH row is BYTE-FOR-BYTE the same four fields in both exchanges', () => {
    // The height claim's structural half. Grid was first put in this row, where
    // it does not fit: at the Satellites column's real width the four fields are
    // a single full line, so a fifth pushed it to two (+52 px) and made the
    // satellite strip TALLER than the park row it had just removed (−48). It
    // moved to the exchange row, where the wrap leaves slack. Nothing here may
    // change without re-measuring — the numbers are in the CHANGELOG.
    const qthRow = () =>
      Array.from(document.querySelectorAll('.le-row'))
        .find((r) => r.querySelector('.le-qth'))!
        .querySelectorAll('input, select, textarea')
    for (const ex of ['terrestrial', 'satellite'] as const) {
      renderAs(ex)
      expect(
        Array.from(qthRow(), (n) => n.className.split(/\s+/).find((c) => c.startsWith('le-'))),
        `the QTH row changed in the ${ex} exchange`,
      ).toEqual(['le-qth', 'le-state', 'le-country', 'le-comment'])
      cleanup()
    }
  })

  it('satellite: a pending hunt fires no park lookup — not even the live directory fetch', async () => {
    // A pending POTA hunt still PREFILLS the park reference in every exchange
    // (the hunt chip stays: the engine tags by callsign whichever strip logs the
    // contact). Without the exchange guard on that effect, the Satellites section
    // would debounce a local lookup and then a LIVE POTA-directory fetch — network
    // traffic mid-pass for a chip it does not render.
    const hunted = {
      radio: { band: '20m', dialMhz: 14.2 },
      hunt: { program: 'POTA', reference: 'K-1234', call: 'W1AW' },
    } as unknown as AppSnapshot
    const renderHunt = (exchange: 'terrestrial' | 'satellite') =>
      render(
        <LogEntry
          snap={hunted}
          mode="SSB"
          defaultRst="59"
          exchange={exchange}
          fieldDay={null}
          fdMode={undefined}
        />,
      )

    // Positive control first: terrestrial DOES look the prefilled park up, which
    // also proves the 250 ms debounce has fired by the time we check the other.
    mockedLookupPark.mockClear()
    renderHunt('terrestrial')
    await waitFor(() => expect(mockedLookupPark).toHaveBeenCalledWith('K-1234'))
    cleanup()

    mockedLookupPark.mockClear()
    renderHunt('satellite')
    await new Promise((r) => setTimeout(r, 400)) // well past the 250 ms debounce
    expect(mockedLookupPark, 'the satellite exchange looked a park up anyway').not.toHaveBeenCalled()
  })
})

describe('LogEntry — the grid the other station passed you', () => {
  // GAP 1. `logGrid` has always reached the RECORD and printed in the summary
  // line and the recall card — but it could only ever be written by a callbook
  // lookup. There was no input, so the square a station passed ON AIR could not
  // be entered at all. On a bird that is most of the contact: grid-for-grid IS
  // the satellite exchange, and Satellite VUCC is scored on it.
  //
  // THE SATELLITE EXCHANGE ONLY, for now. The case for Phone and CW is good and
  // is NOT settled against — a wrong callbook square is uncorrectable there
  // today, and those are the cockpits that meet the rovers and /P activations
  // whose square is wrong most often. What stops it here is height, not merit:
  // the field costs each of those strips a wrapped line (+52 px measured), and
  // the operator's instruction for this change was "keep focused on sat". The
  // open question is recorded in the CHANGELOG and in the `exchange` prop doc.
  function renderStd(exchange: 'terrestrial' | 'satellite' = 'satellite') {
    render(
      <LogEntry
        snap={snap}
        mode="SSB"
        defaultRst="59"
        exchange={exchange}
        fieldDay={null}
        fdMode={undefined}
      />,
    )
  }
  const gridInput = () => screen.getByPlaceholderText('Grid') as HTMLInputElement
  const callInput = () => screen.getByPlaceholderText('Call') as HTMLInputElement
  const logBtn = () => screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement

  beforeEach(() => {
    mockedQrz.mockReset()
    mockedQrz.mockResolvedValue(null as never)
  })

  it('is asked for in the satellite exchange and nowhere else', () => {
    renderStd('satellite')
    expect(gridInput(), 'no Grid input on a bird, where the grid IS the exchange').toBeTruthy()
    cleanup()
    renderStd('terrestrial')
    expect(
      screen.queryByPlaceholderText('Grid'),
      'Phone and CW grew a Grid field — that is +52 px on two strips nobody asked us to touch',
    ).toBeNull()
  })

  it('sits in the EXCHANGE row, beside the reports — not in the QTH row', () => {
    // Where it goes is the whole of the height fix, so it is pinned at the DOM
    // and not left to the eye. Beside the reports because that is what it IS on
    // a bird: something the other operator says in the same breath as the
    // report, read and typed at the same moment — and because that row already
    // wraps, so the field lands in slack instead of buying a new line. In the
    // QTH row it bought one (measured: 44 → 96 px at the section's real width).
    renderStd('satellite')
    const rows = Array.from(document.querySelectorAll('.le-row'))
    const gridRow = rows.find((r) => r.contains(gridInput()))!
    expect(gridRow.querySelector('.le-call'), 'Grid is not in the exchange row').not.toBeNull()
    expect(gridRow.querySelector('.le-rst'), 'Grid is not beside the reports').not.toBeNull()
    expect(gridRow.querySelector('.le-qth'), 'Grid landed back in the QTH row').toBeNull()
    expect(rows.indexOf(gridRow), 'the exchange row is the first row').toBe(0)
  })

  it('the typed square reaches the RECORD, not just the screen', () => {
    // Asserting on what `logQso` receives. The display is not what LoTW reads.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    fireEvent.change(gridInput(), { target: { value: 'fn31pr' } })
    fireEvent.click(logBtn())
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    expect(mockedLogQso.mock.calls[0][0].grid).toBe('FN31PR')
  })

  it('uppercases as typed, so case is never why a locator is refused', () => {
    renderStd()
    fireEvent.change(gridInput(), { target: { value: 'en52' } })
    expect(gridInput().value).toBe('EN52')
  })

  it('a blank grid is no error — the record simply carries none', () => {
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    expect(logBtn().disabled).toBe(false)
    fireEvent.click(logBtn())
    expect(mockedLogQso.mock.calls[0][0].grid).toBeNull()
  })

  it('REFUSES the commit on a malformed locator, and names the forms it takes', () => {
    // A QSO record is permanent and cannot be repaired after upload, and a wrong
    // square is a wrong VUCC credit — so "EN5" does not go in. Refused at the
    // commit rather than silently dropped: dropping a value the operator can see
    // on screen makes the screen lie. Same shape as the two gates this component
    // already has (the FD section code, the override frequency).
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    fireEvent.change(gridInput(), { target: { value: 'EN5' } })
    expect(logBtn().disabled, 'a malformed locator still commits').toBe(true)
    fireEvent.click(logBtn())
    expect(mockedLogQso).not.toHaveBeenCalled()

    // And it SAYS which forms it takes — a disabled button with no reason is a
    // dead end mid-pass.
    const why = document.querySelector('.le-grid-warn')
    expect(why, 'nothing tells the operator why Log went dead').not.toBeNull()
    expect(why!.textContent).toMatch(/EN52/)
    expect(why!.textContent).toMatch(/EN52XA/)
    expect(why!.textContent, 'the message still names only 4 and 6').toMatch(/EN52XA25/)
    // The field itself is marked, so the eye lands on it and not on the button.
    expect(gridInput().classList.contains('invalid')).toBe(true)

    // Fix it → it commits, carrying the square.
    fireEvent.change(gridInput(), { target: { value: 'EN52' } })
    expect(logBtn().disabled).toBe(false)
    expect(document.querySelector('.le-grid-warn')).toBeNull()
    fireEvent.click(logBtn())
    expect(mockedLogQso.mock.calls[0][0].grid).toBe('EN52')
  })

  it('ENTER does not commit a malformed locator either — the keyboard is the real path', () => {
    // The disabled button is the visible half of the gate; `logIt`'s own early
    // return is the load-bearing half, and it had no test. Enter reaches `logIt`
    // straight from any field (`onEnter`), never touching the button's
    // `disabled`, so with that block deleted every other assertion here still
    // passes while EN5 goes into the log — and Enter is how an operator commits
    // mid-pass, one hand on the rotator. Pinned on the keyboard, at the field.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    fireEvent.change(gridInput(), { target: { value: 'EN5' } })
    fireEvent.keyDown(gridInput(), { key: 'Enter' })
    expect(mockedLogQso, 'Enter committed a malformed locator').not.toHaveBeenCalled()
    // Same key, same field, once the square is a square.
    fireEvent.change(gridInput(), { target: { value: 'EN52' } })
    fireEvent.keyDown(gridInput(), { key: 'Enter' })
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    expect(mockedLogQso.mock.calls[0][0].grid).toBe('EN52')
  })

  it('takes every length ADIF carries — 4, 6 AND 8 — and refuses the rest', () => {
    // `isValidLoggedGrid` (ui/src/grid.ts), the RECORD-side ruling. It is the
    // same alphabet as `isValidGrid` with the extended pair on the end, not a
    // second parser, and the two are deliberately different questions:
    // `isValidGrid` still gates the operator's OWN square at 4/6 (the setup
    // wizard, the programming workbench), because that is what Nexus stores.
    // A square you were PASSED goes to ADIF's GRIDSQUARE, which carries 8 — and
    // 8 is what the VHF/microwave and satellite operators who pass grids use.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    for (const bad of ['EN5', 'EN52X', '1234', 'ZZ99', 'EN52YZ', 'EN52XA9', 'EN52XA99X']) {
      fireEvent.change(gridInput(), { target: { value: bad } })
      expect(logBtn().disabled, `${bad} was accepted`).toBe(true)
    }
    for (const good of ['EN52', 'EN52XA', 'RR73', 'IO91WM', 'FN31PR99', 'EN52XA25']) {
      fireEvent.change(gridInput(), { target: { value: good } })
      expect(logBtn().disabled, `${good} was refused`).toBe(false)
    }
    fireEvent.change(gridInput(), { target: { value: 'FN31PR99' } })
    fireEvent.click(logBtn())
    expect(mockedLogQso.mock.calls[0][0].grid, 'the 8-char square was dropped').toBe('FN31PR99')
  })

  it('a callbook square NEVER holds the Log button — the operator did not type it', async () => {
    // THE STRAND. QRZ answers `grid` with whatever the operator put in their
    // profile: an 8-character extended locator (probed live: FN31PR99, now
    // accepted), but also a rover's "EN52/EN53" and plain free text. The strip
    // filled the field with it and then refused it, disabling Log over a value
    // nobody typed — on a QSO that logged fine before the field existed, and
    // mid-pass, when there is no second chance at the contact.
    //
    // Closed at the seam the bad value enters: the callbook fills the field only
    // when what it returned IS a locator. Screen blank, record blank — no lie in
    // either direction, and the gate is left free to do its real job on what the
    // operator types.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    mockedQrz.mockResolvedValueOnce({ call: 'W1AW', grid: 'EN52/EN53', name: 'Rover' } as never)
    fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))

    // The lookup landed (it filled the blank Name) and left the grid alone.
    await waitFor(() =>
      expect((screen.getByPlaceholderText('Name') as HTMLInputElement).value).toBe('Rover'),
    )
    expect(gridInput().value, 'the callbook wrote a non-locator into the field').toBe('')
    expect(logBtn().disabled, 'a callbook value disabled Log').toBe(false)
    fireEvent.click(logBtn())
    expect(mockedLogQso.mock.calls[0][0].grid).toBeNull()
  })

  it('a later callbook lookup never overwrites the square the operator typed', async () => {
    // The one that matters on a bird: a rover passes you where he IS, and his
    // callbook says where he LIVES. The autofill is blanks-only (LogEntry
    // `lookup`) — pinned here so a refactor cannot quietly make the callbook win.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    fireEvent.change(gridInput(), { target: { value: 'EN52XA' } })

    mockedQrz.mockResolvedValueOnce({
      call: 'W1AW',
      grid: 'FN31PR',
      name: 'Hiram',
    } as never)
    fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))

    // The lookup DID land (it filled the blank Name), and left the grid alone.
    await waitFor(() =>
      expect((screen.getByPlaceholderText('Name') as HTMLInputElement).value).toBe('Hiram'),
    )
    expect(gridInput().value, 'the callbook clobbered the operator-typed square').toBe('EN52XA')

    fireEvent.click(logBtn())
    expect(mockedLogQso.mock.calls[0][0].grid).toBe('EN52XA')
  })

  it('still fills a BLANK grid from the callbook — the autofill is outranked, not disabled', async () => {
    // The mirror of the test above: blanks-only means blanks ARE filled.
    renderStd()
    fireEvent.change(callInput(), { target: { value: 'w1aw' } })
    mockedQrz.mockResolvedValueOnce({ call: 'W1AW', grid: 'FN31PR' } as never)
    fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))
    await waitFor(() => expect(gridInput().value).toBe('FN31PR'))
  })
})

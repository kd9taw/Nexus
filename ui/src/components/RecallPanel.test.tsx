// @vitest-environment jsdom
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { RecallPanel } from './RecallPanel'
import { distanceLabel, bearingLabel } from '../grid'
import type { CallHistory } from '../features/callHistory'
import type { LoggedQso } from '../types'

afterEach(cleanup)

function qso(over: Partial<LoggedQso> = {}): LoggedQso {
  return {
    call: 'W1ABC',
    grid: 'FN31',
    band: '20m',
    freqMhz: 14.25,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    whenUnix: Date.UTC(2026, 2, 14) / 1000, // 14 Mar 26
    ...(over as object),
  } as LoggedQso
}

function hist(over: Partial<CallHistory> = {}): CallHistory {
  const qsos = over.qsos ?? [qso()]
  return {
    qsos,
    count: qsos.length,
    workedBefore: qsos.length > 0,
    dupeThisBand: false,
    lastUnix: qsos.length ? qsos[0].whenUnix : null,
    confirmedCount: 0,
    bands: ['20m'],
    modes: ['SSB'],
    ...over,
    // keep the derived fields consistent with whatever qsos we were handed
    ...(over.qsos ? { count: over.qsos.length } : {}),
  }
}

// The FULL card is the only variant since 2026-07-31: the pane grid made the cockpit log pane's
// .pane-body the scroller, so the card can no longer crush the cockpit and `compact` (0.18.0's
// height-crush mitigation) lost its reason to exist. These tests pin the full card's inventory —
// the operator asked for exactly what 0.18.0 dropped: photo, QTH, distance/bearing, note, history.
describe('RecallPanel — the full card', () => {
  // REGRESSION KEPT FROM COMPACT (operator, 2026-07-26): "the log this qso on Voice is no longer
  // showing previous contacts for a CS entered and resolved." A bare "worked 3×" count answers
  // "have I worked them?" but NOT the question actually being asked mid-contact: when, on what
  // band, and what did we exchange. The full card must keep the real list.
  it('lists previous contacts with date / band+mode / report readable', () => {
    render(
      <RecallPanel
        call="W1ABC"
        band="40m"
        hist={hist({
          qsos: [
            qso({ band: '20m', mode: 'SSB', whenUnix: Date.UTC(2026, 2, 14) / 1000 }),
            qso({ band: '15m', mode: 'CW', rstSent: '599', rstRcvd: '579', whenUnix: Date.UTC(2025, 10, 2) / 1000 }),
          ],
        })}
      />,
    )
    expect(screen.getByText('14 Mar 26')).toBeTruthy()
    expect(screen.getByText('20m SSB')).toBeTruthy()
    expect(screen.getByText('59/57')).toBeTruthy()
    expect(screen.getByText('02 Nov 25')).toBeTruthy()
    expect(screen.getByText('15m CW')).toBeTruthy()
    expect(screen.getByText('599/579')).toBeTruthy()
  })

  it('shows the callbook photo over the initials, and a broken URL falls back to them', () => {
    const url = 'https://cdn-xfer.qrz.com/x/w1abc/photo.jpg'
    const { container } = render(<RecallPanel call="W1ABC" band="20m" image={url} hist={hist()} />)
    const img = container.querySelector('.recall-avatar-img') as HTMLImageElement
    expect(img, 'no callbook photo <img>').not.toBeNull()
    expect(img.src).toBe(url)
    // The box is reserved by .recall-avatar (fixed circle; the img is absolutely positioned
    // inside it) so loading causes no layout shift — the initials sit underneath throughout.
    expect(container.querySelector('.recall-avatar .recall-avatar-initials')?.textContent).toBe('W1')
    // Hotlink-blocked / dead URL: the img hides itself and the initials show through.
    fireEvent.error(img)
    expect(img.style.display).toBe('none')
    expect(container.querySelector('.recall-avatar .recall-avatar-initials')).not.toBeNull()
  })

  it('renders no <img> at all when the callbook returned no photo URL', () => {
    const { container } = render(<RecallPanel call="W1ABC" band="20m" image={null} hist={hist()} />)
    expect(container.querySelector('.recall-avatar-img')).toBeNull()
    expect(container.querySelector('.recall-avatar-initials')).not.toBeNull()
  })

  it('shows name, QTH · grid · country, and grid-derived distance + bearing from MY grid', () => {
    const { container } = render(
      <RecallPanel
        call="W1ABC"
        band="20m"
        name="Alice"
        qth="Hartford, CT"
        grid="FN31"
        country="United States"
        myGrid="EN52"
        hist={hist()}
      />,
    )
    expect(screen.getByText('Alice')).toBeTruthy()
    expect(container.querySelector('.recall-where')?.textContent).toBe('Hartford, CT (FN31) · United States')
    // The geo line derives from ui/src/grid.ts against the OPERATOR'S grid — assert the same
    // computation, not a hand-copied number that would rot if the haversine helper changed.
    const geo = container.querySelector('.recall-geo')
    expect(geo, 'no distance/bearing line').not.toBeNull()
    expect(geo!.textContent).toBe(`${distanceLabel('EN52', 'FN31')} · ${bearingLabel('EN52', 'FN31')}`)
  })

  it('omits the geo line when my grid or theirs is unknown (no "NaN mi")', () => {
    const noMine = render(<RecallPanel call="W1ABC" band="20m" grid="FN31" hist={hist()} />)
    expect(noMine.container.querySelector('.recall-geo')).toBeNull()
    cleanup()
    const noTheirs = render(<RecallPanel call="W1ABC" band="20m" myGrid="EN52" hist={hist()} />)
    expect(noTheirs.container.querySelector('.recall-geo')).toBeNull()
  })

  it('surfaces the most recent operator note', () => {
    render(
      <RecallPanel
        call="W1ABC"
        band="20m"
        hist={hist({ qsos: [qso({ notes: 'Runs a KX3 at 5W from a sailboat' })] })}
      />,
    )
    expect(screen.getByText(/Runs a KX3 at 5W from a sailboat/)).toBeTruthy()
  })

  it('still leads with the call and the decide-now flags', () => {
    render(<RecallPanel call="w1abc" band="20m" hist={hist({ dupeThisBand: true })} />)
    expect(screen.getByText('W1ABC')).toBeTruthy() // upper-cased for readback
    expect(screen.getByText(/Dupe 20m/)).toBeTruthy()
  })

  it('shows no history block on a never-worked call', () => {
    const { container } = render(<RecallPanel call="W9XYZ" band="20m" hist={hist({ qsos: [] })} />)
    expect(container.querySelector('.recall-log')).toBeNull()
  })

  // The history is a BOUNDED internal scroller — a sanctioned bounded log widget. Since the pane
  // grid, the log pane's .pane-body is the real scroller; a full-length nested list (or the old
  // 0.38·--vh-eff viewport-share cap) fights it for the same surplus. All rows stay in the DOM
  // and reachable; CSS scrolls them inside a fixed em ceiling.
  it('keeps a long history bounded rather than growing the card', () => {
    const many = Array.from({ length: 40 }, (_, i) =>
      qso({ whenUnix: Date.UTC(2026, 0, 1) / 1000 - i * 86_400 }),
    )
    const { container } = render(<RecallPanel call="W1ABC" band="20m" hist={hist({ qsos: many })} />)
    const list = container.querySelector('.recall-log-list')
    expect(list).toBeTruthy()
    expect(list?.querySelectorAll('.recall-log-row').length).toBe(40) // all present…
    expect(list?.getAttribute('role')).toBe('list') // …and reachable as a list, scrolled by CSS
  })

  it('renders nothing until enough of a call is typed', () => {
    const { container } = render(<RecallPanel call="W1" band="20m" hist={hist()} />)
    expect(container.firstChild).toBeNull()
  })
})

// ── The sheet side of the bounded-history contract ─────────────────────────────────────────
// jsdom applies no stylesheet, so the ceiling is verified against styles.css itself. This is
// NOT a dead-selector regex test (the banned kind): the render tests above prove the DOM the
// selectors target actually exists, and the walk below reads every DECLARATION on the class —
// descending into @media — so a second rule sneaking in a different cap fails the census.
describe('RecallPanel — bounded history + compact carcass census (styles.css)', () => {
  // import.meta.url is an http: URL under the jsdom environment — resolve from the
  // package root instead (the index-preseed.test.ts pattern).
  const SHEET = readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '') // prose must never read as a declaration

  /** Every top-level or @media-nested rule as {selector, body}. */
  function rules(sheet: string): Array<{ selector: string; body: string }> {
    const out: Array<{ selector: string; body: string }> = []
    let i = 0
    let selStart = 0
    while (i < sheet.length) {
      if (sheet[i] === '{') {
        const sel = sheet.slice(selStart, i).trim().replace(/\s+/g, ' ')
        i++
        const bodyStart = i
        let depth = 1
        while (i < sheet.length && depth > 0) {
          if (sheet[i] === '{') depth++
          else if (sheet[i] === '}') depth--
          i++
        }
        const body = sheet.slice(bodyStart, i - 1)
        if (sel.startsWith('@media') || sel.startsWith('@supports')) {
          out.push(...rules(body)) // a cap hiding inside a media block still counts
        } else if (!sel.startsWith('@')) {
          for (const s of sel.split(',')) if (s.trim()) out.push({ selector: s.trim(), body })
        }
        selStart = i
      } else {
        i++
      }
    }
    return out
  }
  const RULES = rules(SHEET)

  it('.recall-log-list scrolls inside a fixed em ceiling (~12em), not a viewport share', () => {
    const declaring = RULES.filter(
      (r) => r.selector.includes('.recall-log-list') && /max-height\s*:/.test(r.body),
    )
    expect(declaring.length, 'exactly one rule caps the history list').toBe(1)
    const cap = /max-height\s*:\s*([^;]+);/.exec(declaring[0].body)![1].trim()
    // A fixed em bound: the pane body is the real scroller, so the widget's ceiling must not
    // scale with the viewport (the old 0.38·--vh-eff cap re-created a second viewport-sized
    // grower inside the pane).
    expect(cap).toBe('12em')
    expect(/overflow-y\s*:\s*auto/.test(declaring[0].body), 'the ceiling must scroll, not clip').toBe(true)
  })

  it('the compact variant CSS is gone, not merely unused', () => {
    // Negative census, the cockpit-shells guard style: compact died with its last caller
    // (2026-07-31). A resurrected .recall-compact / .recall-line* is how a "one-line recall"
    // quietly comes back without the operator asking for it.
    for (const cls of ['recall-compact', 'recall-line']) {
      const hits = RULES.filter((r) => r.selector.includes(`.${cls}`)).map((r) => r.selector)
      expect(hits, `\`.${cls}\` rules are back in styles.css:\n${hits.join('\n')}`).toEqual([])
    }
  })
})

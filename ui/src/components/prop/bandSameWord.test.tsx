// @vitest-environment jsdom
//
// ONE BAND, ONE WORD, ONE COLOUR. The map's Band conditions list, the Band Advisor's rows, the band
// menu and the NOW bar each describe the band a report is about, and they must say the same thing.
// Before, they did not:
//   · a band heard NOW that the model calls closed (summer Es on 10 m or 6 m) read "Open" on the list
//     and in the menu — observation proves a band open — but was painted in the model's closed grey;
//   · the advisor coloured its word by the model the same way;
//   · the NOW bar said what the activity tier said ("quiet" for a band the model calls open and
//     nobody has heard yet, where the list says "Open · none heard"), and painted a closed band red.
// Now every surface takes the word and the colour from the same cell (propViz `bandConditionCell`):
// the colour follows the word. A heard band is Open and green; a silent band wears the model's word
// and its colour; a closed one recedes in grey, never red.
//
// The fixture bands DISAGREE on purpose: a test whose model and observation agree cannot tell
// "colour by the word" from "colour by the model".
//
// THE WORDS ARE TOKENS (operator, 2026-09-29: "Keep English tokens … the same English word in every
// language"). Open, Marginal and Closed are the backend's words and are shown as they come, like a
// band name: no surface translates them, so every surface says the same word in every language.
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandConditionStrip } from './BandConditionStrip'
import { BandAdvisor } from './BandAdvisor'
import { NowBar } from '../NowBar'
import { publishBandConditions, useBandConditions } from '../../bandConditions'
import { bandConditionCell } from '../../propViz'
import { installCatalog, setLocale } from '../../i18n'
import { DE } from '../../i18n/de'
import { ES } from '../../i18n/es'
import { FR } from '../../i18n/fr'
import { JA } from '../../i18n/ja'
import type { AppSnapshot, BandReport, PropagationSnapshot } from '../../types'

const report = (band: string, modeled: BandReport['modeled'], tier: BandReport['tier']): BandReport =>
  ({ band, modeled, tier, score: 0.1, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: 'r' }) as BandReport

/** [band, model, tier, the word every surface must say, the colour every surface must wear]. */
const CASES: ReadonlyArray<readonly [string, BandReport['modeled'], BandReport['tier'], 'Open' | 'Marginal' | 'Closed', string]> = [
  ['10m', 'Closed', 'Active', 'Open', 'var(--band-open)'], // heard, the model says closed: an Es opening
  ['17m', 'Marginal', 'Moderate', 'Open', 'var(--band-open)'], // heard, the model says marginal
  ['20m', 'Open', 'Quiet', 'Open', 'var(--band-open)'], // open by the model, nobody heard yet
  ['30m', 'Marginal', 'Quiet', 'Marginal', 'var(--band-marginal)'],
  ['40m', 'Closed', 'Quiet', 'Closed', 'var(--band-closed)'],
  ['80m', 'Closed', 'Closed', 'Closed', 'var(--band-closed)'],
]
const BANDS = CASES.map(([b, m, tier]) => report(b, m, tier))
const PROP = {
  advisory: { headline: '', banners: [], bands: BANDS },
  openings: [],
  dxpeditions: { workableNow: [], active: [], upcoming: [] },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

beforeEach(() => {
  localStorage.clear()
  publishBandConditions(PROP)
})
afterEach(() => {
  cleanup()
  publishBandConditions(null)
})

describe('one band, one word, one colour', () => {
  it('the cell: the colour follows the word; with no model and nothing heard it stays neutral, never green', () => {
    for (const [band, , , word, colour] of CASES) {
      const cell = bandConditionCell(BANDS.find((b) => b.band === band)!)
      expect([band, cell.word, cell.color]).toEqual([band, word, colour])
    }
    // No model (an older station's report) and nobody heard: the word defaults to Open, which is not
    // evidence, so the colour is the tier's neutral.
    expect(bandConditionCell(report('6m', undefined, 'Quiet')).color).toBe('var(--text-dim)')
    expect(bandConditionCell(report('6m', undefined, 'Closed')).color).toBe('var(--text-faint)')
  })

  it('the map’s list and the Band Advisor’s rows say the cell’s word in the cell’s colour', () => {
    const strip = render(<BandConditionStrip bands={BANDS} />).container
    const listed = [...strip.querySelectorAll<HTMLElement>('.bc-cell')].map((c) => {
      const pill = c.querySelector<HTMLElement>('.bc-state')!
      return [c.querySelector('.bc-band')!.textContent, pill.textContent, pill.style.getPropertyValue('--bc-color')]
    })
    const want = (order: string[]) => order.map((b) => CASES.find((c) => c[0] === b)!).map(([b, , , w, col]) => [b, w, col])
    expect(listed).toEqual(want(['80m', '40m', '30m', '20m', '17m', '10m']))
    cleanup()
    const advisor = render(<BandAdvisor bands={BANDS} />).container
    const rows = [...advisor.querySelectorAll<HTMLElement>('.ba-row')].map((r) => {
      const word = r.querySelector<HTMLElement>('.ba-modeled')!
      return [r.querySelector('.ba-band')!.textContent, word.textContent, word.style.getPropertyValue('--bc-color')]
    })
    expect(rows, 'the advisor keeps its own order (best first), so compare as sets').toEqual(
      expect.arrayContaining(want(CASES.map((c) => c[0]))),
    )
    expect(rows.length).toBe(CASES.length)
    for (const r of advisor.querySelectorAll<HTMLElement>('.ba-modeled')) {
      expect(r.style.color, 'the advisor letters its word inline').toBe('')
      // The same pill the list draws, so the list's lettering guard (BandConditionStrip.test.tsx)
      // covers the advisor's word too.
      expect(r.classList.contains('bc-state'), 'the advisor word is not the list’s pill').toBe(true)
    }
  })

  it('the band menu and the NOW bar say the same word, the backend’s, and a closed band is not red', () => {
    let lookup: ReturnType<typeof useBandConditions> | null = null
    function Probe() {
      lookup = useBandConditions()
      return null
    }
    render(<Probe />)
    const NOW_CLASS: Record<string, string> = { 'var(--band-open)': 'good', 'var(--band-marginal)': 'ok', 'var(--band-closed)': 'weak' }
    for (const [band, , , word, colour] of CASES) {
      const menu = lookup!(band)
      expect([band, menu.word, menu.color]).toEqual([band, word, colour])
      cleanup()
      const snap = { radio: { band } } as unknown as AppSnapshot
      const bar = render(
        <NowBar snap={snap} prop={PROP} feedHealth={null} connectEnabled={false} dxpedEnabled={false} onNavigate={() => {}} />,
      ).container
      const chip = [...bar.querySelectorAll<HTMLElement>('.nb-chip')].find((c) => c.querySelector('.nb-v')?.textContent?.startsWith(band))!
      expect([band, chip.querySelector('.nb-v')!.textContent]).toEqual([band, `${band} ${word}`])
      const cls = [...chip.classList].filter((c) => c !== 'nb-chip')
      expect([band, cls]).toEqual([band, [NOW_CLASS[colour]]])
      expect(cls, `${band}: a closed band's chip is red`).not.toContain('bad')
      cleanup()
      render(<Probe />)
    }
  })
})

describe('the band words are tokens, like the band names', () => {
  afterEach(() => setLocale('en'))

  it('in German, Spanish, French and Japanese the NOW bar says the band menu’s English word', () => {
    for (const [locale, catalog] of [['de', DE], ['es', ES], ['fr', FR], ['ja', JA]] as const) {
      installCatalog(locale, catalog)
      setLocale(locale)
      for (const [band, , , word] of CASES) {
        let lookup: ReturnType<typeof useBandConditions> | null = null
        function Probe() {
          lookup = useBandConditions()
          return null
        }
        render(<Probe />)
        expect([locale, band, lookup!(band).word], 'control: the menu says the backend’s word').toEqual([locale, band, word])
        cleanup()
        const snap = { radio: { band } } as unknown as AppSnapshot
        const bar = render(
          <NowBar snap={snap} prop={PROP} feedHealth={null} connectEnabled={false} dxpedEnabled={false} onNavigate={() => {}} />,
        ).container
        const chip = [...bar.querySelectorAll<HTMLElement>('.nb-chip')].find((c) => c.querySelector('.nb-v')?.textContent?.startsWith(band))!
        expect([locale, chip.querySelector('.nb-v')!.textContent]).toEqual([locale, `${band} ${word}`])
        cleanup()
      }
    }
  })
})

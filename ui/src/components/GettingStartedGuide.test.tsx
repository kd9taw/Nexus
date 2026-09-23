// @vitest-environment jsdom
//
// Structure guards for the Getting started guide. Three things can break here
// without anyone noticing, so each is COMPUTED rather than asserted by presence:
//
//   1. Step state. Four panels, one integer, three ways to move (rail, Back,
//      Next) and two bounds. A panel that renders alongside another, or a Next
//      that runs off the end, is invisible in a diff.
//   2. The wizard recreations are PICTURES of the setup wizard, not live
//      controls. Swap one static `<span className="wizard-go">` for a real
//      `<button>` and the guide grows a dead "Import my ADIF log…" button that
//      looks exactly like the working one. The test walks the rendered panels
//      and refuses any interactive element inside them.
//
// The third guard — that every token the guide paints with resolves in BOTH
// themes — reads styles.css from disk and so lives in `styles-getting-started.
// test.ts`, with the other node-environment sheet guards.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { GettingStartedGuide } from './GettingStartedGuide'
import { EN } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { resolveTarget } from '../settings/registry'

beforeAll(() => {
  // jsdom has neither; the guide asks matchMedia about reduced motion and
  // scrolls the dialog box back to the top on every step change.
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} }) as unknown as
      MediaQueryList) as typeof window.matchMedia
  Element.prototype.scrollTo = function () {}
})

afterEach(cleanup)

/** The step panels are the sections carrying an "Step N of 4: …" label. */
function panels(): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>('.gsg-content section[aria-label^="Step "]'),
  )
}

/** The single visible step, by its 1-based number. Throws if not exactly one. */
function currentStep(): number {
  const p = panels()
  expect(p, 'exactly one step panel renders at a time').toHaveLength(1)
  const m = /^Step (\d) of 4/.exec(p[0].getAttribute('aria-label') ?? '')
  expect(m, `unreadable step label: ${p[0].getAttribute('aria-label')}`).toBeTruthy()
  return Number(m![1])
}

const back = () => screen.getByRole<HTMLButtonElement>('button', { name: /Back/ })
const next = () => screen.getByRole('button', { name: /Next —|That’s all four/ })

describe('Getting started guide — step state', () => {
  it('opens on step 1 with Back out of reach', () => {
    render(<GettingStartedGuide onClose={() => {}} />)
    expect(currentStep()).toBe(1)
    expect(back().disabled, 'Back is out of reach on step 1').toBe(true)
  })

  it('Next walks 1→4 and Back walks it home, one panel at a time', () => {
    render(<GettingStartedGuide onClose={() => {}} />)
    for (const want of [2, 3, 4]) {
      fireEvent.click(next())
      expect(currentStep()).toBe(want)
    }
    for (const want of [3, 2, 1]) {
      fireEvent.click(back())
      expect(currentStep()).toBe(want)
    }
    expect(back().disabled, 'Back is out of reach on step 1').toBe(true)
  })

  it('the rail jumps straight to a step and marks it current', () => {
    render(<GettingStartedGuide onClose={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Your ADIF log/ }))
    expect(currentStep()).toBe(4)
    const cur = document.querySelectorAll('.gsg-rail-btn.cur')
    expect(cur, 'exactly one rail button is current').toHaveLength(1)
    expect(cur[0].textContent).toContain('Your ADIF log')
    // The three behind it read as done, none of them as current.
    expect(document.querySelectorAll('.gsg-rail-btn.done')).toHaveLength(3)
  })

  it('the last step closes the guide instead of running off the end', () => {
    const onClose = vi.fn()
    render(<GettingStartedGuide onClose={onClose} />)
    fireEvent.click(screen.getByRole('button', { name: /Your ADIF log/ }))
    expect(currentStep()).toBe(4)
    fireEvent.click(screen.getByRole('button', { name: /That’s all four/ }))
    expect(onClose).toHaveBeenCalledTimes(1)
    // And it did not try to become step 5.
    expect(currentStep()).toBe(4)
  })
})

describe('the wizard recreations are pictures, not controls', () => {
  it('no step puts an interactive element inside a .gsg-shot', () => {
    render(<GettingStartedGuide onClose={() => {}} />)
    const seen: string[] = []
    for (let step = 1; step <= 4; step++) {
      if (step > 1) fireEvent.click(next())
      expect(currentStep()).toBe(step)
      const shots = document.querySelectorAll('.gsg-shot')
      expect(shots.length, `step ${step} shows at least one wizard panel`).toBeGreaterThan(0)
      for (const shot of shots) {
        for (const el of shot.querySelectorAll('button, a, input, select, textarea, [tabindex]')) {
          seen.push(`step ${step}: <${el.tagName.toLowerCase()}> ${el.textContent?.trim()}`)
        }
      }
    }
    expect(
      seen,
      `a recreated wizard panel grew a real control — it is documentation of the wizard, ` +
        `and a control here would look live and do nothing:\n${seen.join('\n')}`,
    ).toHaveLength(0)
  })
})

// #353 — the "Coming from WSJT-X?" note promised that JTAlert and GridTracker keep working, and
// they do, but only once the WSJT-X UDP API switch is on, and it ships OFF. A GridTracker2 user
// went looking for an ADIF file instead. The note has to name the switch and say where it is.
describe('Coming from WSJT-X: the switch JTAlert and GridTracker need', () => {
  /** The switch's label in Settings. A name, not prose: SettingsPanel.tsx renders this same
   *  literal in every language, so every translation must carry it verbatim. */
  const SWITCH = 'WSJT-X UDP API'
  /** Where it lives, in the labels the English Settings actually shows. */
  const PATH = `${EN['nav.settings.label']} ▸ ${EN['settings.tabs.logging']} ▸ ${EN['settings.integrations.legend']}`
  const note = () => document.querySelector<HTMLElement>('.gsg-wsjtx') as HTMLElement

  it('names the switch, and the path to it in the words Settings shows', () => {
    render(<GettingStartedGuide onClose={() => {}} onOpenSettings={() => {}} />)
    const text = note().textContent ?? ''
    expect(text).toContain(SWITCH)
    expect(text).toContain(PATH)
  })

  it('links straight to the section that holds the switch, and gets out of the way', () => {
    const onOpenSettings = vi.fn()
    const onClose = vi.fn()
    render(<GettingStartedGuide onClose={onClose} onOpenSettings={onOpenSettings} />)
    fireEvent.click(
      within(note()).getByRole('button', { name: EN['settings.integrations.legend'] }),
    )
    expect(onOpenSettings).toHaveBeenCalledTimes(1)
    expect(onOpenSettings).toHaveBeenCalledWith('integrations-feeds')
    // The guide is a modal; left open, it would sit on top of the section it just opened.
    expect(onClose).toHaveBeenCalledTimes(1)
    // …and the id is a live one. A pointer at a section that no longer resolves fails silently.
    expect(resolveTarget('integrations-feeds')).toEqual({
      tab: 'logging',
      section: 'integrations-feeds',
    })
  })

  it('with no way into Settings (the Remote page), the path is text, not a dead button', () => {
    render(<GettingStartedGuide onClose={() => {}} />)
    expect(within(note()).queryAllByRole('button')).toHaveLength(0)
    expect(note().textContent).toContain(PATH)
  })

  it('every language names the same switch and links the section by the name its Settings shows', () => {
    const problems: string[] = []
    const catalogs = { en: EN, de: DE, es: ES, fr: FR, ja: JA } as Record<
      string,
      Record<string, unknown>
    >
    for (const [lang, cat] of Object.entries(catalogs)) {
      const body = String(cat['gettingStarted.wsjtx.body'])
      if (!body.includes(SWITCH)) problems.push(`${lang}: never names "${SWITCH}"`)
      const path = `${cat['nav.settings.label']} ▸ ${cat['settings.tabs.logging']} ▸ <a>${cat['settings.integrations.legend']}</a>`
      if (!body.includes(path)) problems.push(`${lang}: no "${path}" in "${body}"`)
    }
    expect(problems).toEqual([])
  })
})

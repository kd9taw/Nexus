// @vitest-environment jsdom
//
// THE SPLASH PAGE'S WORDS — `ui/public/splashscreen.html`.
//
// The splash says, on the first launch after the logbook database arrived, that Nexus is moving
// the log into it. That page loads before the app and before this folder's runtime, and pulling
// the runtime in would load every catalog into a page whose job is to appear at once — so it
// carries its own few strings. This file is what keeps them honest: the same languages as the
// app, every string in each, the same placeholders, the operator's language chosen the way the
// app chooses it, and the numbers written the invariant way. It also runs the page's own script
// against each note the launch sends (`SplashNote` in src-tauri/src/lib.rs, whose test pins the
// same JSON literals used here).
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { LOCALE_KEY, SOURCE_LOCALE } from './index'

// jsdom replaces `URL` here, and `fs` takes only Node's, so paths go through their `href`.
const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url).href), 'utf8')
const HTML = read('../../public/splashscreen.html')

type Strings = Record<string, Record<string, string>>
type Splash = { show(note: unknown): void }

function splashStrings(): Strings {
  const doc = new DOMParser().parseFromString(HTML, 'text/html')
  return JSON.parse(doc.getElementById('splash-strings')?.textContent ?? '{}') as Strings
}

/** Put the page in this document and run its script, as the webview does. */
function loadSplash(): Splash {
  const doc = new DOMParser().parseFromString(HTML, 'text/html')
  document.head.innerHTML = doc.head.innerHTML
  document.body.innerHTML = doc.body.innerHTML
  const code = [...document.querySelectorAll('script:not([type])')].map((s) => s.textContent ?? '').join('\n')
  new Function(code)()
  return (window as unknown as { nexusSplash: Splash }).nexusSplash
}

const box = () => document.getElementById('status') as HTMLElement
const part = (name: string) => box().querySelector(`.${name}`) as HTMLElement

const placeholders = (s: string) => [...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort()

describe('the splash page speaks the languages the app ships', () => {
  it('every language main.tsx installs, and English', () => {
    const main = read('../main.tsx')
    const installed = [...main.matchAll(/installCatalog\(\s*'([a-z-]+)'/g)].map((m) => m[1])
    expect(installed.length, 'control: main.tsx really does install catalogs').toBeGreaterThan(0)
    expect(Object.keys(splashStrings()).sort()).toEqual([SOURCE_LOCALE, ...installed].sort())
  })

  it('every string in each, none blank, with the placeholders English has', () => {
    const strings = splashStrings()
    const en = strings[SOURCE_LOCALE]
    expect(Object.keys(en).length, 'control: English has strings').toBeGreaterThan(0)
    for (const [lang, table] of Object.entries(strings)) {
      expect(Object.keys(table).sort(), `${lang}: the same strings as English`).toEqual(Object.keys(en).sort())
      for (const [key, text] of Object.entries(table)) {
        expect(text.trim(), `${lang}/${key} is blank`).not.toBe('')
        expect(placeholders(text), `${lang}/${key}: placeholders`).toEqual(placeholders(en[key]))
      }
    }
  })

  it('reads the language choice from the key the app stores it under', () => {
    expect(HTML).toContain(`localStorage.getItem('${LOCALE_KEY}')`)
  })
})

describe('what the splash shows for each note the launch sends', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => vi.restoreAllMocks())

  it('shows nothing on an ordinary launch, which sends no note', () => {
    const splash = loadSplash()
    expect(splash, 'control: the page script ran').toBeDefined()
    expect(box().hidden).toBe(true)
  })

  it('converting, not yet counted: what is happening, that it happens once, and a sweep', () => {
    loadSplash().show({ state: 'converting', done: 0, total: 0 })
    expect(box().hidden).toBe(false)
    expect(part('title').textContent).toBe('Moving your logbook into its new database')
    expect(part('detail').textContent).toBe('This happens once. Nexus opens by itself when it is done.')
    expect(part('bar').classList.contains('indeterminate')).toBe(true)
    expect(part('count').textContent).toBe('')
  })

  it('converting, counted: the count and a bar, from the JSON the launch sends', () => {
    // The exact literal `splash_script` produces (see its test in src-tauri/src/lib.rs).
    loadSplash().show(JSON.parse('{"state":"converting","done":12,"total":34}'))
    expect(part('count').textContent).toBe('12 of 34 contacts')
    expect(part('bar').classList.contains('indeterminate')).toBe(false)
    expect(part('fill').style.width).toBe('35.3%')
  })

  it('writes counts the invariant way, in every language: never grouped, never a comma', () => {
    const splash = loadSplash()
    localStorage.setItem(LOCALE_KEY, 'de')
    splash.show({ state: 'converting', done: 150214, total: 300000 })
    expect(part('count').textContent).toBe('150214 von 300000 Kontakten')
    expect(part('fill').style.width).toBe('50.1%')
  })

  it('says it is finishing once every contact is in', () => {
    loadSplash().show({ state: 'converting', done: 34, total: 34 })
    expect(part('count').textContent).toBe('Finishing up…')
    expect(part('fill').style.width, 'a full bar').toBe('100%')
  })

  it('then that Nexus is opening', () => {
    const splash = loadSplash()
    splash.show({ state: 'converting', done: 3, total: 34 })
    splash.show(JSON.parse('{"state":"opening"}'))
    expect(part('title').textContent).toBe('Opening Nexus…')
    expect(part('detail').textContent).toBe('')
    expect(part('count').textContent).toBe('')
    expect(part('bar').classList.contains('indeterminate')).toBe(false)
  })

  it('ignores a note it does not know', () => {
    loadSplash().show({ state: 'something-new' })
    expect(box().hidden).toBe(true)
  })

  it("speaks the operator's language: the one they chose, else the system's, else English", () => {
    const splash = loadSplash()
    localStorage.setItem(LOCALE_KEY, 'de')
    splash.show({ state: 'converting', done: 0, total: 0 })
    expect(part('title').textContent).toBe('Dein Logbuch zieht in seine neue Datenbank um')
    expect(document.documentElement.lang).toBe('de')

    localStorage.removeItem(LOCALE_KEY)
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('fr-CA')
    splash.show({ state: 'converting', done: 0, total: 0 })
    expect(part('title').textContent).toBe('Transfert de votre journal vers sa nouvelle base de données')

    localStorage.setItem(LOCALE_KEY, 'xx')
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('pt-BR')
    splash.show({ state: 'converting', done: 0, total: 0 })
    expect(part('title').textContent).toBe('Moving your logbook into its new database')
  })
})

// @vitest-environment jsdom
//
// A click on a call shown as <...> is refused by the engine, in English, and the toast put that
// sentence after the operator's own-language "Could not work <...>": half a toast in one language,
// half in another. `callStation` now swaps each refusal for its catalog entry. These hold the
// swap to the engine's exact words (read from engine.rs, so a rewording there turns this red
// instead of quietly bringing the English back), and every catalog to having both sentences.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { callStation, ENGINE_UNRESOLVED_CALL, ENGINE_UNRESOLVED_CQ } from '../api'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'
import { RU } from './ru'
import { installCatalog, setLocale } from './index'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = { en: EN, de: DE, es: ES, fr: FR, ja: JA, ru: RU }
const KEYS = ['shell.work.unresolvedCq', 'shell.work.unresolvedCall'] as const

/** The text of `Engine::<name>`, its line continuations joined as Rust joins them. */
function engineSentence(name: string): string {
  // Resolved from the cwd (vitest runs in ui/): under jsdom import.meta.url is http:.
  const src = readFileSync(resolve(process.cwd(), '../crates/tempo-app/src/engine.rs'), 'utf8')
  const m = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(src)
  if (!m) throw new Error(`no ${name} in engine.rs`)
  return m[1].replace(/\\\n\s*/g, '')
}

describe('a click on a call shown as <...>', () => {
  it("knows the engine's refusals word for word", () => {
    expect([ENGINE_UNRESOLVED_CQ, ENGINE_UNRESOLVED_CALL]).toEqual([
      engineSentence('UNRESOLVED_CQ'),
      engineSentence('UNRESOLVED_CALL'),
    ])
  })

  it('the English entries are the engine sentences', () => {
    expect([EN['shell.work.unresolvedCq'], EN['shell.work.unresolvedCall']]).toEqual([
      engineSentence('UNRESOLVED_CQ'),
      engineSentence('UNRESOLVED_CALL'),
    ])
  })

  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => KEYS.map((key) => [lang, key, cat] as const)))(
    '%s %s: is there, and keeps the <...> the operator sees',
    (lang, key, cat) => {
      const text = cat[key]
      expect(typeof text === 'string' && text.includes('<...>'), String(text)).toBe(true)
      if (lang !== 'en') expect(text).not.toBe(EN[key])
    },
  )

  describe('through callStation', () => {
    let refusal: unknown
    beforeEach(() => {
      ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
        invoke: async () => {
          throw refusal
        },
      }
      installCatalog('fr', FR)
      setLocale('fr')
    })
    afterEach(() => {
      delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
      setLocale('en')
    })

    it("the refusal reaches the toast in the operator's language", async () => {
      refusal = engineSentence('UNRESOLVED_CALL')
      await expect(callStation('<...>', undefined, 'K1ABC <...> -08', -8, 1413)).rejects.toBe(
        FR['shell.work.unresolvedCall'],
      )
      refusal = engineSentence('UNRESOLVED_CQ')
      await expect(callStation('<...>', undefined, 'CQ <...>')).rejects.toBe(FR['shell.work.unresolvedCq'])
    })

    it('any other refusal comes through as it was', async () => {
      refusal = 'No recent decode from K1ABC — wait for their next transmission, then click again.'
      await expect(callStation('K1ABC')).rejects.toBe(refusal)
      refusal = new Error('bridge gone')
      await expect(callStation('K1ABC')).rejects.toBe(refusal)
    })
  })
})

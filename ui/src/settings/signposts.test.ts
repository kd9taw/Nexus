// A SIGNPOST MUST NAME A PLACE THAT EXISTS.
//
// #358: the Repeaters screen told operators to add a token in "Settings ▸ Integrations". No such
// place existed; the token lives in Services ▸ Confirmations. Nothing went red, because
// nothing checked an in-app hint against the Settings the app actually has. The registry already
// knows every tab and section (registry.ts), so this does:
//
//   1. PROSE. Every English catalog string that says "Settings ▸ …" must walk to a real place: the
//      first step a current tab or section (by its label, or a section's id), each step after a
//      tab one of that tab's sections (or a keyword of one, e.g. "Station ▸ Grid"), anything after
//      a section free (the control's own name). A legacy name the registry keeps only as an alias
//      ("Integrations", "Spot Sources") is NOT a place: an alias exists so an old link still lands
//      somewhere, which is exactly why it must not be written into a new one.
//   2. LINKS. Every literal passed to `openSettingsAt` / `onOpenSettings` in the app must be a
//      section's own id: ids are the stable public names (registry.ts header), and a label or an
//      alias there is a pointer waiting to break on the next rename.
//
// What it cannot see: the four translated catalogs (their tab and section names are translations
// the registry does not hold; they follow English by key), a pointer that names a place with no
// "Settings ▸" in front of it, and a link built from a variable (`blockerTarget` is checked below
// by calling it).
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { EN } from '../i18n/en'
import {
  SETTINGS_TABS,
  SETTINGS_SECTIONS,
  sectionSlug,
  sectionsForTab,
  type SettingsTabId,
} from './registry'
import { blockerTarget } from '../features/contestStart'

/** Strings that name ANOTHER program's settings (the OS, a browser, a web service), by key. */
const OTHER_PROGRAMS: Record<string, string> = {
  'remote.stream.mic.denied.system': "the operating system's privacy settings",
  'remote.lan.firewall.public': "Windows' network settings",
  'gettingStarted.wsjtx.mac': "macOS's keyboard settings",
  'settings.connections.qrz.test.fail': "QRZ.com's account settings",
}

/** The "Settings ▸ a ▸ b" paths in a catalog string, each as its steps. */
export function settingsPaths(text: string): string[][] {
  const plain = text.replace(/<[^>]+>/g, '').replace(/&amp;/g, '&')
  const out: string[][] = []
  for (const m of plain.matchAll(/Settings((?:\s*▸\s*[^▸.;:!?,"“”\n]+)+)/g)) {
    out.push(m[1].split('▸').map((x) => x.trim()).filter(Boolean))
  }
  return out
}

interface Place {
  tab: SettingsTabId
  section?: string
}

const slug = (s: string) => sectionSlug(s.replace(/[()]/g, ' '))

/** One step, by its exact words: a tab or a section first, then a section of that tab. */
function step(words: string, at?: Place): Place | null {
  if (!at) {
    const tab = SETTINGS_TABS.find((t) => slug(t.label) === slug(words))
    if (tab) return { tab: tab.id }
    const sec = SETTINGS_SECTIONS.find((s) => slug(s.label) === slug(words) || s.id === slug(words))
    return sec ? { tab: sec.tab, section: sec.id } : null
  }
  if (at.section) return at // below a section: the control's own name
  const secs = sectionsForTab(at.tab)
  const sec =
    secs.find((s) => slug(s.label) === slug(words) || s.id === slug(words)) ??
    secs.find((s) => s.keywords.some((k) => slug(k) === slug(words)))
  return sec ? { tab: at.tab, section: sec.id } : null
}

/** Walk a path. The LAST step runs on into the sentence ("Station to download around…"), so it
 *  is matched by its longest leading words that name a place. */
export function walkSettingsPath(steps: string[]): Place | null {
  let at: Place | undefined
  for (let i = 0; i < steps.length; i++) {
    const words = steps[i].split(/\s+/)
    const shortest = i === steps.length - 1 ? 1 : words.length
    let got: Place | null = null
    for (let n = words.length; n >= shortest && !got; n--) got = step(words.slice(0, n).join(' '), at)
    if (!got) return null
    at = got
  }
  return at ?? null
}

const src = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** Every literal handed to a Settings deep link in the app's own modules: `[file, literal]`. */
export function deepLinkLiterals(files: { rel: string; text: string }[]): [string, string][] {
  const out: [string, string][] = []
  for (const { rel, text } of files) {
    for (const m of text.matchAll(/\b(?:openSettingsAt|onOpenSettings)\??\.?\(\s*(['"`])([^'"`]*)\1\s*\)/g)) {
      out.push([rel, m[2]])
    }
  }
  return out
}
const isSectionId = (id: string) => SETTINGS_SECTIONS.some((s) => s.id === id)

function appModules(): { rel: string; text: string }[] {
  return (readdirSync(src, { recursive: true }) as string[])
    .filter((rel) => /\.tsx?$/.test(rel) && !/\.test\.tsx?$/.test(rel) && !rel.split(sep).includes('__fixtures__'))
    .map((rel) => ({ rel: rel.split(sep).join('/'), text: readFileSync(resolve(src, rel), 'utf8') }))
}

describe('every "Settings ▸" in the English catalog names a place that exists', () => {
  it('walks every path to a current tab or section', () => {
    const dead: string[] = []
    let seen = 0
    for (const [key, value] of Object.entries(EN)) {
      if (typeof value !== 'string' || key in OTHER_PROGRAMS) continue
      for (const path of settingsPaths(value)) {
        seen++
        if (!walkSettingsPath(path)) dead.push(`${key}: Settings ▸ ${path.join(' ▸ ')}`)
      }
    }
    // The parser found the catalog's pointers at all (81 when this was written).
    expect(seen).toBeGreaterThan(60)
    expect(dead).toEqual([])
  })

  it('every exemption is still a string that names another program (no stale exemptions)', () => {
    for (const key of Object.keys(OTHER_PROGRAMS)) {
      const value = (EN as Record<string, unknown>)[key]
      expect(typeof value, key).toBe('string')
      expect(settingsPaths(value as string).length, key).toBeGreaterThan(0)
    }
  })

  it('CONTROL: the #358 pointer and an alias-only name fail; real ones pass', () => {
    const walk = (s: string) => settingsPaths(s).map(walkSettingsPath)
    expect(walk('Add a token in Settings ▸ Integrations.')).toEqual([null])
    expect(walk('Set a cluster in Settings ▸ Spot Sources.')).toEqual([null])
    expect(walk('in Settings ▸ Services ▸ Integrations.')).toEqual([null])
    expect(walk('in Settings ▸ Services ▸ Confirmations fills the gap.')).toEqual([
      { tab: 'services', section: 'confirmations' },
    ])
    // A tab the regroup removed is an alias now, not a place, so a hint still naming it fails.
    expect(walk('in Settings ▸ Logging & Connectors ▸ Confirmations fills the gap.')).toEqual([null])
    expect(walk('in Settings ▸ Config ▸ Backup & reset.')).toEqual([null])
    expect(walk('your grid (Settings ▸ Station ▸ Grid) to know when')).toEqual([
      { tab: 'station', section: 'operator-radio' },
    ])
    expect(walk('Settings ▸ Beacons (WSPR & FST4W)')).toEqual([{ tab: 'digital', section: 'beacons-wspr-fst4w' }])
  })
})

describe('every Settings deep link in the app is a section id', () => {
  it('names a section of the registry by its id', () => {
    const links = deepLinkLiterals(appModules())
    expect(links.length).toBeGreaterThan(5)
    expect(links.filter(([, id]) => !isSectionId(id))).toEqual([])
  })

  it('the contest blockers open a section that exists', () => {
    expect(isSectionId(blockerTarget({ fields: ['class'] } as never))).toBe(true)
    expect(isSectionId(blockerTarget({} as never))).toBe(true)
  })

  it('CONTROL: a planted link to a tab, a label or an alias is caught', () => {
    const planted = deepLinkLiterals([
      { rel: 'x.tsx', text: "onOpenSettings('integrations'); openSettingsAt(\"Confirmations\"); onOpenSettings?.('radios')" },
    ])
    expect(planted.map(([, id]) => id)).toEqual(['integrations', 'Confirmations', 'radios'])
    expect(planted.filter(([, id]) => !isSectionId(id)).map(([, id]) => id)).toEqual(['integrations', 'Confirmations'])
  })
})

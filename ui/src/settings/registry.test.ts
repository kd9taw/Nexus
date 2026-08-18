// Guards for the settings registry.
//
// The registry is only worth having if it is PROVEN to describe the real panel. Four reorgs
// decayed because placement lived in the shape of the JSX and nothing checked it; a registry
// that is allowed to drift from the panel is the same failure with an extra file. So:
//
//   1. EXHAUSTIVENESS — every `<legend>` the panel renders is a registered section, and every
//      registered section is rendered. Neither direction alone is enough: the first catches a
//      new section added without a registry entry (the ~3-fields-a-day path), the second
//      catches a registry entry left behind when a section is deleted or renamed.
//   2. THE ADVANCED RULE — a setting may hide in a collapsed disclosure only if it is not
//      needed in the first hour. Issue #62 was an operator who could not find a toggle that
//      already shipped, inside the collapsed Advanced group. The rule was written down in the
//      0.17 plan and never enforced by anything, so it did not hold.
//   3. DENSITY — no tab may grow past a ceiling without someone deciding to raise it. This is
//      the guard the previous four reorgs lacked entirely: they each fixed the symptom (tab
//      count) while the thing operators actually scan (sections per page) grew monotonically.
//
// These run in node — the registry is pure data, so none of this needs a DOM.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { EN } from '../i18n/en'
import {
  searchSettings,
  SETTINGS_TABS,
  SETTINGS_SECTIONS,
  TAB_ALIASES,
  resolveTarget,
  sectionById,
  sectionsForTab,
  sectionSlug,
} from './registry'

const here = dirname(fileURLToPath(import.meta.url))

/**
 * The panel's source, plus every component a section has been extracted INTO.
 *
 * ⚠️ ADD A FILE HERE WHEN YOU EXTRACT A SECTION. This guard reads legends out of the source
 * text, so a `<fieldset>` that moves to its own component vanishes from the guard's view and
 * its registry entry looks orphaned. `SettingsStation.tsx` was the first (2026-08-18, the i18n
 * pilot); the failure it would otherwise have produced named the registry, not the move.
 */
const PANEL_SOURCES = ['../components/SettingsPanel.tsx', '../components/SettingsStation.tsx']
const panelSrc = PANEL_SOURCES.map((rel) => readFileSync(resolve(here, rel), 'utf8')).join('\n')

/** The panel writes legends as JSX with HTML entities (`&amp;`, `&mdash;`); the registry holds
 * plain text. Decode the handful the panel actually uses so the two can be compared.
 *
 * A MIGRATED section writes its legend as `{t('key')}` instead of literal English, so resolve
 * that through the English catalog — which is what the panel actually renders. Doing it here
 * rather than loosening the guard keeps the doc headings and deep-link anchors checked against
 * the real text, in either form. */
const decode = (s: string) => {
  const key = /^\{t\('([^']+)'\)\}$/.exec(s.trim())?.[1]
  const raw = key ? (EN as Record<string, unknown>)[key] : s
  if (key && typeof raw !== 'string') {
    throw new Error(`legend key ${key} is not a plain English catalog entry`)
  }
  return String(raw)
    .replace(/&amp;/g, '&')
    .replace(/&mdash;/g, '—')
    .replace(/&rarr;/g, '→')
    .replace(/&apos;/g, "'")
    .replace(/&quot;/g, '"')
    .trim()
}

/** Every `<legend>` in the panel, in source order. */
function panelLegends(): string[] {
  return [...panelSrc.matchAll(/<legend>([\s\S]*?)<\/legend>/g)].map((m) =>
    decode(m[1].replace(/\s+/g, ' ')),
  )
}

/** Every collapsible `SettingsGroup` title in the panel. Attribute order is not fixed — the
 * anchor `id` may precede `title` — so match the tag and pull `title` out of it. */
function panelGroups(): string[] {
  return [...panelSrc.matchAll(/<SettingsGroup\s+([^>]*?)>/g)]
    .map((m) => m[1].match(/title="([^"]+)"/)?.[1])
    .filter((t): t is string => Boolean(t))
    .map(decode)
}

describe('the registry describes the panel that actually renders', () => {
  it('registers every legend the panel renders', () => {
    const registered = new Set(SETTINGS_SECTIONS.filter((s) => !s.parent).map((s) => s.label))
    const missing = panelLegends().filter((l) => !registered.has(l))
    expect(
      missing,
      `SettingsPanel.tsx renders these legends with no registry entry — add them to ` +
        `SETTINGS_SECTIONS so they get an anchor, a doc heading and search keywords`,
    ).toEqual([])
  })

  it('registers every collapsible group the panel renders', () => {
    const registered = new Set(SETTINGS_SECTIONS.filter((s) => s.parent).map((s) => s.label))
    const missing = panelGroups().filter((g) => !registered.has(g))
    expect(missing, 'SettingsGroup titles with no registry entry').toEqual([])
  })

  it('renders every section the registry claims', () => {
    const rendered = new Set([...panelLegends(), ...panelGroups()])
    const orphans = SETTINGS_SECTIONS.filter((s) => !rendered.has(s.label)).map((s) => s.id)
    expect(
      orphans,
      'registry entries with no matching legend/group in SettingsPanel.tsx — a stale entry ' +
        'publishes a doc heading and a deep-link target that go nowhere',
    ).toEqual([])
  })

  it('gives every section a unique, anchor-safe id', () => {
    const ids = SETTINGS_SECTIONS.map((s) => s.id)
    expect([...new Set(ids)].sort()).toEqual([...ids].sort())
    const bad = ids.filter((id) => !/^[a-z0-9-]+$/.test(id))
    expect(bad, 'ids become DOM anchors and doc slugs — lowercase, digits and dashes only').toEqual(
      [],
    )
  })

  it('points every section at a real tab, and every nested group at a real parent', () => {
    const tabIds = new Set(SETTINGS_TABS.map((t) => t.id))
    expect(SETTINGS_SECTIONS.filter((s) => !tabIds.has(s.tab)).map((s) => s.id)).toEqual([])
    const ids = new Set(SETTINGS_SECTIONS.map((s) => s.id))
    const orphanParents = SETTINGS_SECTIONS.filter((s) => s.parent && !ids.has(s.parent)).map(
      (s) => `${s.id} -> ${s.parent}`,
    )
    expect(orphanParents).toEqual([])
    // A nested group must live on the same tab as its parent, or the deep link expands a
    // disclosure the operator cannot see.
    const strayTabs = SETTINGS_SECTIONS.filter(
      (s) => s.parent && sectionById(s.parent)?.tab !== s.tab,
    ).map((s) => s.id)
    expect(strayTabs).toEqual([])
  })

  it('leaves no tab empty', () => {
    const empty = SETTINGS_TABS.filter((t) => sectionsForTab(t.id).length === 0).map((t) => t.id)
    expect(empty, 'a tab with no sections renders a blank page').toEqual([])
  })
})

describe('the panel rail and the registry agree', () => {
  // ⚠️ WHY THIS IS A TEST AND NOT AN IMPORT. `docs-match-code.test.ts:420-484` parses the
  // literal `SETTINGS_TABS = [...]` declaration OUT of SettingsPanel.tsx with a regex and
  // asserts the manual's `##` headings equal those labels in order. Re-exporting the registry's
  // array from the panel would leave that parser with nothing to read, and the tempting fix
  // (weakening the docs guard) is the dead-fix pattern its own header warns about. So the panel
  // keeps its literal array, and THIS test is what stops the two drifting apart.
  it('renders exactly the registry tabs, in registry order', () => {
    const decl = panelSrc.match(/const SETTINGS_TABS[^=]*=\s*\[([\s\S]*?)\n\]/)
    expect(decl, 'SettingsPanel.tsx must keep a literal SETTINGS_TABS array').toBeTruthy()
    const panelTabs = [...decl![1].matchAll(/id:\s*'([a-z]+)',\s*label:\s*'([^']*)'/g)].map((m) => ({
      id: m[1],
      label: decode(m[2]),
    }))
    expect(panelTabs).toEqual(SETTINGS_TABS.map((t) => ({ id: t.id, label: t.label })))
  })

  it('lists sections in the order the panel renders them', () => {
    // Membership is not enough. The generated manual walks this array, so an array that holds
    // the right sections in the wrong order publishes a reference that describes a panel nobody
    // has — which is the drift this registry exists to stop, just quieter. Caught exactly that
    // way: the JSX move that put Audio under the CAT rows left this array still listing it after
    // the satellite stack, and the manual dutifully documented the old order.
    // Compared PER TAB, deliberately. The panel's source order is not the rail's order — the
    // Appearance blocks are declared first in the file — and only the active tab ever mounts, so
    // cross-tab source order is invisible to an operator and meaningless to the manual. What
    // must agree is the order WITHIN a page.
    const rendered = [...panelSrc.matchAll(/id="settings-([a-z0-9-]+)"/g)].map((m) => m[1])
    // Nested disclosures render inside their parent's fieldset, so compare only top-level ones.
    const nested = new Set(SETTINGS_SECTIONS.filter((s) => s.parent).map((s) => s.id))
    const byId = new Map(SETTINGS_SECTIONS.map((s) => [s.id, s]))
    for (const tab of SETTINGS_TABS) {
      const declared = SETTINGS_SECTIONS.filter((s) => s.tab === tab.id && !nested.has(s.id)).map(
        (s) => s.id,
      )
      const actual = rendered.filter((id) => !nested.has(id) && byId.get(id)?.tab === tab.id)
      expect(actual, `${tab.label} renders its sections in a different order than declared`).toEqual(
        declared,
      )
    }
  })

  it('anchors every section it renders', () => {
    // The anchor is what a deep link scrolls to; a section without one is reachable only by
    // landing on its tab and scrolling by eye, which is the state this whole change exists to end.
    const anchored = new Set(
      [...panelSrc.matchAll(/id="settings-([a-z0-9-]+)"/g)].map((m) => m[1]),
    )
    const groupAnchored = new Set(
      [...panelSrc.matchAll(/<SettingsGroup\s+id="([a-z0-9-]+)"/g)].map((m) => m[1]),
    )
    const missing = SETTINGS_SECTIONS.filter(
      (s) => !anchored.has(s.id) && !groupAnchored.has(s.id),
    ).map((s) => s.id)
    expect(missing, 'sections with no DOM anchor in SettingsPanel.tsx').toEqual([])
  })
})

describe('the Advanced rule', () => {
  // (minority ∧ never-needed-in-hour-one ∧ cannot break the default path). Only the middle
  // clause is machine-checkable, and it is the one Issue #62 turned on.
  it('hides nothing behind a disclosure that a first-hour operator needs', () => {
    const violations = SETTINGS_SECTIONS.filter((s) => s.advanced && s.neededInHourOne).map(
      (s) => `${s.id} is advanced:true but neededInHourOne:true`,
    )
    expect(
      violations,
      'a setting needed to get on the air must not hide in a collapsed group — that is the ' +
        'Issue #62 failure (an operator filed a feature request for a toggle that shipped)',
    ).toEqual([])
  })

  it('only nests advanced sections inside a parent', () => {
    // An `advanced` section that is not nested is a contradiction: there is no disclosure to
    // hide it in, so the flag is claiming a protection the panel does not provide.
    const loose = SETTINGS_SECTIONS.filter((s) => s.advanced && !s.parent).map((s) => s.id)
    expect(loose).toEqual([])
  })
})

describe('density — the guard the four prior reorgs lacked', () => {
  // Sections-per-tab is what an operator scans. It grew 10→19→22→26→36 across four reorgs, each
  // of which optimised TAB COUNT instead. Raising this ceiling must be a deliberate edit with a
  // reason, not a side effect of landing a feature.
  const MAX_SECTIONS_PER_TAB = 12

  it('keeps every tab under the density ceiling', () => {
    const over = SETTINGS_TABS.map((t) => ({ id: t.id, n: sectionsForTab(t.id).length }))
      .filter((x) => x.n > MAX_SECTIONS_PER_TAB)
      .map((x) => `${x.id} has ${x.n} sections (ceiling ${MAX_SECTIONS_PER_TAB})`)
    expect(
      over,
      'split the tab or move a section — do not raise the ceiling to make this pass without ' +
        'deciding that a deeper page is genuinely better for the operator',
    ).toEqual([])
  })

  it('gives every section searchable words beyond its own label', () => {
    // The vocabulary operators type lives in hints, not labels: nothing in "Rig Control" or
    // "Audio" contains "COM port" or "sound card". A section with no keywords is unfindable by
    // anyone who does not already know its name — which is the population that needs search.
    const bare = SETTINGS_SECTIONS.filter((s) => s.keywords.length === 0).map((s) => s.id)
    expect(bare).toEqual([])
    const lazy = SETTINGS_SECTIONS.filter((s) => {
      const label = s.label.toLowerCase()
      return s.keywords.every((k) => label.includes(k.toLowerCase()))
    }).map((s) => s.id)
    expect(lazy, 'keywords that only repeat the label add nothing').toEqual([])
  })
})

describe('resolveTarget — a stale pointer must land, never dead-end', () => {
  it('resolves a plain tab id and a tab label', () => {
    expect(resolveTarget('radio')).toEqual({ tab: 'radio' })
    expect(resolveTarget('Logging & Connectors')).toEqual({ tab: 'logging' })
  })

  it('resolves a section to its tab AND the section', () => {
    expect(resolveTarget('audio')).toEqual({ tab: 'radio', section: 'audio' })
    expect(resolveTarget('Confirmations')).toEqual({ tab: 'logging', section: 'confirmations' })
  })

  it('resolves the path forms the app and docs actually write', () => {
    expect(resolveTarget('Settings ▸ Radio ▸ Audio')).toEqual({ tab: 'radio', section: 'audio' })
    expect(resolveTarget('Settings -> Radio')).toEqual({ tab: 'radio' })
  })

  it('lands a pointer written against the old Modes tab on the section it named', () => {
    // The section wins over the (now-deleted) tab name, so the hundreds of pointers written as
    // "Settings ▸ Modes ▸ X" keep landing exactly — on X's NEW tab, without being rewritten.
    // This is what makes the split safe to ship ahead of the pointer sweep.
    expect(resolveTarget('Settings > Modes > CW')).toEqual({ tab: 'cw', section: 'cw' })
    expect(resolveTarget('Settings ▸ Modes ▸ APRS')).toEqual({ tab: 'digital', section: 'aprs' })
    expect(resolveTarget('Settings ▸ Modes ▸ RTTY')).toEqual({ tab: 'digital', section: 'rtty' })
    // A bare "Modes" can no longer name one page; it lands on Digital, which kept most of it.
    expect(resolveTarget('Settings ▸ Modes')).toEqual({ tab: 'digital' })
    // Same for the folded-away Frequencies tab.
    expect(resolveTarget('Settings ▸ Frequencies')).toEqual({ tab: 'digital' })
  })

  it('lands every legacy tab name still live in the tree', () => {
    // These four are the residue the 0.17 sweep missed and are STILL cited in the app today:
    // Features ×11, Logbook & QSL ×6, Integrations ×4, Connections ×3.
    expect(resolveTarget('Settings ▸ Features')?.tab).toBe('appearance')
    expect(resolveTarget('Settings ▸ Logbook & QSL')?.tab).toBe('logging')
    expect(resolveTarget('Settings ▸ Integrations')?.tab).toBe('logging')
    expect(resolveTarget('Settings ▸ Connections')?.tab).toBe('logging')
    // And the pre-0.17 names for what is now the Radio tab.
    expect(resolveTarget('Settings ▸ Rig / CAT')?.tab).toBe('radio')
    expect(resolveTarget('Settings ▸ Rig Control')?.tab).toBe('radio')
  })

  it('every alias resolves to a live tab', () => {
    const tabIds = new Set(SETTINGS_TABS.map((t) => t.id))
    const dead = Object.entries(TAB_ALIASES)
      .filter(([, v]) => !tabIds.has(v))
      .map(([k, v]) => `${k} -> ${v}`)
    expect(dead).toEqual([])
  })

  it('prefers the section when a legacy name is also a live section', () => {
    // "Audio" is both a pre-0.17 tab name and a live section. Landing on the section is
    // strictly better: the operator asked for the control, not the page that now holds it.
    expect(resolveTarget('Settings ▸ Audio')).toEqual({ tab: 'radio', section: 'audio' })
  })

  it('resolves a section id however the operator capitalised it', () => {
    // Prose names a mode the way a ham writes it — "Settings ▸ Q65", "MSK144", "JT65" — not in
    // the lowercase the id happens to use. Case-sensitive lookup silently returned null for
    // every one of those, which is the dead-end this resolver exists to prevent.
    expect(resolveTarget('Q65')).toEqual({ tab: 'digital', section: 'q65' })
    expect(resolveTarget('MSK144')).toEqual({ tab: 'digital', section: 'msk144' })
    expect(resolveTarget('Settings ▸ JT65')).toEqual({ tab: 'digital', section: 'jt65' })
    expect(sectionById('APRS')?.id).toBe('aprs')
  })

  it('returns null rather than guessing at something it does not know', () => {
    expect(resolveTarget('')).toBeNull()
    expect(resolveTarget('Settings ▸ Nonexistent Thing')).toBeNull()
  })
})

describe('slugs agree with the generated manual', () => {
  it('produces the anchor form the docs contract uses', () => {
    expect(sectionSlug('Operator & Radio')).toBe('operator-radio')
    expect(sectionSlug('Digital (FT8/FT4)')).toBe('digital-ft8ft4')
    expect(sectionSlug('Spots & Alerts')).toBe('spots-alerts')
  })

  it('gives every section a unique slug', () => {
    const slugs = SETTINGS_SECTIONS.map((s) => sectionSlug(s.label))
    const dupes = slugs.filter((s, i) => slugs.indexOf(s) !== i)
    expect(dupes, 'two sections sharing a slug collide as doc anchors').toEqual([])
  })
})

describe('searchSettings — the operator types their own words', () => {
  const ids = (q: string) => searchSettings(q).map((h) => h.section.id)

  it('finds a section by the words that are NOT in its label', () => {
    // The whole reason this searches keywords. None of these strings appear in the heading the
    // operator would have to guess, and each is what a real report called the thing.
    expect(ids('com port')).toContain('rig-control') // heading says "Rig & CAT"
    expect(ids('sound card')).toContain('audio') // heading says "Audio"
    expect(ids('pl tone')).toContain('phone') // heading says "Phone (SSB / FM)"
    expect(ids('wpm')).toContain('cw')
    expect(ids('keps')).toContain('orbital-elements')
    expect(ids('atno')).toContain('pounce')
    expect(ids('text size')).toContain('workspace')
  })

  it('finds the toggle from issue #62 by the words that report used', () => {
    // An FT-991A owner could not find "Data modes use plain SSB" — it was folded inside the
    // collapsed Advanced group — and filed a feature request for a capability that had already
    // shipped. It was closed with a hint rewrite; the findability cause is what this fixes.
    for (const q of ['plain ssb', 'data modes', 'no rf', 'rigblaster', 'mic jack']) {
      expect(ids(q), `"${q}" must reach the Advanced group`).toContain('rig-advanced')
    }
  })

  it('is punctuation- and case-blind, the way an operator types', () => {
    // ⚠️ THESE QUERIES ARE CHOSEN SO ONLY THE CODE CAN SATISFY THEM. The first version of this
    // test used 'CI-V' and 'civ', both of which are LISTED KEYWORDS — so it passed with the
    // punctuation handling deliberately removed, and proved nothing. Every spelling below is
    // absent from `keywords`, so each one fails unless the search really is punctuation-blind.
    expect(ids('wsjtx')).toContain('integrations-feeds') // keyword is 'wsjt-x'
    expect(ids('civ')).toContain('rig-control') // (a listed keyword, but see 'ciV' below)
    expect(ids('ciV')).toContain('rig-control') // case-blind
    expect(ids('pltone')).toContain('phone') // keyword is 'pl tone'
    expect(ids('soundcard')).toContain('audio') // matches 'sound card' either way
    expect(ids('comport')).toContain('rig-control') // keyword is 'com port'
    expect(ids('stopbits')).toContain('rig-control') // keyword is 'stop bits'
  })

  it('requires every word of a multi-word query to land', () => {
    // Otherwise the weaker half drags in everything: "audio contest" would return Audio.
    expect(ids('audio device')).toContain('audio')
    expect(ids('audio contest')).toEqual([])
  })

  it('ranks an exact label above a mere keyword mention', () => {
    // "CW" is the CW section's whole label and also a keyword elsewhere.
    expect(searchSettings('cw')[0].section.id).toBe('cw')
  })

  it('says WHAT matched, so a result never looks arbitrary', () => {
    const hit = searchSettings('sound card').find((h) => h.section.id === 'audio')
    expect(hit?.matched).toBe('sound card')
  })

  it('returns nothing for a blank query, and for a word nobody uses', () => {
    expect(searchSettings('')).toEqual([])
    expect(searchSettings('   ')).toEqual([])
    expect(searchSettings('zzzznotasetting')).toEqual([])
  })

  it('every hit resolves to a real landing place', () => {
    // Search that finds a section it cannot navigate to is worse than no search.
    for (const q of ['com port', 'sound card', 'plain ssb', 'aprs', 'lotw', 'keyer']) {
      for (const h of searchSettings(q)) {
        const t = resolveTarget(h.section.id)
        expect(t, `${q} -> ${h.section.id} must resolve`).not.toBeNull()
        expect(t?.section).toBe(h.section.id)
      }
    }
  })
})

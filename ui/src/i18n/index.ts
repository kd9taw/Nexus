// THE i18n RUNTIME — string lookup, interpolation, plurals, and the invariant-token rule.
//
// Zero dependencies, on purpose. The alternatives (react-i18next, LinguiJS, react-intl) all
// carry a plugin/backend architecture — loaders that fetch catalogs, language detectors, ICU
// message compilers — that this application must not have: Nexus runs in a field-day tent with
// no network, so every catalog is a static import, and the plural data every one of those
// libraries ships is already in the webview as `Intl.PluralRules`. What is left after removing
// what we must not use is this file. See `docs/guide/i18n.md` for the full comparison.
//
// SHAPE: this is the `units.ts` pattern one level up — pure formatting at the DISPLAY EDGE,
// nothing here ever touches a wire value, a persisted value, or a value on its way to the
// radio. Read that sentence again before adding anything to this module.
//
// ---------------------------------------------------------------------------------------
// ⚠️ THE INVARIANT-TOKEN RULE — the part of i18n that can put a station off frequency.
// ---------------------------------------------------------------------------------------
//
// Localisation applies to PROSE. It does not apply to the technical vocabulary of amateur
// radio, and the difference is not stylistic: `14,074` is not `14.074`, and a decimal comma
// that reaches a dial, a log, a settings file or a CAT command is an operating fault, not a
// cosmetic one. A German or Greek Windows install is one `toLocaleString()` away from it.
//
// TWO CATEGORIES, and every string in the app is in exactly one:
//
//   TRANSLATABLE PROSE — labels, hints, tooltips, button text, error sentences, empty states,
//   accessible names, and human example values (a first name, "leave blank if that is you").
//   These become catalog entries.
//
//   INVARIANT TECHNICAL TOKENS — never translated, never locale-formatted, never in a catalog:
//     • frequencies and any number that is one (dial MHz, offsets, shifts, tone Hz, baud)
//     • signal reports (599, -14 dB, S9+20), power in watts, WPM, percentages of a technical
//       quantity
//     • callsigns, grid squares, DXCC prefixes, ITU/CQ zones, ARRL/RAC section codes
//     • band names (20m), mode names (FT8, USB, PSK31), Q-codes (QSY, QRZ), RST, SOTA/POTA refs
//     • ADIF field names (OPERATOR, STATION_CALLSIGN) and macro tokens ({NAME}, {MYSTATE})
//     • every `value` of a <select>, every persisted key, every id — the LABEL is prose, the
//       VALUE is a token, and they are not the same string
//     • example values that are drawn from a technical namespace (`KD9TAW`, `EN52xa`, `WI`)
//
// MECHANICALLY, that means:
//   1. Interpolated numbers are stringified by `invariantNumber` (below) — `String(n)`, which
//      is always `.`-decimal and never grouped. `t()` CANNOT emit a localised number.
//   2. Nothing in this module, or in any module it calls, uses `toLocaleString`,
//      `toLocaleDateString`, `Intl.NumberFormat` or `Intl.DateTimeFormat`. The one `Intl` API
//      used is `PluralRules`, which returns a category name, not a formatted number.
//   3. A component that needs a number in prose formats it itself, invariantly, and passes the
//      finished string. If a future surface genuinely wants a grouped count ("1,234 QSOs"),
//      that formatter goes at that call site with the category rule quoted, never in `t()`.
//   4. The active locale is a PROSE setting. It must never be read by, derived from, or
//      written to anything that formats, parses, persists or transmits a number. `units.ts`
//      already sniffs the OS locale for metric/imperial DISPLAY — that is the boundary, and it
//      converts at the display edge for the same reason.
//
// `i18n.invariant.test.ts` holds this to a positive control: it proves a de-DE formatter
// really does produce a comma (so the check can fail) and then proves our path does not.
//
// ---------------------------------------------------------------------------------------
// MISSING TRANSLATIONS FALL BACK TO ENGLISH — always, structurally.
// ---------------------------------------------------------------------------------------
//
// `rawMessage` reads the active catalog, then the English one. A key the active locale lacks —
// or has as an empty string, which is what a half-finished machine translation looks like —
// resolves to the English source string. The operator sees English, never a key and never a
// blank. The English catalog cannot itself have a hole: `MessageKey` is `keyof typeof EN`.

import { EN } from './en'
import type { Message, Params, PluralForms, RichNode } from './types'

export type { Message, Params, PluralForms, RichNode } from './types'
export { EN } from './en'

/** Every key the app may ask for. A key outside this union does not compile. */
export type MessageKey = keyof typeof EN

/** A locale's catalog. Only English must be complete; every other locale is an overlay. */
export type PartialCatalog = Partial<Record<MessageKey, Message>>

/** The locale the source strings are written in, and the fallback for every other. */
export const SOURCE_LOCALE = 'en'

// ── Invariant technical tokens ────────────────────────────────────────────────────────

/**
 * Stringify a number for insertion into prose WITHOUT locale formatting.
 *
 * `String(n)` is the whole implementation and the whole point: always `.`-decimal, never
 * grouped, identical on every OS locale. This is what `t()` uses for numeric params, so no
 * interpolated number can ever pick up a decimal comma. Use it directly wherever a component
 * renders a technical number outside a catalog string.
 */
export function invariantNumber(n: number): string {
  return String(n)
}

// ── Catalog registry ──────────────────────────────────────────────────────────────────

const catalogs = new Map<string, PartialCatalog>([[SOURCE_LOCALE, EN]])
let active: string = SOURCE_LOCALE

/**
 * Register a locale's catalog. This is how a language is added: import its catalog statically
 * at startup and install it. There is no loader and there will not be one — a catalog that
 * arrives over the network is a catalog that is absent when the operator is off-grid.
 */
export function installCatalog(locale: string, catalog: PartialCatalog): void {
  catalogs.set(locale, catalog)
}

/** Locales with a catalog installed, English first. */
export function availableLocales(): string[] {
  return [SOURCE_LOCALE, ...[...catalogs.keys()].filter((l) => l !== SOURCE_LOCALE).sort()]
}

/**
 * Switch the prose locale. An unknown locale is IGNORED and English stays active — a typo in a
 * setting must not blank the interface.
 *
 * ⚠️ Nothing in the app re-renders on this today, because nothing changes it: phase 1 ships
 * English only. Before a locale picker exists, this needs the subscribe/notify pair `units.ts`
 * uses (a window event + a hook), or a switch will leave the mounted tree in the old language.
 */
export function setLocale(locale: string): void {
  if (catalogs.has(locale)) active = locale
}

/** The active prose locale. Never consult this to format a number — see the rule above. */
export function getLocale(): string {
  return active
}

// ── Lookup ────────────────────────────────────────────────────────────────────────────

/**
 * One `Intl.PluralRules` per locale, built on first use and kept.
 *
 * ⚠️ THE ONLY EXPENSIVE CALL IN THIS MODULE. Constructing an `Intl` object is orders of
 * magnitude dearer than the Map and property lookups around it — everything else here is
 * effectively free next to React's own render work, and this is not. Today every plural
 * string is a per-VIEW counter (a band-map header, a spots filter count), so a per-call
 * construction would have been survivable; the moment one lands in a decode row or a roster
 * row it is paid per row, per frame, and that is a change nobody would connect to this file.
 * Built once instead. `i18n.test.ts` counts the constructions so a later edit cannot quietly
 * move it back inside the call.
 *
 * A `null` entry means the tag was rejected — remembered deliberately, so a bad locale costs
 * one throw rather than one per lookup. Bounded by the number of distinct tags asked for,
 * which in practice is the number of installed catalogs.
 */
const pluralRules = new Map<string, Intl.PluralRules | null>()

function pluralRulesFor(locale: string): Intl.PluralRules | null {
  const hit = pluralRules.get(locale)
  if (hit !== undefined) return hit
  let made: Intl.PluralRules | null = null
  try {
    made = new Intl.PluralRules(locale)
  } catch {
    made = null
  }
  pluralRules.set(locale, made)
  return made
}

/**
 * Select a plural form for `count` under `locale`.
 *
 * `Intl.PluralRules` owns the per-language rules, so this stays a few lines however many
 * languages ship. A category the catalog does not carry falls to `other`, which is required.
 */
export function selectPluralForm(locale: string, count: number, forms: PluralForms): string {
  const rules = pluralRulesFor(locale)
  // A structurally invalid tag. English-ish two-form behaviour is a better guess than a throw.
  const category: keyof PluralForms = rules
    ? (rules.select(count) as keyof PluralForms)
    : count === 1
      ? 'one'
      : 'other'
  return forms[category] ?? forms.other
}

/** One catalog's answer for a key, or undefined when it has nothing usable. */
function lookIn(
  catalog: PartialCatalog | undefined,
  key: MessageKey,
  locale: string,
  count: number | undefined,
): string | undefined {
  const entry: Message | undefined = catalog?.[key]
  if (entry === undefined) return undefined
  const text =
    typeof entry === 'string'
      ? entry
      : count === undefined
        ? entry.other
        : selectPluralForm(locale, count, entry)
  // An empty or whitespace-only string is what a half-finished translation looks like. Treat
  // it as missing so it falls through to English rather than blanking the control.
  return text.trim() === '' ? undefined : text
}

/**
 * The message for `key`, plural-selected but NOT interpolated.
 *
 * Exported for the rich-text renderer, which must parse markup markers before values are
 * substituted (so a value containing `<b>` can never become markup).
 */
export function rawMessage(key: MessageKey, params?: Params): string {
  const count = typeof params?.count === 'number' ? params.count : undefined
  return (
    lookIn(catalogs.get(active), key, active, count) ??
    lookIn(EN, key, SOURCE_LOCALE, count) ??
    // Unreachable: `key` is `keyof typeof EN`, so English always has it. Returning the key
    // rather than throwing keeps a hypothetical hole from taking the window down.
    key
  )
}

/**
 * Substitute `{{name}}` placeholders. Single braces are left alone — `{NAME}` and `{MYSTATE}`
 * are CW macro tokens that appear inside real hints and must reach the operator intact.
 *
 * An unsupplied placeholder is left verbatim rather than blanked: `{{call}}` in the interface
 * is a visible, greppable bug; a silent gap in a sentence is not.
 */
export function interpolate(text: string, params?: Params): string {
  if (!params) return text
  return text.replace(/\{\{(\w+)\}\}/g, (whole, name: string) => {
    const v = params[name]
    if (v === undefined) return whole
    return typeof v === 'number' ? invariantNumber(v) : v
  })
}

/**
 * The translated string for `key`.
 *
 * Use for attributes — `title`, `placeholder`, `aria-label` — and anywhere a plain string is
 * required. For visible text that carries emphasis, use `<T>` instead (./T.tsx).
 */
export function t(key: MessageKey, params?: Params): string {
  return interpolate(rawMessage(key, params), params)
}

// ── Rich text ─────────────────────────────────────────────────────────────────────────

/** The `</tag>` closing the `<tag>` that opened at `from`, honouring same-name nesting. */
function findClose(text: string, tag: string, from: number): number {
  const open = `<${tag}>`
  const close = `</${tag}>`
  let depth = 1
  let i = from
  while (i < text.length) {
    const o = text.indexOf(open, i)
    const c = text.indexOf(close, i)
    if (c < 0) return -1
    if (o >= 0 && o < c) {
      depth++
      i = o + open.length
    } else {
      depth--
      if (depth === 0) return c
      i = c + close.length
    }
  }
  return -1
}

/**
 * Split a message into text and marker spans.
 *
 * Only names in `knownTags` — the ones the call site supplied an element for — are markers;
 * every other `<…>` stays literal text. That is the safety property: a catalog string, however
 * it was produced, cannot introduce an element, an attribute or a handler. An unclosed marker
 * also degrades to text rather than swallowing the rest of the sentence.
 */
export function parseRich(text: string, knownTags: readonly string[]): RichNode[] {
  const known = new Set(knownTags)
  const out: RichNode[] = []
  let buf = ''
  let i = 0
  const flush = () => {
    if (buf !== '') {
      out.push(buf)
      buf = ''
    }
  }
  while (i < text.length) {
    const lt = text.indexOf('<', i)
    if (lt < 0) {
      buf += text.slice(i)
      break
    }
    const m = /^<([a-zA-Z][a-zA-Z0-9]*)>/.exec(text.slice(lt))
    const tag = m?.[1]
    const close = tag && known.has(tag) ? findClose(text, tag, lt + m![0].length) : -1
    if (!tag || !known.has(tag) || close < 0) {
      buf += text.slice(i, lt + 1)
      i = lt + 1
      continue
    }
    buf += text.slice(i, lt)
    flush()
    out.push({ tag, children: parseRich(text.slice(lt + m![0].length, close), knownTags) })
    i = close + tag.length + 3 // `</tag>`
  }
  flush()
  return out
}

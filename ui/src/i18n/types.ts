// Catalog value types, split out so `en.ts` can be shape-checked without importing the runtime
// (a value cycle between the catalog and the runtime that reads it would be a real one; a
// type-only import is erased).

/** Interpolation values. `number` is stringified INVARIANTLY — see `invariantNumber`. */
export type Params = Record<string, string | number>

/**
 * A message with plural forms. `other` is required and is the fallback for every category a
 * locale (or a translator) did not supply; the rest are the CLDR categories `Intl.PluralRules`
 * can select. English only ever needs `one` + `other`; Polish needs `few`/`many`; Arabic uses
 * all six. Nothing here enumerates locales — `Intl.PluralRules` owns that knowledge and ships
 * with the webview, so adding a language costs no rule data.
 */
export interface PluralForms {
  zero?: string
  one?: string
  two?: string
  few?: string
  many?: string
  other: string
}

/** One catalog entry: a plain string, or a set of plural forms selected by `params.count`. */
export type Message = string | PluralForms

/**
 * A parsed rich-text message. Markup in a catalog string is a *marker*, never HTML: the
 * renderer maps a marker name to a React element the CALL SITE supplied, so a catalog can
 * never inject an element, an attribute or a handler. See `parseRich`.
 */
export type RichNode = string | { tag: string; children: RichNode[] }

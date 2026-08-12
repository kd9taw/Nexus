// The FT country-exclusion filter — a DISPLAY filter for experienced chasers who want the
// high-density countries out of the way. Requested by the operator: "get rid of specific
// countries … it needs to be under 20 and the top ones people would exclude."
//
// ## What it is allowed to touch
//
// The RX display, and nothing else. It never reaches the decoder, the logbook, the QSO
// path, or the needed-entity alerts. That is why the state lives HERE, in localStorage,
// rather than in the Rust Settings round-trip: with the list absent from the backend the
// decoder structurally cannot consult it, instead of it being a rule someone has to
// remember not to break.
//
// Three further protections make it a filter rather than a trap, and they live in
// `isHiddenByCountry` so the roster and Band Activity cannot drift apart on them:
//   - a station we are IN A QSO WITH is never hidden mid-exchange;
//   - a message addressed to us (or our own TX) is never hidden — someone calling US
//     outranks any view filter;
//   - NEEDED OUTRANKS EXCLUDED: a new entity or a new band slot still surfaces.
//
// ## Why the identity is the entity NAME
//
// There is no numeric DXCC id anywhere in this codebase — `DxccInfo.entity` is a
// `&'static str`, and `DecodeRow.country` / `Station.country` are that string, resolved
// backend-side by `propagation::dxcc::resolve` (see src-tauri/src/lib.rs). Matching on the
// resolved entity is therefore both the only option and the correct one: it handles the
// portable forms `dxcc.rs::base_call` already normalizes (`VE3XYZ/W1` is United States,
// `W1ABC/VE3` is Canada) and every prefix block an entity owns, which a text prefix on the
// callsign would get wrong in both directions.
//
// The consequence is that a NAME is load-bearing, and a wrong one fails SILENTLY — it
// simply never matches, and the operator ticks a box that does nothing. Two defences:
// `entity` is stored separately from everything the operator reads (the prefix in a label
// is vocabulary, never the matching rule — the operator says "Germany (DL)" where cty.dat
// says "Fed. Rep. of Germany"), and countryExclude.test.tsx pins every one of the 18
// against the vendored cty.dat so an upstream respelling fails loudly instead.
//
// ## Why the key is app-global and bare
//
// NOT `surfaceGet`/`surfaceSet`. Per-surface scoping exists so a docked and a popped-out
// copy of a view do not fight over one value — right for panel visibility and rail widths,
// wrong here. A country-exclusion list is a standing statement about how this operator
// chases, not a property of a window: under `surfaceGet` a torn-off band map would inherit
// once and then diverge on its first toggle, so the pop-out would show the DL decodes the
// main window hides. One bare key, and the convergence below keeps every window on it.

import { useCallback, useEffect, useMemo, useState } from 'react'
import type { NeedTag } from '../types'

/** One excludable country. `key` is the persisted identity; `entity` is the matching one. */
export interface ExcludableCountry {
  /** Stable short key — what goes on disk. Deliberately neither the entity name (an
   *  upstream respelling would orphan the operator's list) nor the prefix. */
  key: string
  /** What the operator calls it. */
  name: string
  /** Callsign prefix, shown so the label reads in operator vocabulary ("Germany (DL)").
   *  DISPLAY ONLY — never the matching rule. */
  prefix: string
  /** The cty.dat entity name: exactly the string `DecodeRow.country` / `Station.country`
   *  carry. Pinned against the vendored cty.dat by countryExclude.test.tsx. */
  entity: string
}

/**
 * The curated 18. The operator's constraint was explicit — "I can't have a huge list of
 * countries, it needs to be under 20 and the top ones people would exclude" — so this is a
 * fixed catalog of the highest-density FT entities, not the full 340-entity DXCC table.
 * Ordered roughly by how much of a busy waterfall each one accounts for, because that is
 * the order the operator scans it in.
 *
 * S5, XE and VE are here at the operator's specific request.
 */
export const EXCLUDABLE_COUNTRIES: readonly ExcludableCountry[] = [
  { key: 'us', name: 'United States', prefix: 'K/W/N', entity: 'United States' },
  { key: 've', name: 'Canada', prefix: 'VE', entity: 'Canada' },
  { key: 'xe', name: 'Mexico', prefix: 'XE', entity: 'Mexico' },
  // The trap this table exists for: the operator says Germany, cty.dat says
  // "Fed. Rep. of Germany". Matching on the label would hide nothing, forever.
  { key: 'dl', name: 'Germany', prefix: 'DL', entity: 'Fed. Rep. of Germany' },
  { key: 'i', name: 'Italy', prefix: 'I', entity: 'Italy' },
  { key: 'ea', name: 'Spain', prefix: 'EA', entity: 'Spain' },
  { key: 'g', name: 'England', prefix: 'G', entity: 'England' },
  { key: 'f', name: 'France', prefix: 'F', entity: 'France' },
  { key: 'ja', name: 'Japan', prefix: 'JA', entity: 'Japan' },
  { key: 'py', name: 'Brazil', prefix: 'PY', entity: 'Brazil' },
  { key: 'lu', name: 'Argentina', prefix: 'LU', entity: 'Argentina' },
  { key: 'sp', name: 'Poland', prefix: 'SP', entity: 'Poland' },
  { key: 'ua', name: 'European Russia', prefix: 'UA', entity: 'European Russia' },
  { key: 'ur', name: 'Ukraine', prefix: 'UR', entity: 'Ukraine' },
  { key: 'pa', name: 'Netherlands', prefix: 'PA', entity: 'Netherlands' },
  { key: 'ok', name: 'Czech Republic', prefix: 'OK', entity: 'Czech Republic' },
  { key: 's5', name: 'Slovenia', prefix: 'S5', entity: 'Slovenia' },
  { key: 'by', name: 'China', prefix: 'BY', entity: 'China' },
]

/** "Germany (DL)" — country and prefix, as the operator asked for. */
export function countryLabel(c: ExcludableCountry): string {
  return `${c.name} (${c.prefix})`
}

const BY_KEY = new Map(EXCLUDABLE_COUNTRIES.map((c) => [c.key, c]))

export const COUNTRY_EXCLUDE_KEY = 'nexus.decodes.countryExclude'
/** Same-window broadcast. The native `storage` event does NOT fire in the document that
 *  wrote the value, so it alone cannot keep two panes in ONE window in step — the
 *  `nexus.waterfall.palette` precedent. */
const COUNTRY_EXCLUDE_EVENT = 'nexus:country-exclude'

/**
 * The ticked country keys.
 *
 * Everything unreadable falls back to EXCLUDING NOTHING, and the asymmetry is deliberate:
 * this filter's only failure mode that hurts is hiding rows the operator cannot explain
 * from the UI, so every ambiguity resolves toward showing more. A member this build does
 * not know is dropped rather than poisoning the whole list (the per-field narrowing
 * `loadRosterFilters` does) — there is no checkbox for a country we have never heard of,
 * so honouring it would hide rows with nothing on screen to account for them.
 */
export function loadCountryExclude(): ReadonlySet<string> {
  let raw: string | null
  try {
    raw = window.localStorage.getItem(COUNTRY_EXCLUDE_KEY)
  } catch {
    return new Set()
  }
  if (!raw) return new Set()
  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((k): k is string => typeof k === 'string' && BY_KEY.has(k)))
  } catch {
    return new Set()
  }
}

/** Write the ticked keys and tell every mounted consumer, in this window and the others. */
export function saveCountryExclude(keys: Iterable<string>): void {
  // Catalog order, so the stored value is stable regardless of the order they were ticked
  // in — a set that reorders itself makes every diff of this key unreadable.
  const set = new Set(keys)
  const ordered = EXCLUDABLE_COUNTRIES.filter((c) => set.has(c.key)).map((c) => c.key)
  try {
    window.localStorage.setItem(COUNTRY_EXCLUDE_KEY, JSON.stringify(ordered))
  } catch {
    /* full/unavailable — the choice still applies this session via the event below */
  }
  window.dispatchEvent(new Event(COUNTRY_EXCLUDE_EVENT))
}

/** Ticked keys → the cty.dat entity names a row's `country` is compared against. */
export function excludedEntities(keys: ReadonlySet<string>): ReadonlySet<string> {
  const out = new Set<string>()
  for (const k of keys) {
    const c = BY_KEY.get(k)
    if (c) out.add(c.entity)
  }
  return out
}

/**
 * The need claims that OUTRANK an exclusion.
 *
 * Only the two that are genuinely worth interrupting the operator's own choice for. A new
 * GRID is deliberately NOT here: in a country dense enough to be on this list, an unworked
 * grid is routine, so exempting it would leave the pane as full as it was and the filter
 * would read as broken. A new ENTITY or a new BAND SLOT is a real "new one".
 */
export const OVERRIDING_NEEDS: readonly NeedTag[] = ['NewEntity', 'NewBand']

/** Does this station carry a need that outranks the exclusion? (Roster side — the decode
 *  side reads the engine's own `newDxcc` / `newBand` flags, which say the same thing.) */
export function hasOverridingNeed(tags: readonly NeedTag[] | undefined): boolean {
  return tags != null && tags.some((t) => OVERRIDING_NEEDS.includes(t))
}

/** One row's country context. Both panes fill the same shape, so what "protected" means
 *  is decided once, here, and cannot drift between them. */
export interface CountryRow {
  /** The cty.dat-resolved entity (`DecodeRow.country` / `Station.country`). */
  entity?: string | null
  /** The row's callsign. */
  call?: string | null
  /** The Tx panel's current DX call — the station we are in a QSO with. */
  qsoPartner?: string | null
  /** The message is addressed to us, or is our own transmission. */
  addressedToMe?: boolean
  /** Carries a need that outranks the exclusion (see `OVERRIDING_NEEDS`). */
  needed?: boolean
}

const same = (a: string | null | undefined, b: string | null | undefined): boolean =>
  !!a && !!b && a.trim().toUpperCase() === b.trim().toUpperCase()

/**
 * Should this row be hidden by the country exclusion?
 *
 * `hidden` is the resolved ENTITY set from [`excludedEntities`]. A row with no resolved
 * entity is never hidden: absence is not a match, and guessing from the callsign text is
 * exactly the second resolver this filter must not grow.
 */
export function isHiddenByCountry(row: CountryRow, hidden: ReadonlySet<string>): boolean {
  if (hidden.size === 0 || !row.entity) return false
  if (row.addressedToMe) return false
  if (row.needed) return false
  if (same(row.call, row.qsoPartner)) return false
  return hidden.has(row.entity)
}

export interface CountryExcludeState {
  /** The ticked catalog keys. */
  keys: ReadonlySet<string>
  /** Those keys as cty.dat entity names — what [`isHiddenByCountry`] matches on. */
  hidden: ReadonlySet<string>
  /** Tick or untick one country. */
  toggle: (key: string) => void
  /** Untick every country (the one-click clear beside the hidden count). */
  clear: () => void
  /** Arbitrarily-picked entity NAMES beyond the curated 18 (F4MQS). */
  entities: ReadonlySet<string>
  /** Add/remove one entity by NAME from the arbitrary set. */
  toggleEntity: (entity: string) => void
  /** Paused: the ticks are kept but nothing is hidden. */
  paused: boolean
  /** Pause/resume the exclusion without losing the ticked set. */
  setPaused: (paused: boolean) => void
}

/**
 * Subscribe to the exclusion list. Every mounted consumer — both decode panes, the roster,
 * the picker, and the same panes again in a torn-off window — moves together.
 *
 * Both listeners RE-READ storage rather than trusting an event payload, so a window that
 * missed an event still converges on the next one and there is only ever one source of
 * truth for what is ticked.
 */
/** Pause the country exclusion WITHOUT losing the ticks (F4MQS: Clear discarded them). */
export const COUNTRY_EXCLUDE_PAUSED_KEY = 'nexus.decodes.countryExclude.paused'

/** Arbitrary DXCC entities picked beyond the curated 18 (F4MQS). Stored as entity NAMES
 *  (cty.dat's own, exactly what a decode row's `country` carries) in a SEPARATE key, so the
 *  curated-key flow and its cty.dat pin tests are untouched. */
export const COUNTRY_EXCLUDE_ENTITIES_KEY = 'nexus.decodes.countryExclude.entities'

function loadExcludedEntityNames(): ReadonlySet<string> {
  try {
    const raw = window.localStorage.getItem(COUNTRY_EXCLUDE_ENTITIES_KEY)
    if (!raw) return new Set()
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((k): k is string => typeof k === 'string' && k.length > 0))
  } catch {
    return new Set()
  }
}

function saveExcludedEntityNames(names: Iterable<string>): void {
  const ordered = [...new Set(names)].sort()
  try {
    window.localStorage.setItem(COUNTRY_EXCLUDE_ENTITIES_KEY, JSON.stringify(ordered))
  } catch {
    /* full/unavailable — applies this session via the event */
  }
  window.dispatchEvent(new Event(COUNTRY_EXCLUDE_EVENT))
}

function loadCountryExcludePaused(): boolean {
  try {
    return window.localStorage.getItem(COUNTRY_EXCLUDE_PAUSED_KEY) === '1'
  } catch {
    return false
  }
}

function saveCountryExcludePaused(paused: boolean): void {
  try {
    window.localStorage.setItem(COUNTRY_EXCLUDE_PAUSED_KEY, paused ? '1' : '0')
  } catch {
    /* ignore */
  }
  window.dispatchEvent(new Event(COUNTRY_EXCLUDE_EVENT))
}

export function useCountryExclude(): CountryExcludeState {
  const [keys, setKeys] = useState<ReadonlySet<string>>(loadCountryExclude)
  const [entities, setEntitiesState] = useState<ReadonlySet<string>>(loadExcludedEntityNames)
  const [paused, setPausedState] = useState<boolean>(loadCountryExcludePaused)

  useEffect(() => {
    const reread = () => {
      setKeys(loadCountryExclude())
      setEntitiesState(loadExcludedEntityNames())
      setPausedState(loadCountryExcludePaused())
    }
    const onStorage = (e: StorageEvent) => {
      if (
        e.key === COUNTRY_EXCLUDE_KEY ||
        e.key === COUNTRY_EXCLUDE_PAUSED_KEY ||
        e.key === COUNTRY_EXCLUDE_ENTITIES_KEY
      )
        reread()
    }
    window.addEventListener(COUNTRY_EXCLUDE_EVENT, reread)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(COUNTRY_EXCLUDE_EVENT, reread)
      window.removeEventListener('storage', onStorage)
    }
  }, [])

  const toggle = useCallback((key: string) => {
    // Built from STORAGE, not from this component's state, so two panes toggling in the
    // same tick cannot each write a list that drops the other's change.
    const next = new Set(loadCountryExclude())
    if (!next.delete(key)) next.add(key)
    saveCountryExclude(next)
  }, [])

  const toggleEntity = useCallback((entity: string) => {
    const next = new Set(loadExcludedEntityNames())
    if (!next.delete(entity)) next.add(entity)
    saveExcludedEntityNames(next)
  }, [])

  const clear = useCallback(() => {
    saveCountryExclude([])
    saveExcludedEntityNames([])
  }, [])
  const setPaused = useCallback((p: boolean) => saveCountryExcludePaused(p), [])

  // Paused → hide NOTHING while keeping the ticks (they resume on unpause). Otherwise the
  // resolved set is the curated keys' entities PLUS the arbitrarily-picked entity names.
  const hidden = useMemo(() => {
    if (paused) return new Set<string>()
    const out = new Set(excludedEntities(keys))
    for (const e of entities) out.add(e)
    return out
  }, [keys, entities, paused])
  return { keys, entities, hidden, toggle, toggleEntity, clear, paused, setPaused }
}

// The per-surface NATURAL FOOTPRINT registry, checked against the window geometry Rust
// actually builds. Two halves that only make sense together:
//
//   ui/src/useScale.ts      PANEL_NATURAL — the smallest UNZOOMED box in which a
//                           surface's content is whole (the fit denominator).
//   src-tauri/src/lib.rs    panel_default_inner / panel_min_inner — the box the OS
//                           window opens at, and the smallest it can be dragged to.
//
// They are NOT the same number and must not be conflated (that conflation is what
// pinned every pop-out to the 65% floor: a band map opens 780 tall but its track is
// height-elastic down to 240). What must hold between them is a RELATION, and TypeScript
// cannot see it because half of it is Rust:
//
//   1. natural ≤ default_inner (per axis) — a window that opens smaller than the box it
//      declares is asking for a scale it can never get.
//   2. natural ≤ min_inner / 0.65 (per axis) — auto-fit floors at 65%, so at the window's
//      SMALLEST legal size the effective box is min_inner/0.65 CSS px. A natural larger
//      than that claims content fits where it provably cannot, and the failure direction
//      is the silent one (clipping, unreachable controls) rather than the loud one
//      (small text). This is the guard that keeps a future entry honest.
//   3. at its default size every pop-out fits its own natural whole — i.e. auto-fit gives
//      100 under the default cap. `operate` is the one deliberate exception (below).
//
// Reads the Rust the way wire-consistency.test.ts reads dto.rs, and walks src/ for the
// openable slugs the way storage-scope.test.ts walks it — the call sites are the
// authoritative list (`memories` and `sats` fall through Rust's `other =>` title arm, so
// parsing Rust alone would silently miss two surfaces).
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { fitScale, naturalFor, MAIN_NATURAL, type Natural } from './useScale'

const SRC = fileURLToPath(new URL('.', import.meta.url))
const lib = readFileSync(fileURLToPath(new URL('../../src-tauri/src/lib.rs', import.meta.url)), 'utf8')

/** Every panel slug some component can actually pop out. */
function openableSlugs(): string[] {
  const found = new Set<string>()
  for (const rel of readdirSync(SRC, { recursive: true }) as string[]) {
    if (!/\.tsx?$/.test(rel) || /\.test\.tsx?$/.test(rel)) continue
    const text = readFileSync(join(SRC, rel), 'utf8')
    for (const m of text.matchAll(/openPanelWindow\(\s*'([A-Za-z0-9]+)'/g)) found.add(m[1])
  }
  return [...found].sort()
}

/** `fn <name>(...) { match slug { "a" | "b" => (W.0, H.0), … _ => (W.0, H.0) } }` as a
 *  lookup, with `_` under the `''` key. Throws (never silently empties) if the shape
 *  moves — an unparsed table would turn every assertion below into a no-op. */
function rustSizes(fnName: string): Map<string, [number, number]> {
  const body = new RegExp(`fn ${fnName}\\(slug: &str\\) -> \\(f64, f64\\) \\{([\\s\\S]*?)\\n\\}`).exec(lib)
  if (!body) throw new Error(`${fnName} not found in src-tauri/src/lib.rs`)
  const out = new Map<string, [number, number]>()
  for (const m of body[1].matchAll(/((?:"[A-Za-z0-9]+"(?:\s*\|\s*)?)+|_)\s*=>\s*\(([\d.]+),\s*([\d.]+)\)/g)) {
    const size: [number, number] = [Number(m[2]), Number(m[3])]
    if (m[1] === '_') out.set('', size)
    else for (const q of m[1].matchAll(/"([A-Za-z0-9]+)"/g)) out.set(q[1], size)
  }
  if (!out.has('')) throw new Error(`${fnName}: no fallback arm parsed`)
  return out
}

const DEFAULT_INNER = rustSizes('panel_default_inner')
const MIN_INNER = rustSizes('panel_min_inner')
const sizeFor = (t: Map<string, [number, number]>, slug: string) => t.get(slug) ?? t.get('')!
/** Auto-fit's hard floor — the scale below which it never goes (Z_MIN in useScale.ts). */
const FLOOR = 0.65

describe('pop-out natural footprints vs the windows Rust builds', () => {
  it('parses both Rust tables (a silent parse failure would void every case below)', () => {
    expect(DEFAULT_INNER.size).toBeGreaterThan(1)
    expect(sizeFor(DEFAULT_INNER, 'bandmapCw')).toEqual([420, 780])
    expect(sizeFor(MIN_INNER, 'waterfall')).toEqual([380, 180])
    expect(sizeFor(MIN_INNER, 'connect')).toEqual([420, 360])
  })

  it('finds every openable panel by scanning the call sites', () => {
    // Rust's title match knows nothing of `memories`/`sats` — they take its `other =>`
    // arm — so the components are the only complete list.
    expect(openableSlugs()).toEqual([
      'bandmapCw',
      'bandmapPhone',
      'connect',
      'dxped',
      'fieldday',
      'memories',
      'needed',
      'operate',
      'pota',
      'sats',
      'waterfall',
    ])
  })

  // `operate` hosts the SAME dense cockpit as the main window, so 1200×900 is genuinely
  // the question its window should be asked — it is the one surface whose natural is
  // larger than the window it opens in (1140×760 → 80%, exactly as before this change).
  // Stated as its own case so the exception stays explicit and cannot spread by accident.
  it('operate is the one surface that keeps the main cockpit footprint', () => {
    expect(naturalFor('operate')).toEqual(MAIN_NATURAL)
  })

  it('every other pop-out declares a natural its window can actually show', () => {
    const bad: string[] = []
    for (const slug of openableSlugs()) {
      if (slug === 'operate') continue
      const nat: Natural = naturalFor(slug)
      const [dw, dh] = sizeFor(DEFAULT_INNER, slug)
      const [mw, mh] = sizeFor(MIN_INNER, slug)
      if (nat.w > dw || nat.h > dh) bad.push(`${slug}: natural ${nat.w}×${nat.h} > default inner ${dw}×${dh}`)
      if (nat.w > mw / FLOOR || nat.h > mh / FLOOR) {
        bad.push(
          `${slug}: natural ${nat.w}×${nat.h} > ${Math.round(mw / FLOOR)}×${Math.round(mh / FLOOR)} ` +
            `(min inner ${mw}×${mh} at the ${FLOOR * 100}% floor) — it would clip at the window minimum`,
        )
      }
    }
    expect(bad).toEqual([])
  })

  it('every other pop-out opens at 100% — the behaviour the field report asked for', () => {
    const opened = Object.fromEntries(
      openableSlugs()
        .filter((s) => s !== 'operate')
        .map((slug) => {
          const [dw, dh] = sizeFor(DEFAULT_INNER, slug)
          return [slug, fitScale(dw, dh, 100, undefined, naturalFor(slug))]
        }),
    )
    expect(opened).toEqual({
      bandmapCw: 100,
      bandmapPhone: 100,
      connect: 100,
      dxped: 100,
      fieldday: 100,
      memories: 100,
      needed: 100,
      pota: 100,
      sats: 100,
      waterfall: 100,
    })
  })
})

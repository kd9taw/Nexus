import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// THE MAP TOOLBAR NEVER TRAPS THE OPERATOR, AT ANY MAP WIDTH FROM 280 PX.
//
// The 2-D map's toolbar was one no-wrap flex row: three segmented groups, the grid, the provenance
// badge, Reset and Full screen, ~560 px of min-content. In Connect's centre cell that min-content
// became the map's minimum width (`.connect-map > .map-view` had no `min-width: 0`), so at the
// 1024×768 floor (a ~460 px cell) the whole map ran past the cell's right edge and `.connect-map
// { overflow: hidden }` clipped it: the badge half gone, Reset and Full screen unreachable. With the
// rails draggable wider the cell can reach 280 px, where most of the toolbar was gone.
//
// The fix is three computed properties, and these guards resolve each against the parsed sheet
// (last-wins among the rules that name the element, @media bodies included) rather than grepping
// for a string — a dead selector passes a regex, it does not pass this:
//   1. the toolbar WRAPS — a control strip grows a row rather than pushing its box wider;
//   2. the map in Connect's cell may SHRINK below its content (`min-width: 0`), so the cell — not
//      the toolbar — decides the map's width;
//   3. the Layers panel stacks ABOVE the Conditions rail, so where the two overlays meet on a narrow
//      map the Layers fold chevron and rows stay on top (the Conditions chevron is on its far edge).
// The rendered half — every control's box inside the cell and hit-testable at 280/460/1200 px, 2-D
// and 3-D — is measured in headless Chrome; jsdom lays nothing out.

const STYLES = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '',
)

interface Rule {
  selectors: string[]
  body: string
  order: number
}

/** Brace-aware rule walk that descends into @media/@supports bodies. */
function parse(sheet: string, out: Rule[] = [], counter = { n: 0 }): Rule[] {
  let i = 0
  let selStart = 0
  while (i < sheet.length) {
    if (sheet[i] !== '{') {
      i++
      continue
    }
    const sel = sheet.slice(selStart, i).trim()
    const bodyStart = ++i
    let depth = 1
    while (i < sheet.length && depth > 0) {
      if (sheet[i] === '{') depth++
      else if (sheet[i] === '}') depth--
      i++
    }
    const body = sheet.slice(bodyStart, i - 1)
    if (sel.startsWith('@')) {
      if (/^@(media|supports)\b/.test(sel)) parse(body, out, counter)
    } else {
      out.push({ selectors: sel.split(',').map((s) => s.trim().replace(/\s+/g, ' ')), body, order: ++counter.n })
    }
    selStart = i
  }
  return out
}

const RULES = parse(STYLES)

/** The value `prop` computes for an element matched by exactly `selector` (one of the rule's
 *  comma-separated selectors), last declaration in sheet order winning. The selectors passed below
 *  are the only ones in the sheet that set these properties on these elements — asserted, so a new
 *  competing selector fails loudly instead of being ignored. */
function winning(selector: string, prop: string): string | null {
  let v: string | null = null
  for (const r of RULES) {
    if (!r.selectors.includes(selector)) continue
    for (const decl of r.body.split(';')) {
      const m = new RegExp(`^\\s*${prop}\\s*:\\s*([^;]+?)\\s*$`).exec(decl)
      if (m) v = m[1]
    }
  }
  return v
}

/** Every selector in the sheet that names `cls` as its SUBJECT and sets `prop`. */
function settersOf(cls: string, prop: string): string[] {
  const out: string[] = []
  for (const r of RULES) {
    if (!new RegExp(`(^|;)\\s*${prop}\\s*:`).test(r.body)) continue
    for (const s of r.selectors) if (new RegExp(`\\.${cls}(?![\\w-])[^\\s>+~]*$`).test(s)) out.push(s)
  }
  return out
}

describe('the map toolbar and its overlays at narrow map widths', () => {
  it('the toolbar wraps instead of widening the map', () => {
    expect(settersOf('map-toolbar', 'flex-wrap')).toEqual(['.map-toolbar'])
    expect(winning('.map-toolbar', 'flex-wrap')).toBe('wrap')
  })

  it("the map in Connect's centre cell can shrink below its toolbar's content", () => {
    expect(winning('.connect-map > .map-view', 'min-width')).toBe('0')
    // CONTROL: no later rule on the map's other selectors puts a floor back.
    const floors = settersOf('map-view', 'min-width').filter((s) => s !== '.connect-map > .map-view')
    expect(floors).toEqual([])
  })

  it('the Layers panel stacks above the Conditions rail on both surfaces', () => {
    const conditions = Number(winning('.map-insights', 'z-index'))
    for (const layers of ['.map-layers', '.globe3d-layers']) {
      const z = Number(winning(layers, 'z-index'))
      expect(Number.isFinite(z) && Number.isFinite(conditions), layers).toBe(true)
      expect(z, `${layers} over .map-insights`).toBeGreaterThan(conditions)
    }
  })
})

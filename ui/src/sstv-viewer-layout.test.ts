// THE PICTURE VIEWER'S LAYOUT CONTRACT, computed out of the sheet.
//
// The viewer is one window with two children: the picture, which takes everything going,
// and a details strip, which takes exactly what it needs. That is the pane-grid role
// question asked of a pop-out, and getting it backwards is the app's most-repeated layout
// bug — a grower pointed at content that cannot stretch leaves the empty black box, which
// shipped twice, once per abstraction level.
//
// ⚠️ NOT A PRESENCE TEST. `styles.css` is 19k lines and a later or more specific rule wins
// silently; matching a declaration with a regex is how two dead fixes shipped before the
// 2026-07 overhaul. Everything below collects EVERY rule in the sheet that can match the
// element and computes which declaration actually wins by (specificity, source order) —
// so an override added anywhere in the file, before or after, is what this reads.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const STYLES = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  // Prose must never read as a declaration.
  .replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string
  body: string
  order: number
  /** Depth of enclosing at-rules — a declaration inside `@media` is conditional, and this
   *  test asks what the element computes UNCONDITIONALLY. */
  nested: boolean
}

/** Brace-aware walk of the whole sheet, tracking at-rule nesting. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  let selStart = 0
  let depth = 0
  while (i < sheet.length) {
    const ch = sheet[i]
    if (ch === '{') {
      const sel = sheet.slice(selStart, i).trim()
      i++
      if (sel.startsWith('@')) {
        // An at-rule block: descend, so its inner rules are seen as nested.
        depth++
        selStart = i
        continue
      }
      const bodyStart = i
      let d = 1
      while (i < sheet.length && d > 0) {
        if (sheet[i] === '{') d++
        else if (sheet[i] === '}') d--
        i++
      }
      out.push({ selector: sel, body: sheet.slice(bodyStart, i - 1), order: order++, nested: depth > 0 })
      selStart = i
    } else if (ch === '}') {
      if (depth > 0) depth--
      i++
      selStart = i
    } else {
      i++
    }
  }
  return out
}

const RULES = parseRules(STYLES)

/** Class/attribute count — the specificity that matters in this sheet (no ids are used). */
const specificity = (sel: string) => (sel.match(/\.[a-z][a-zA-Z0-9-]*|\[[^\]]+\]|:[a-z-]+/g) ?? []).length

/** Does `selector` (one comma-separated branch) target an element carrying `cls` as its
 *  own last simple selector? Deliberately generous on the left — an ancestor chain still
 *  counts, because it still applies to the element. */
const targets = (branch: string, cls: string) => {
  const last = branch.trim().split(/\s+|>|\+|~/).filter(Boolean).pop() ?? ''
  return last.split(':')[0].split('.').filter(Boolean).includes(cls.replace('.', ''))
}

/** The winning value of `prop` on an element whose only class is `cls`, computed across
 *  every unconditional rule in the sheet by (specificity, source order). */
function winner(cls: string, prop: string): { value: string; selector: string } | null {
  let best: { value: string; selector: string; spec: number; order: number } | null = null
  for (const rule of RULES) {
    if (rule.nested) continue
    for (const branch of rule.selector.split(',')) {
      if (!targets(branch, cls)) continue
      // An ancestor requirement means the rule needs more than this one class; skip those
      // that could not match a bare element, but KEEP them if the ancestor is the viewer
      // shell, which every one of these elements really does sit inside.
      const parts = branch.trim().split(/\s+/).filter(Boolean)
      if (parts.length > 1 && !parts.slice(0, -1).every((p) => /sstv-viewer|detached|app/.test(p))) continue
      const spec = specificity(branch)
      for (const decl of rule.body.split(';')) {
        const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
        if (!m) continue
        if (!best || spec > best.spec || (spec === best.spec && rule.order >= best.order)) {
          best = { value: m[1], selector: branch.trim(), spec, order: rule.order }
        }
      }
    }
  }
  return best ? { value: best.value, selector: best.selector } : null
}

describe('the SSTV picture viewer pop-out', () => {
  it('⭐ the picture is the grower and it can actually stretch', () => {
    // `flex: 1` on the stage, and — the half that is always forgotten — `min-height: 0`,
    // without which a flex item refuses to shrink below its content and pushes the details
    // strip off the bottom of the window instead of letting the picture scale down.
    const flex = winner('sstv-viewer-stage', 'flex')
    expect(flex, 'nothing in the sheet gives .sstv-viewer-stage a flex').not.toBeNull()
    expect(flex!.value, `won by ${flex!.selector}`).toMatch(/^1\b/)
    const min = winner('sstv-viewer-stage', 'min-height')
    expect(min?.value, `min-height won by ${min?.selector}`).toBe('0')
  })

  it('⭐ the picture it points at yields on BOTH axes, so the grower is never a black box', () => {
    // The stage clips (overflow: hidden). A fixed-size child under a clipping parent with
    // no interposed scroller is the contract's named failure; the image yields instead.
    expect(winner('sstv-viewer-stage', 'overflow')?.value).toBe('hidden')
    const rule = RULES.find((r) => r.selector.includes('.sstv-viewer-stage img'))
    expect(rule, 'the sheet must size the picture inside the stage').toBeTruthy()
    expect(rule!.body).toMatch(/max-width:\s*100%/)
    expect(rule!.body).toMatch(/max-height:\s*100%/)
  })

  it('⭐ the details strip takes exactly its content, and wraps rather than overflowing', () => {
    // `flex: 0 0 auto` is the pop-out spelling of the pane contract's fit="content": a
    // strip cannot use surplus, because every pixel it took would come off the picture.
    const flex = winner('sstv-viewer-bar', 'flex')
    expect(flex?.value, `won by ${flex?.selector}`).toBe('0 0 auto')
    // Dragged narrow, the buttons go onto a second line instead of off the edge — the
    // window's minimum is 420 px wide and five buttons do not fit across it.
    expect(winner('sstv-viewer-bar', 'flex-wrap')?.value).toBe('wrap')
  })

  it('⭐ the window height is zoom-corrected, never raw vh', () => {
    // Zoom lives on `.app`, which this branch carries, so a raw 100vh overshoots the
    // window by the zoom factor and clips the details strip away. The ONE branch that got
    // this wrong shipped a window that ignored the operator's UI scale.
    const h = winner('detached-sstvviewer', 'height')
    expect(h?.value).toMatch(/var\(--vh-eff/)
    expect(h?.value, 'a bare vh unit inside .app is zoom-blind').not.toMatch(/^\s*\d+vh/)
  })

  it('positive control: this file can tell a wrong value from a right one', () => {
    // Without this, every assertion above would also pass against a parser that found
    // nothing and compared `undefined` to `undefined`. Point it at a declaration whose
    // value is known and DIFFERENT from what the tests above accept.
    const known = winner('sstv-viewer-stage', 'align-items')
    expect(known?.value).toBe('center')
    expect(known?.value).not.toBe('0')
    expect(winner('sstv-viewer-stage', 'no-such-property-exists')).toBeNull()
  })
})

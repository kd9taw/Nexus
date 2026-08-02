// Cascade guards for the country-exclusion chrome.
//
// These COMPUTE the winning declaration out of styles.css rather than asserting a selector
// is present — a dead rule passes a presence check, which is how two layout fixes shipped
// broken pre-overhaul. Two claims are load-bearing and would rot silently:
//
//   1. `.od-status` WRAPS. It gained a third child (the hidden-country chip), and that chip
//      is a nowrap atom. `.cockpit-side` / `.cockpit-panes` clip overflow-x rather than
//      scroll it, so as a nowrap row the chip is painted outside the clip — unreachable —
//      once the rail is near its 260px minimum. Exactly the trap `.od-filters` documents.
//   2. The ticked checkmark actually draws: `.country-item[data-state='checked']::before`
//      must outrank the base `.country-item::before`, or every country renders unticked
//      while `aria-checked` says otherwise.
//
// Green-by-construction on the day they landed (the rules are new), so they were run RED
// against a deliberately-broken sheet first — the grading cockpit-panes.test.ts uses. Undoing
// the wrap and restoring `justify-content: space-between` failed (1) and its companion;
// demoting the checked tick to equal specificity ABOVE the base rule — a dead rule a presence
// check passes happily — failed (2). Their job starts the next time someone edits this sheet.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const CSS = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '', // prose must never read as a declaration
)

interface Rule {
  selector: string
  body: string
  order: number
  media: string | null
}

/** Brace-aware walk that records each rule's @media condition instead of skipping it — a
 *  conditional override is a real cascade context, not something to be silently blind to. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  let selStart = 0
  let media: string | null = null
  let mediaEnd = -1
  while (i < sheet.length) {
    if (i === mediaEnd) {
      media = null
      mediaEnd = -1
    }
    const ch = sheet[i]
    if (ch === '{') {
      const sel = sheet.slice(selStart, i).trim()
      i++
      if (sel.startsWith('@')) {
        // Step INTO the block so its rules are collected, tagged with the condition.
        if (/^@(media|supports|container)/.test(sel)) {
          media = sel
          let d = 1
          let j = i
          while (j < sheet.length && d > 0) {
            if (sheet[j] === '{') d++
            else if (sheet[j] === '}') d--
            j++
          }
          mediaEnd = j - 1
        } else {
          // @keyframes/@font-face — skip the whole block, it declares nothing here.
          let d = 1
          while (i < sheet.length && d > 0) {
            if (sheet[i] === '{') d++
            else if (sheet[i] === '}') d--
            i++
          }
        }
        selStart = i
        continue
      }
      let depth = 1
      const bodyStart = i
      while (i < sheet.length && depth > 0) {
        if (sheet[i] === '{') depth++
        else if (sheet[i] === '}') depth--
        i++
      }
      for (const one of sel.split(',')) {
        out.push({ selector: one.trim(), body: sheet.slice(bodyStart, i - 1), order: order++, media })
      }
      selStart = i
      continue
    }
    if (ch === '}') selStart = i + 1
    i++
  }
  return out
}

const RULES = parseRules(CSS)

/** Last declaration of `prop` in a block (in-block order decides). */
function declValue(body: string, prop: string): string | null {
  let v: string | null = null
  const re = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, 'g')
  for (const m of body.matchAll(re)) v = m[1].trim()
  return v
}

/** Class + attribute + pseudo-class count. Pseudo-ELEMENTS (::before) add no class-level
 *  weight, and every selector compared here shares the same one, so they are excluded. */
function specificity(sel: string): number {
  const bare = sel.replace(/::[a-z-]+/g, '')
  return (
    (bare.match(/\./g) ?? []).length +
    (bare.match(/\[/g) ?? []).length +
    (bare.match(/(?<!:):(?!:)[a-z-]+/g) ?? []).length
  )
}

/** The rule that WINS `prop` among every rule whose selector text matches `selectorText`
 *  exactly — resolved by specificity, then source order, and reported with its origin so a
 *  failure names the rule that stole it. */
function winner(match: (sel: string) => boolean, prop: string) {
  let win: { value: string; selector: string; spec: number; order: number; media: string | null } | null =
    null
  for (const r of RULES) {
    if (!match(r.selector)) continue
    const v = declValue(r.body, prop)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order, media: r.media }
    }
  }
  return win
}

describe('the decode status row wraps, so the hidden-country chip is never clipped away', () => {
  // Any rule whose subject is `.od-status` — including a compound or a descendant form a
  // later edit might add, since those would outrank the base rule.
  const hitsOdStatus = (sel: string) => /(^|[\s>+~])\.od-status(\.|:|\[|$)/.test(`${sel}`)

  it('computes flex-wrap: wrap', () => {
    const w = winner(hitsOdStatus, 'flex-wrap')
    expect(w, '.od-status declares no flex-wrap — a third status chip will be clipped').not.toBeNull()
    expect(w!.value, `lost to \`${w?.selector}\`${w?.media ? ` in ${w.media}` : ''}`).toBe('wrap')
  })

  it('declares no justify-content that would fling the wrapped chips to both edges', () => {
    // The auto margin on `.od-paused` groups them right on one line and packs them left
    // once the row wraps; space-between would undo the second half of that.
    const w = winner(hitsOdStatus, 'justify-content')
    expect(w?.value ?? null).toBeNull()
  })

  it('still gives the heard count the slack (chips group right on a single line)', () => {
    const w = winner((sel) => /(^|[\s>+~])\.od-paused(\.|:|\[|$)/.test(sel), 'margin-right')
    expect(w?.value).toBe('auto')
  })
})

describe('a ticked country actually draws its checkmark', () => {
  const isBefore = (sel: string) => sel.includes('.country-item') && sel.includes('::before')

  it('the checked rule outranks the base rule for `content`', () => {
    const w = winner(isBefore, 'content')
    expect(w, 'no .country-item::before content rule at all').not.toBeNull()
    expect(w!.selector).toContain("[data-state='checked']")
    expect(w!.value).toBe("'✓'")
    // …and it must genuinely outrank, not merely come later — a reorder would then be safe.
    const base = RULES.find((r) => r.selector === '.country-item::before')
    expect(base, 'the unchecked fixed-width tick column is gone').toBeTruthy()
    expect(specificity(w!.selector)).toBeGreaterThan(specificity(base!.selector))
  })
})

describe('the portaled picker sizes itself, not .ui-menu', () => {
  it('.country-menu wins min-width over the shared .ui-menu rule', () => {
    const w = winner((sel) => sel === '.ui-menu' || sel === '.country-menu', 'min-width')
    expect(w!.selector).toBe('.country-menu')
    // Equal specificity — so this holds only while .country-menu comes LATER in the sheet.
    expect(w!.value).toBe('15em')
  })

  it('bounds its own height so 18 rows cannot outgrow a short screen', () => {
    const w = winner((sel) => sel === '.country-menu', 'max-height')
    expect(w!.value).toContain('--radix-dropdown-menu-content-available-height')
    expect(winner((sel) => sel === '.country-menu', 'overflow-y')!.value).toBe('auto')
  })
})

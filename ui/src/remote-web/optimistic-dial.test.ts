// THE GATE ON THE OPTIMISTIC DIAL. `FrequencyReadout` can draw a dial the radio has not reached
// (`provisionalMhz`), the digits a remote page showed while a tuning command was on its way. Nothing
// feeds it now that the hosted page's wheel tuning is gone; this file keeps it a display value if
// anything ever does again. Only the readout may name a provisional dial, and nothing anywhere may
// feed one into a `dialMhz`, which every privilege, band, mode and transmit computation reads. The
// control below must trip the scanner, because a clean result from a check nobody has seen fail is
// not a result.
import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'

// ── CONTAINMENT ───────────────────────────────────────────────────────────────────────────────
/** The provisional dial, by every name it goes by. */
const NAMES = /provisional/i
/** A `dialMhz` — a prop, a field or an assignment — fed from a provisional value. This is the
 *  wiring mistake the whole gate exists for: the moment the optimistic number becomes A DIAL, it
 *  is an input to everything that reads one. */
const AS_A_DIAL = /\bdialMhz\s*[:=]\s*\{?[^,;)\n]*provisional/i

/** The only file allowed to name it: the readout that draws it. A file appearing here that is not
 *  in this list is a widening of the safety case. */
const ALLOWED = [
  'components/FrequencyReadout.tsx',
]

type Source = { path: string; text: string }
/** The scanner, over (path, text) pairs so the control below can run the same code on a violation
 *  it makes up rather than on a second implementation of it. */
function violations(sources: Source[]): string[] {
  const found: string[] = []
  for (const { path, text } of sources) {
    if (NAMES.test(text) && !ALLOWED.includes(path)) found.push(`${path}: names the provisional dial`)
    const line = text.split('\n').findIndex(l => AS_A_DIAL.test(l))
    if (line >= 0) found.push(`${path}:${line + 1}: feeds a provisional value into a dialMhz`)
  }
  return found
}

function sources(): Source[] {
  const root = resolve(process.cwd(), 'src')
  const out: Source[] = []
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const p = join(dir, entry)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p)) out.push({ path: relative(root, p).split(sep).join('/'), text: readFileSync(p, 'utf8') })
    }
  }
  walk(root)
  return out
}

describe('nothing reads the provisional dial but the digits', () => {
  it('only the files that own and draw it name it at all', () => {
    const all = sources()
    expect(all.length, 'the tree was actually walked').toBeGreaterThan(200)
    expect(violations(all)).toEqual([])
    // ...and every file on the list really does still name it, so the list cannot rot into an
    // allowance for files that no longer exist.
    const naming = all.filter(s => NAMES.test(s.text)).map(s => s.path).sort()
    expect(naming).toEqual([...ALLOWED].sort())
  })

  it('control: a reader outside the list, and a provisional wired into a dial, both trip it', () => {
    expect(violations([{ path: 'stationAccess.ts', text: 'const dial = tuning.getProvisionalHz()\n' }]))
      .toEqual(['stationAccess.ts: names the provisional dial'])
    // The allowed files are not exempt from the second rule: this is the mistake that matters.
    expect(violations([{ path: 'components/FrequencyReadout.tsx', text: 'a\nb\n<X dialMhz={remoteTuning.provisionalMhz} />\n' }]))
      .toEqual(['components/FrequencyReadout.tsx:3: feeds a provisional value into a dialMhz'])
  })
})

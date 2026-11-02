// A DISABLED SWITCH LOOKS DISABLED. Settings ▸ Rig & CAT shows the Flex native panadapter disabled
// off the network connection, with a note saying why. Before this sheet had a disabled rule, a
// disabled `.toggle` painted exactly like a live one, so "greyed out" was true only of the note.
//
// Resolved the way the other style guards do (cssCascade): for a live and a disabled switch, on
// and off, in every theme mode, which rule WINS each property, not whether a declaration exists.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { MODES, parseRules, tokensAt, winnerAt, type El, type Mode } from './cssCascade'

const RAW = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(CSS)

/** A settings switch as SettingsPanel renders it, inside its `label.settings-field`. */
const chain = (on: boolean, disabled: boolean): El[] => [
  { tag: 'div', classes: ['settings-panel'], attrs: {} },
  { tag: 'label', classes: ['settings-field'], attrs: {} },
  {
    tag: 'button',
    classes: on ? ['toggle', 'on'] : ['toggle'],
    attrs: { type: 'button', role: 'switch', 'aria-checked': String(on), ...(disabled ? { disabled: '' } : {}) },
  },
]

const opacity = (c: El[], mode: Mode) => {
  const w = winnerAt(RULES, mode, c, 'opacity')
  return w ? parseFloat(w.value) : 1
}

describe('a disabled settings switch is visibly disabled', () => {
  it.each(MODES)('dimmed, and not-allowed, on or off, in %s', (mode) => {
    for (const on of [false, true]) {
      expect(opacity(chain(on, true), mode), `on=${on}`).toBeLessThanOrEqual(0.6)
      expect(winnerAt(RULES, mode, chain(on, true), 'cursor')?.value, `on=${on}`).toBe('not-allowed')
    }
  })

  it.each(MODES)('a live switch keeps its full look in %s', (mode) => {
    for (const on of [false, true]) {
      expect(opacity(chain(on, false), mode), `on=${on}`).toBe(1)
      expect(winnerAt(RULES, mode, chain(on, false), 'cursor')?.value, `on=${on}`).not.toBe('not-allowed')
    }
    // ON stays readable as on: the live ON rim is the accent.
    expect(winnerAt(RULES, mode, chain(true, false), 'border-color')?.value).toBe('var(--accent)')
  })

  it.each(MODES)('a disabled ON switch drops the accent rim, and every token it reads exists, in %s', (mode) => {
    const rim = winnerAt(RULES, mode, chain(true, true), 'border-color')
    expect(rim?.value).toBe('var(--border)')
    const tokens = tokensAt(RULES, mode, chain(true, true))
    for (const [, name] of rim!.value.matchAll(/var\((--[\w-]+)/g)) {
      expect(tokens.has(name), `${name} is undefined in ${mode}`).toBe(true)
    }
  })
})

// The one-tap looks (features/looks.ts) — the table the operator reviews, held to its promises.
//
// Every promise is checked over EVERY screen the five settings can make (2 × 3 × 2 × 3 × 4 = 144),
// not over a hand-picked few, so a look that only works from the defaults cannot pass:
//   · a look writes exactly its own settings, and reaches no setter outside them;
//   · applied from anywhere, the row reads it back (it round-trips);
//   · no screen reads as two looks, so "which look is on screen" always has one answer;
//   · changing a setting a look owns reads Custom (or the one look that now matches), and a setting
//     it does not own never changes the answer;
//   · Night on Auto is a schedule, not a look: only the Night look moves it.
import { describe, expect, it } from 'vitest'
import { EN } from '../i18n'
import { DENSITY_STEPS, type Density } from '../useDensity'
import type { NightChoice } from '../useNight'
import type { TextSize } from '../useTextSize'
import { LOOKS, applyLook, lookOf, type Look, type LookAxes, type LookSetters } from './looks'

const NIGHTS: NightChoice[] = ['off', 'on', 'auto']
const TEXT: TextSize[] = ['normal', 'large', 'larger']
const BOOL = [false, true]

/** Every screen the five settings can make. */
const ALL: LookAxes[] = BOOL.flatMap((fieldMode) =>
  NIGHTS.flatMap((night) =>
    BOOL.flatMap((highContrast) =>
      TEXT.flatMap((textSize) =>
        DENSITY_STEPS.map((density: Density) => ({ fieldMode, night, highContrast, textSize, density })),
      ),
    ),
  ),
)

/** A fresh install: what the hooks read with nothing stored. */
const DEFAULTS: LookAxes = { fieldMode: false, night: 'off', highContrast: false, textSize: 'normal', density: 'standard' }

const AXES = ['fieldMode', 'night', 'highContrast', 'textSize', 'density'] as const
const SETTER_OF = {
  fieldMode: 'setFieldMode',
  night: 'setNight',
  highContrast: 'setHighContrast',
  textSize: 'setTextSize',
  density: 'setDensity',
} as const

/** Setters that write into a copy of `from`, and a log of every call. */
function recorder(from: LookAxes) {
  const state: LookAxes = { ...from }
  const calls: [string, unknown][] = []
  const set: LookSetters = {
    setFieldMode: (v) => {
      calls.push(['setFieldMode', v])
      state.fieldMode = v
    },
    setNight: (v) => {
      calls.push(['setNight', v])
      state.night = v
    },
    setHighContrast: (v) => {
      calls.push(['setHighContrast', v])
      state.highContrast = v
    },
    setTextSize: (v) => {
      calls.push(['setTextSize', v])
      state.textSize = v
    },
    setDensity: (v) => {
      calls.push(['setDensity', v])
      state.density = v
    },
  }
  return { state, calls, set }
}

/** The table, and proof it is the table: a loop over an empty LOOKS would pass every test below. */
const looks = (): readonly Look[] => {
  expect(LOOKS.length, 'the looks table is empty — every loop below would pass over nothing').toBe(6)
  return LOOKS
}

const byId = (id: string): Look => {
  const l = LOOKS.find((x) => x.id === id)
  if (!l) throw new Error(`no look '${id}'`)
  return l
}
const owned = (l: Look) => AXES.filter((a) => l.sets[a] !== undefined)
const show = (a: LookAxes) => JSON.stringify(a)

describe('the six looks', () => {
  it('are the spec’s six, in its order, each named in the catalog', () => {
    expect(LOOKS.map((l) => l.id)).toEqual(['shack', 'field', 'night', 'contest', 'bigClear', 'touch'])
    for (const l of LOOKS) expect(EN[l.labelKey], l.id).toBeTruthy()
  })

  it('read the fresh-install screen as Shack, the everyday look', () => {
    expect(lookOf(DEFAULTS)).toBe('shack')
  })

  it('pin the spec’s own words: Field mode, Night On, Compact, Larger + High contrast + Comfortable, Touch', () => {
    expect(byId('field').sets.fieldMode).toBe(true)
    expect(byId('night').sets.night).toBe('on')
    expect(byId('contest').sets.density).toBe('dense')
    expect(byId('bigClear').sets).toMatchObject({ textSize: 'larger', highContrast: true, density: 'guided' })
    expect(byId('touch').sets.density).toBe('touch')
  })
})

describe('a look writes exactly its own settings', () => {
  it('from every screen: only setters for settings it owns, each with the look’s value', () => {
    for (const look of looks()) {
      for (const from of ALL) {
        const r = recorder(from)
        applyLook(look, from, r.set)
        const mine = new Set(owned(look).map((a) => SETTER_OF[a]))
        for (const [name] of r.calls) expect(mine.has(name as never), `${look.id} called ${name} from ${show(from)}`).toBe(true)
        for (const a of owned(look)) {
          if (a === 'night') continue // the Night rule has its own test below
          expect(r.state[a], `${look.id} left ${a} at ${String(r.state[a])} from ${show(from)}`).toBe(look.sets[a])
        }
      }
    }
  })

  it('reaches nothing but the five setters it is handed — no theme, scale, colour, motion or palette', () => {
    for (const look of looks()) {
      const touched = new Set<string>()
      const r = recorder(DEFAULTS)
      const spy = new Proxy(r.set, {
        get(target, key: string) {
          touched.add(key)
          return target[key as keyof LookSetters]
        },
      })
      applyLook(look, { ...DEFAULTS, fieldMode: true, night: 'on', highContrast: true, textSize: 'larger', density: 'touch' }, spy)
      for (const k of touched) expect(Object.values(SETTER_OF), `${look.id} reached '${k}'`).toContain(k)
    }
  })

  it('never writes a setting that already has the look’s value', () => {
    for (const look of looks()) {
      const r = recorder(DEFAULTS)
      applyLook(look, DEFAULTS, r.set)
      const again = recorder(r.state)
      applyLook(look, r.state, again.set)
      expect(again.calls, `${look.id} re-applied over itself`).toEqual([])
    }
  })
})

describe('the row reads the screen back', () => {
  it('round-trips: applied from every screen, a look is the look on screen', () => {
    for (const look of looks()) {
      for (const from of ALL) {
        const r = recorder(from)
        applyLook(look, from, r.set)
        expect(lookOf(r.state), `${look.id} from ${show(from)}`).toBe(look.id)
      }
    }
  })

  it('no screen reads as two looks', () => {
    for (const a of ALL) {
      const matching = looks().filter((l) => {
        const r = recorder(a)
        applyLook(l, a, r.set)
        return r.calls.length === 0 // already on this look: applying it changes nothing
      }).map((l) => l.id)
      expect(matching.length, `${show(a)} reads as ${matching.join(' + ')}`).toBeLessThanOrEqual(1)
      if (matching.length === 1) expect(lookOf(a)).toBe(matching[0])
      else expect(lookOf(a)).toBe('custom')
    }
  })

  it('a change to a setting a look owns reads Custom, or the one look that now matches', () => {
    const values: Record<(typeof AXES)[number], unknown[]> = {
      fieldMode: BOOL,
      night: NIGHTS,
      highContrast: BOOL,
      textSize: TEXT,
      density: DENSITY_STEPS,
    }
    for (const look of looks()) {
      const r = recorder(DEFAULTS)
      applyLook(look, DEFAULTS, r.set)
      for (const a of owned(look)) {
        for (const v of values[a]) {
          if (v === r.state[a]) continue // not a change
          const changed = { ...r.state, [a]: v } as LookAxes
          if (lookOf(changed) === look.id) {
            // Only a value the look itself allows may keep its name: Auto under a look that
            // switches a manual Night off.
            expect(`${a}=${String(v)}`, `${look.id} still named after ${a} → ${String(v)}`).toBe('night=auto')
            continue
          }
          const now = lookOf(changed)
          if (now !== 'custom') {
            const again = recorder(changed)
            applyLook(byId(now), changed, again.set)
            expect(again.calls, `${show(changed)} named ${now} but is not it`).toEqual([])
          }
        }
      }
    }
  })

  it('a setting a look does not own never changes which look the row names', () => {
    for (const look of looks()) {
      const r = recorder(DEFAULTS)
      applyLook(look, DEFAULTS, r.set)
      const free = AXES.filter((a) => look.sets[a] === undefined)
      for (const a of free) {
        for (const v of { fieldMode: BOOL, night: NIGHTS, highContrast: BOOL, textSize: TEXT, density: DENSITY_STEPS }[a]) {
          expect(lookOf({ ...r.state, [a]: v } as LookAxes), `${look.id} with ${a}=${String(v)}`).toBe(look.id)
        }
      }
    }
  })
})

describe('Night on Auto is a schedule, not a look', () => {
  it('survives every look but Night, and only Night switches Night on', () => {
    for (const look of looks()) {
      const r = recorder({ ...DEFAULTS, night: 'auto' })
      applyLook(look, r.state, r.set)
      expect(r.state.night, look.id).toBe(look.id === 'night' ? 'on' : 'auto')
    }
  })

  it('a Night switched On by hand goes off with a look that turns Night off', () => {
    const offs = looks().filter((l) => l.sets.night === 'off')
    expect(offs.length, 'no look turns a manual Night off').toBeGreaterThan(0)
    for (const look of offs) {
      const r = recorder({ ...DEFAULTS, night: 'on' })
      applyLook(look, r.state, r.set)
      expect(r.state.night, look.id).toBe('off')
    }
  })
})

describe('Field / POTA keeps the standing High contrast choice', () => {
  // useFieldMode.ts: field mode and high contrast never write each other's key, so leaving the
  // field restores the operator's own contrast. The look that turns field mode on keeps that.
  it('leaves High contrast exactly as it was, either way', () => {
    for (const hc of BOOL) {
      const r = recorder({ ...DEFAULTS, highContrast: hc })
      applyLook(byId('field'), r.state, r.set)
      expect(r.state.highContrast).toBe(hc)
      expect(r.calls.some(([n]) => n === 'setHighContrast')).toBe(false)
    }
  })
})

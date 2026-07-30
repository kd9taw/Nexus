// @vitest-environment jsdom
//
// The Operate cockpit's persisted view filters: do they round-trip, do they survive a
// corrupt stored value, and does a pop-out keep its own set? The CLASSIFICATION of the two
// keys (per-surface, not shared) is pinned in storage-scope.test.ts; this file is the
// behaviour.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import {
  DECODE_FILTER_KEY,
  DEFAULT_ROSTER_FILTERS,
  ROSTER_FILTER_KEY,
  loadDecodeFilter,
  loadRosterFilters,
  saveDecodeFilter,
  saveRosterFilters,
} from './operateFilters'
import { DECODE_FILTERS } from './decodeHistory'

/** Run `fn` as if this webview were a torn-off Operate window. */
function asPopOut<T>(fn: () => T): T {
  window.history.replaceState({}, '', '/?panel=operate')
  try {
    return fn()
  } finally {
    window.history.replaceState({}, '', '/')
  }
}

beforeEach(() => {
  localStorage.clear()
})
afterEach(() => {
  vi.restoreAllMocks()
  window.history.replaceState({}, '', '/')
})

describe('roster filters: default = current behaviour', () => {
  it('reads both checkboxes off when nothing has ever been stored', () => {
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: false })
    expect(DEFAULT_ROSTER_FILTERS).toEqual({ neededOnly: false, hideWorked: false })
  })

  it('does not mutate the exported default when a caller edits what it got back', () => {
    const f = loadRosterFilters()
    f.neededOnly = true
    expect(DEFAULT_ROSTER_FILTERS.neededOnly).toBe(false)
  })
})

describe('roster filters: round trip', () => {
  it.each([
    { neededOnly: true, hideWorked: false },
    { neededOnly: false, hideWorked: true },
    { neededOnly: true, hideWorked: true },
    { neededOnly: false, hideWorked: false },
  ])('saves and reloads %o unchanged', (f) => {
    saveRosterFilters(f)
    expect(loadRosterFilters()).toEqual(f)
  })

  it('writes the BARE key from the main window — nothing on disk to migrate', () => {
    saveRosterFilters({ neededOnly: true, hideWorked: false })
    expect(localStorage.getItem('nexus.roster.filters')).toBe('{"neededOnly":true,"hideWorked":false}')
  })
})

describe('roster filters: a corrupt stored value never hides rows silently', () => {
  // The failure that matters: a bad value leaves the roster FILTERING while the checkbox
  // renders unticked, so rows are missing with nothing on screen to explain why. Every
  // one of these must therefore fall back to "show everything".
  it.each([
    ['not json at all', 'wat{{'],
    ['a truncated write', '{"neededOnly":tr'],
    ['an empty string', ''],
    ['a stored null', 'null'],
    ['a bare number', '7'],
    ['a bare string', '"neededOnly"'],
    ['an array', '[true,true]'],
    ['string "true" instead of a boolean', '{"neededOnly":"true","hideWorked":"true"}'],
    ['numeric 1 instead of a boolean', '{"neededOnly":1,"hideWorked":1}'],
    ['null fields', '{"neededOnly":null,"hideWorked":null}'],
    ['an empty object', '{}'],
    ['keys from some other build', '{"onlyNeeded":true,"showWorked":false}'],
  ])('falls back to both-off for %s', (_label, raw) => {
    localStorage.setItem(ROSTER_FILTER_KEY, raw)
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: false })
  })

  it('keeps the GOOD field when only one of the two is corrupt', () => {
    // Per-field narrowing, not per-object: a half-written value should not throw away the
    // checkbox that did survive.
    localStorage.setItem(ROSTER_FILTER_KEY, '{"neededOnly":true,"hideWorked":"nope"}')
    expect(loadRosterFilters()).toEqual({ neededOnly: true, hideWorked: false })
  })
})

describe('decode filter: round trip and sanitizing', () => {
  it('defaults to All when nothing has ever been stored', () => {
    expect(loadDecodeFilter()).toBe('all')
  })

  it.each([...DECODE_FILTERS])('saves and reloads the %s chip', (f) => {
    saveDecodeFilter(f)
    expect(loadDecodeFilter()).toBe(f)
  })

  it('stores the bare chip name, not JSON', () => {
    saveDecodeFilter('cq')
    expect(localStorage.getItem('nexus.decodes.filter')).toBe('cq')
  })

  it.each(['CQ', 'dxped', '"cq"', '', 'null', 'wat{{', '{"filter":"cq"}'])(
    'falls back to All for the unknown stored value %j',
    (raw) => {
      // A chip renamed or dropped since the value was written must not filter the pane down
      // to nothing with no chip lit — the rule NEED_TYPE_VALUES enforces on the Needed board.
      localStorage.setItem(DECODE_FILTER_KEY, raw)
      expect(loadDecodeFilter()).toBe('all')
    },
  )
})

describe('per-surface: a torn-off Operate window keeps its own filters', () => {
  it('inherits the main window until it writes, then diverges', () => {
    saveRosterFilters({ neededOnly: true, hideWorked: false })
    saveDecodeFilter('cq')

    // Never written its own → inherits what the operator is already using, rather than
    // dropping back to first-run defaults.
    expect(asPopOut(loadRosterFilters)).toEqual({ neededOnly: true, hideWorked: false })
    expect(asPopOut(loadDecodeFilter)).toBe('cq')

    asPopOut(() => {
      saveRosterFilters({ neededOnly: false, hideWorked: true })
      saveDecodeFilter('b4')
    })

    // The pop-out moved; the main window did NOT.
    expect(asPopOut(loadRosterFilters)).toEqual({ neededOnly: false, hideWorked: true })
    expect(asPopOut(loadDecodeFilter)).toBe('b4')
    expect(loadRosterFilters()).toEqual({ neededOnly: true, hideWorked: false })
    expect(loadDecodeFilter()).toBe('cq')
    expect(localStorage.getItem('nexus.roster.filters.operate')).toBe('{"neededOnly":false,"hideWorked":true}')
    expect(localStorage.getItem('nexus.decodes.filter.operate')).toBe('b4')
  })
})

describe('blocked storage is not fatal', () => {
  it('reads defaults when the store throws on read', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('storage blocked')
    })
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: false })
    expect(loadDecodeFilter()).toBe('all')
  })

  it('swallows a throwing write — the filter still applies for this session', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded')
    })
    expect(() => saveRosterFilters({ neededOnly: true, hideWorked: true })).not.toThrow()
    expect(() => saveDecodeFilter('new')).not.toThrow()
  })
})

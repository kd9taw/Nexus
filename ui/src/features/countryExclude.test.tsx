// @vitest-environment jsdom
//
// The country-exclusion list's STORAGE contract and the cross-window convergence that one
// shared key requires. The catalog itself — including the cty.dat pin on every entity name
// — lives in countryExclude.catalog.test.ts, which cannot be a jsdom test.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import {
  COUNTRY_EXCLUDE_KEY,
  loadCountryExclude,
  saveCountryExclude,
  useCountryExclude,
} from './countryExclude'

beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  window.history.replaceState({}, '', '/')
})

describe('persistence: one bare app-global key', () => {
  it('excludes nothing when the operator has never chosen', () => {
    expect([...loadCountryExclude()]).toEqual([])
  })

  it('round-trips a chosen set', () => {
    saveCountryExclude(['dl', 'ea'])
    expect([...loadCountryExclude()].sort()).toEqual(['dl', 'ea'])
  })

  it('stores in catalog order, so the value is stable however it was ticked', () => {
    saveCountryExclude(['ea', 'dl'])
    expect(localStorage.getItem(COUNTRY_EXCLUDE_KEY)).toBe('["dl","ea"]')
  })

  it('writes the BARE key — a pop-out reads the very same one', () => {
    saveCountryExclude(['dl'])
    expect(localStorage.getItem('nexus.decodes.countryExclude')).toBe('["dl"]')
    window.history.replaceState({}, '', '/?panel=operate')
    // Not surfaceGet: a standing statement about how this operator chases is not a
    // property of a window. Per-surface, a torn-off band map would inherit once and then
    // diverge on its first toggle — showing the decodes the main window hides.
    expect([...loadCountryExclude()]).toEqual(['dl'])
  })

  it.each([
    ['not json at all', 'wat{{'],
    ['a truncated write', '["dl"'],
    ['an empty string', ''],
    ['a stored null', 'null'],
    ['a bare number', '7'],
    ['a bare string', '"dl"'],
    ['an object instead of an array', '{"dl":true}'],
    ['non-string members', '[1,2,3]'],
  ])('excludes NOTHING for %s', (_label, raw) => {
    // A hide filter must never hide a row the operator cannot explain from the UI, so every
    // unreadable value falls back to "show everything" rather than to a guess.
    localStorage.setItem(COUNTRY_EXCLUDE_KEY, raw)
    expect([...loadCountryExclude()]).toEqual([])
  })

  it('drops a key from some other build and keeps the ones it understands', () => {
    // Per-member narrowing, the loadRosterFilters rule. Dropping the unknown key is the SAFE
    // direction (fewer rows hidden); honouring it would hide rows with no checkbox to
    // explain them, since this build renders no checkbox for a country it has never heard of.
    localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl","atlantis","ea"]')
    expect([...loadCountryExclude()].sort()).toEqual(['dl', 'ea'])
  })
})

describe('blocked storage is not fatal', () => {
  it('excludes nothing when the store throws, and a throwing write is swallowed', () => {
    const get = Storage.prototype.getItem
    const set = Storage.prototype.setItem
    Storage.prototype.getItem = () => {
      throw new Error('storage blocked')
    }
    Storage.prototype.setItem = () => {
      throw new Error('quota exceeded')
    }
    try {
      expect([...loadCountryExclude()]).toEqual([])
      expect(() => saveCountryExclude(['dl'])).not.toThrow()
    } finally {
      Storage.prototype.getItem = get
      Storage.prototype.setItem = set
    }
  })
})

// ── Cross-window convergence ────────────────────────────────────────────────────────
//
// One shared key and several mounted consumers (Band Activity, the roster, the picker, and
// the same panes again in a torn-off window). A change in one must reach all of them, or
// the two surfaces disagree — the bug class the per-surface scope exists to prevent, which
// this key opts out of and so must solve for itself.

function Probe({ id }: { id: string }) {
  const { keys, toggle } = useCountryExclude()
  return (
    <button type="button" data-testid={id} onClick={() => toggle('dl')}>
      {[...keys].sort().join(',') || 'none'}
    </button>
  )
}

const shows = (id: string) => screen.getByTestId(id).textContent

/** Probe that reports the RESOLVED hidden set + pause state, for the pause test. */
function PauseProbe() {
  const { keys, toggle, hidden, paused, setPaused } = useCountryExclude()
  return (
    <div>
      <button type="button" data-testid="tick" onClick={() => toggle('dl')} />
      <button type="button" data-testid="pause" onClick={() => setPaused(!paused)} />
      <span data-testid="keys">{[...keys].sort().join(',') || 'none'}</span>
      <span data-testid="hidden">{[...hidden].sort().join(',') || 'none'}</span>
    </div>
  )
}

/** Probe reporting the resolved hidden set + the arbitrary-entity toggle. */
function EntityProbe() {
  const { hidden, entities, toggleEntity } = useCountryExclude()
  return (
    <div>
      <button type="button" data-testid="tog" onClick={() => toggleEntity('Fiji')} />
      <span data-testid="ents">{[...entities].sort().join(',') || 'none'}</span>
      <span data-testid="ehidden">{[...hidden].sort().join(',') || 'none'}</span>
    </div>
  )
}

describe('arbitrary-entity picks beyond the curated 18 (F4MQS)', () => {
  it('an entity name toggles into the hidden set and back, persisting separately', () => {
    render(<EntityProbe />)
    expect(screen.getByTestId('ents').textContent).toBe('none')
    fireEvent.click(screen.getByTestId('tog'))
    expect(screen.getByTestId('ents').textContent).toBe('Fiji')
    expect(screen.getByTestId('ehidden').textContent).toBe('Fiji') // resolved into hidden
    // Stored under the SEPARATE key, leaving the curated-key store untouched.
    expect(localStorage.getItem('nexus.decodes.countryExclude.entities')).toBe('["Fiji"]')
    expect(localStorage.getItem(COUNTRY_EXCLUDE_KEY)).toBeNull()
    fireEvent.click(screen.getByTestId('tog'))
    expect(screen.getByTestId('ehidden').textContent).toBe('none')
  })
})

describe('pausing keeps the ticks but hides nothing (F4MQS)', () => {
  it('pause empties the hidden set while the ticks survive; resume restores them', () => {
    render(<PauseProbe />)
    fireEvent.click(screen.getByTestId('tick'))
    expect(screen.getByTestId('keys').textContent).toBe('dl')
    expect(screen.getByTestId('hidden').textContent).not.toBe('none') // hiding Germany

    fireEvent.click(screen.getByTestId('pause'))
    expect(screen.getByTestId('keys').textContent).toBe('dl') // ticks kept
    expect(screen.getByTestId('hidden').textContent).toBe('none') // but nothing hidden

    fireEvent.click(screen.getByTestId('pause'))
    expect(screen.getByTestId('hidden').textContent).not.toBe('none') // resumed
  })
})

describe('every mounted consumer converges on one list', () => {
  it('a toggle in one pane reaches the other, same window', () => {
    render(
      <>
        <Probe id="a" />
        <Probe id="b" />
      </>,
    )
    expect(shows('a')).toBe('none')
    expect(shows('b')).toBe('none')

    fireEvent.click(screen.getByTestId('a'))
    // The native `storage` event does NOT fire in the document that wrote it, so a
    // same-window broadcast is the only thing that can keep these two in step.
    expect(shows('a')).toBe('dl')
    expect(shows('b')).toBe('dl')

    fireEvent.click(screen.getByTestId('b'))
    expect(shows('a')).toBe('none')
    expect(shows('b')).toBe('none')
  })

  it('a write from ANOTHER window lands too, and is RE-READ rather than trusted', () => {
    render(<Probe id="a" />)
    // What a second webview's write looks like here: storage already mutated, then the
    // event. The listener re-reads storage, so an event carrying no payload still lands.
    localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl","ea"]')
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: COUNTRY_EXCLUDE_KEY }))
    })
    expect(shows('a')).toBe('dl,ea')
  })

  it('ignores a storage event for some other key', () => {
    render(<Probe id="a" />)
    localStorage.setItem(COUNTRY_EXCLUDE_KEY, '["dl"]')
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: 'tempo-theme' }))
    })
    expect(shows('a')).toBe('none')
  })
})

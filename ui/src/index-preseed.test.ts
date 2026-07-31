// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// index.html's pre-paint seed script, executed for real: it is the only thing standing
// between launch and a first-paint flash, and it must mirror the React hooks EXACTLY
// (useScale fit model, useViewport --vh/vw-eff, usePaneWidths rail clamps). The rail
// case that motivated this file: with NOTHING stored the seed used to skip the rails
// entirely, so every launch of a never-dragged install first-painted the :root 300px
// fallback and then jumped to usePaneWidths' proportional default (~319px wider at
// 3440) — the first-paint-flash class (census F15) on EVERY launch, not just the first
// (review 2026-07-31).

// (import.meta.url is an http: URL under the jsdom environment — resolve from the
// vitest cwd, which is the ui/ project root.)
const html = readFileSync(resolve(process.cwd(), 'index.html'), 'utf8')
// The preseed is the first PLAIN <script> (the app entry is type="module").
const src = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]

function setWin(w: number, h: number) {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true })
  Object.defineProperty(window, 'innerHeight', { value: h, configurable: true, writable: true })
}

function runPreseed() {
  new Function(src!)()
}

const railVar = (name: string) => document.documentElement.style.getPropertyValue(name)

beforeEach(() => {
  localStorage.clear()
  window.history.replaceState(null, '', '/')
  document.documentElement.removeAttribute('style')
})

describe('index.html preseed', () => {
  it('has an inline preseed script at all', () => {
    expect(src, 'no plain <script> block found in index.html').toBeTruthy()
  })

  it('never-dragged install: rails seed usePaneWidths’ proportional default pre-paint', () => {
    setWin(3440, 1440) // auto mode, default cap 100 → zoom 1 → effective width 3440
    runPreseed()
    expect(railVar('--vh-eff')).not.toBe('') // sanity: the script ran
    // clampLeft(round(0.18·ew)) / clampRight(round(0.22·ew)) — usePaneWidths' exact
    // default formula. The :root fallbacks are 300/360px; anything else here flashes.
    expect(railVar('--left-rail-w')).toBe('619px')
    expect(railVar('--right-rail-w')).toBe('757px')
  })

  it('never-dragged install: the default seed respects the 220/260 px floors', () => {
    setWin(700, 500) // fit floor 65 → ew ≈ 1077 → raw defaults 194/237, under the floors
    runPreseed()
    expect(railVar('--left-rail-w')).toBe('220px')
    expect(railVar('--right-rail-w')).toBe('260px')
  })

  it('stored rail widths still replay clamped against THIS window (unchanged path)', () => {
    setWin(1366, 768) // fit 85 → ew ≈ 1607; 60% ceiling ≈ 964
    localStorage.setItem('tempo-right-rail-w', '2064') // legal on 3440, not here
    runPreseed()
    expect(railVar('--right-rail-w')).toBe('964px')
    // Storage is never rewritten by the seed — the big-monitor preference survives.
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('2064')
  })
})

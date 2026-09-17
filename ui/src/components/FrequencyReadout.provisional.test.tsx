// @vitest-environment jsdom
//
// THE OPTIMISTIC DIAL — the readout half. Remote has no hand on the knob, so the screen has to
// supply the feedback the hand used to: the digits move on the gesture, not a round trip later.
//
// The whole safety of that rests on one property, and it is this file's subject: the optimistic
// number is a DISPLAY value and reaches nothing else. `dialMhz` — the station's own reading — is
// what the band chip, the privilege red, the announcement and (everywhere outside this component)
// the mode, the sideband, the shading strip, TX enable and the S-meter keep going on. So the tests
// below are mostly about what the provisional value does NOT do.
//
// The other half is honesty: while the station has not read it back the number must never look
// like the radio's. Dimmed, trailed by an ellipsis, `aria-busy` for a reader who sees neither —
// and `WheelTuning` reverts it, and says so, if the confirmation never comes (optimistic-dial.test).
import { describe, it, expect, vi, afterEach, beforeAll } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { FrequencyReadout } from './FrequencyReadout'
import { atToken, css, loadSheets } from '../cssCascade.testkit'
import { MODES, expandWith, parseHex, parseRules, rootTokensFrom, type Mode } from '../cssCascade'

const announced: string[] = []
vi.mock('../announce', () => ({ announce: (text: string) => { announced.push(text) } }))

afterEach(() => { cleanup(); announced.length = 0 })

const readout = (root: ParentNode) => root.querySelector('.readout') as HTMLElement
const value = (root: ParentNode) => root.querySelector('.readout-val')!.textContent

describe('what the operator sees while the station has not answered', () => {
  it('shows the dial that was asked for, marked as not yet the station\'s word', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.0752} />)
    expect(value(container)).toBe('14.0752')
    expect(readout(container).getAttribute('aria-busy')).toBe('true')
    expect(readout(container).className).toContain('awaiting')
    const glyph = container.querySelector('.readout-awaiting')!
    expect(glyph.textContent).toBe('…')
    expect(glyph.className).not.toContain('settled')
    expect(glyph.getAttribute('aria-hidden'), 'decoration; aria-busy carries the meaning').toBe('true')
  })

  it('goes back to an ordinary dial the moment the station\'s reading agrees', () => {
    const { container, rerender } = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.0752} />)
    rerender(<FrequencyReadout dialMhz={14.0752} provisionalMhz={14.0752} />)
    expect(value(container)).toBe('14.0752')
    expect(readout(container).getAttribute('aria-busy')).toBeNull()
    expect(readout(container).className).not.toContain('awaiting')
    // The glyph stays MOUNTED and goes invisible. It is a flex item beside the unit and the band
    // chip, so unmounting it per burst would nudge both on every step of a spin.
    expect(container.querySelector('.readout-awaiting')?.className).toContain('settled')
  })

  it('marks a step too fine to change the printed digits — asked for is still not confirmed', () => {
    // 10 Hz on a 100 Hz display: the number does not move, so the dim and the ellipsis are the
    // only thing saying a command is out. Comparing the FORMATTED text would call this confirmed.
    const { container } = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.07401} />)
    expect(value(container)).toBe('14.0740')
    expect(readout(container).getAttribute('aria-busy')).toBe('true')
  })

  it('without the prop the readout is what it always was — no marker, no busy, no extra node', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} band="20m" />)
    expect(value(container)).toBe('14.0740')
    expect(readout(container).getAttribute('aria-busy')).toBeNull()
    expect(readout(container).className).not.toContain('awaiting')
    expect(container.querySelector('.readout-awaiting')).toBeNull()
  })
})

describe('the provisional value decides the digits and nothing else', () => {
  it('cannot turn the privilege red on or off — that is the caller\'s answer about the STATION\'s dial', () => {
    // The optimistic number is 10 MHz away and out of every band; the readout stays accent, because
    // `txBlocked` is `!radio.txAllowed` — the backend's verdict on where the radio actually is.
    const { container } = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={24.074} />)
    expect(readout(container).className).not.toContain('blocked')
    // ...and a blocked dial stays blocked while a command is out: the red outranks the dim.
    const blocked = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.0752} txBlocked />)
    expect(readout(blocked.container).className).toContain('blocked')
  })

  it('does not change the band chip', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} band="20m" provisionalMhz={24.074} />)
    expect(container.querySelector('.band-chip')?.textContent).toBe('20m')
  })

  it('is not announced: a screen reader is told where the rig LANDED, never where we aimed', () => {
    const { rerender } = render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={vi.fn()} />)
    rerender(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.2} digitTune editable onCommit={vi.fn()} onTuneHz={vi.fn()} />)
    expect(announced).toEqual([])
  })
})

describe('the pending ink', () => {
  beforeAll(() => loadSheets())

  it('paints the digits with --state-pending while awaiting, and leaves them alone otherwise', () => {
    // The cascade WINNER, resolved over the real sheet — never a presence check. The token is
    // stood in for by a sentinel ink, so the assertion is about which rule wins, not about a hex.
    const awaiting = render(<FrequencyReadout dialMhz={14.074} provisionalMhz={14.0752} />)
    const settled = render(<FrequencyReadout dialMhz={14.074} />)
    const ink = (root: ParentNode) => atToken('--state-pending', '#abcdef', () => css(root.querySelector('.readout-val')!, 'color'))
    expect(ink(awaiting.container)).toBe('#abcdef')
    expect(ink(settled.container)).not.toBe('#abcdef')
    // ...and the ellipsis beside it carries the same ink.
    expect(atToken('--state-pending', '#abcdef', () => css(awaiting.container.querySelector('.readout-awaiting')!, 'color'))).toBe('#abcdef')
  })

  it('--state-pending resolves in EVERY theme, and is not the confirmed ink in any of them', () => {
    // Computed, never a presence check: the token is resolved through the same cascade the page
    // uses, in each mode, so a declaration that loses (the --need-* class of bug) reads as absent.
    const raw = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8')
    const rules = parseRules(raw.replace(/\/\*[\s\S]*?\*\//g, m => m.replace(/[^\n]/g, ' ')))
    const resolved = (mode: Mode, token: string) => expandWith(rootTokensFrom(rules, mode), `var(${token})`)
    for (const mode of MODES) {
      const pending = resolved(mode, '--state-pending')
      expect(parseHex(pending), `--state-pending is a real colour in ${mode}`).not.toBeNull()
      expect(pending, `--state-pending is distinguishable from the confirmed dial in ${mode}`)
        .not.toBe(resolved(mode, '--accent'))
    }
    // Control: a token nothing defines resolves to nothing, so the assertions above can fail.
    expect(parseHex(resolved(MODES[0], '--state-pending-not-a-token'))).toBeNull()
  })
})

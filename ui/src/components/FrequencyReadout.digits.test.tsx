// @vitest-environment jsdom
//
// PER-DIGIT TUNING — the readout half: the hit regions, the decade each one carries, and the
// keyboard equivalent. The WHEEL half (one event ⇒ one CAT write) is CockpitHeader.digitTune.
//
// The operator's ask: "mouse over any of the numbers and scroll those numbers" — hover the
// 100 Hz digit and one notch moves 100 Hz; hover the 1 MHz digit and one notch moves 1 MHz,
// carrying like a real VFO (14.1990 +1 kHz = 14.2000). Mechanically that is `freq += 10**n` Hz,
// so this component's whole job is to say WHICH n is under the pointer.
//
// Three things this file exists to stop:
//  1. A FIXED INDEX. The integer part is 1 char on 160 m (1.8340) and 4 on 23 cm (1296.1740), so
//     "the 1 MHz digit is at index 1" is wrong on almost every band. The decade is read off the
//     string's own shape, and the table below is the proof across the whole dial.
//  2. THE EXCLUDED SURFACES. TopBar, Settings and the memory rows render this same component
//     inside scrollable lists, where capturing the wheel would trap normal page scrolling. The
//     digits are opt-in; with the prop off the readout must render exactly the single text node
//     it always did.
//  3. JITTER (and the Space-PTT class of bug). `.readout-val` is tabular-nums specifically so
//     digits don't move while tuning; a `.readout-digit` rule that sets any layout property
//     brings the jitter back. And nine new key handlers on a role=button span is nine new places
//     Space could reach the cockpit's window-level PTT — see FrequencyReadout.tsx:103.
import { describe, it, expect, vi, afterEach, beforeAll } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { FrequencyReadout, dialDigits, formatDialMhz } from './FrequencyReadout'
import { css, loadSheets } from '../cssCascade.testkit'

afterEach(cleanup)

/** The decade (power of ten, Hz) of every digit, keyed by the character position. */
const decadesOf = (text: string) => dialDigits(text).map((d) => d.decade)

describe('dialDigits — the decade under the pointer', () => {
  it('maps every digit of a formatted dial, from 630 m to 3 cm', () => {
    // ONE row per integer-part length, because the length is exactly what a fixed index gets
    // wrong. `null` = not a digit (the decimal point) and is inert by construction.
    expect(decadesOf(formatDialMhz(0.4742))).toEqual([6, null, 5, 4, 3, 2]) // 630 m
    expect(decadesOf(formatDialMhz(1.834))).toEqual([6, null, 5, 4, 3, 2]) // 160 m
    expect(decadesOf(formatDialMhz(14.074))).toEqual([7, 6, null, 5, 4, 3, 2]) // 20 m
    expect(decadesOf(formatDialMhz(144.2))).toEqual([8, 7, 6, null, 5, 4, 3, 2]) // 2 m
    expect(decadesOf(formatDialMhz(1296.174))).toEqual([9, 8, 7, 6, null, 5, 4, 3, 2]) // 23 cm
    expect(decadesOf(formatDialMhz(10368.1))).toEqual([10, 9, 8, 7, 6, null, 5, 4, 3, 2]) // 3 cm
  })

  it('keeps the characters, in order, so the rendered text is unchanged', () => {
    for (const mhz of [0.4742, 14.074, 1296.174]) {
      const text = formatDialMhz(mhz)
      expect(dialDigits(text).map((d) => d.ch).join('')).toBe(text)
    }
  })

  it('a non-numeric dial (NaN) is wholly inert — no digit claims the wheel', () => {
    expect(decadesOf(formatDialMhz(Number.NaN))).toEqual([null, null, null])
  })
})

describe('FrequencyReadout digit hit regions', () => {
  const decadeAttrs = (root: ParentNode) =>
    [...root.querySelectorAll('[data-decade]')].map((el) => Number(el.getAttribute('data-decade')))

  it('digitTune splits the number into one hit region per digit, carrying its decade', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} digitTune />)
    expect(decadeAttrs(container)).toEqual([7, 6, 5, 4, 3, 2])
    // The rendered text is byte-identical — splitting is a DOM change, not a display change.
    expect(container.querySelector('.readout-val')?.textContent).toBe('14.0740')
    // The '.' is NOT a hit region and gets no wrapper: a bare text node, so wheeling the
    // separator falls through to whatever the parent does with a non-digit target.
    expect(container.querySelectorAll('.readout-digit')).toHaveLength(6)
  })

  it('WITHOUT digitTune the readout is the single text node it has always been', () => {
    // TopBar / SettingsPanel / the memory rows reach this component through FrequencyControl and
    // live inside scrollable lists. Capturing their wheel would trap the page scroll.
    const { container } = render(<FrequencyReadout dialMhz={14.074} />)
    expect(decadeAttrs(container)).toEqual([])
    expect(container.querySelector('.readout-val')?.childNodes).toHaveLength(1)
  })

  it('clicking a DIGIT still opens the MHz editor (click-to-type survives the split)', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} />)
    fireEvent.click(container.querySelector('.readout-digit')!)
    expect(container.querySelector('input')).not.toBeNull()
  })
})

describe('the keyboard equivalent (a screen-reader user cannot hover-and-scroll)', () => {
  /** The readout stays ONE tab stop. ARIA gives `button` presentational children, so nine
   *  focusable digits inside it would be both a violation and nine new tab stops ahead of
   *  Stop TX in every cockpit header. Left/Right pick the digit; Up/Down spin it. */
  const readout = () => screen.getByRole('button')

  it('Left/Right select a digit and Up/Down step it by that decade', () => {
    const onTuneHz = vi.fn()
    render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={onTuneHz} />)
    // Nothing selected yet ⇒ the first spin takes the finest digit (100 Hz, the display floor).
    fireEvent.keyDown(readout(), { key: 'ArrowUp' })
    expect(onTuneHz).toHaveBeenLastCalledWith(100)
    // Left walks toward the more significant digits: 100 Hz → 1 kHz → 10 kHz.
    fireEvent.keyDown(readout(), { key: 'ArrowLeft' })
    fireEvent.keyDown(readout(), { key: 'ArrowLeft' })
    fireEvent.keyDown(readout(), { key: 'ArrowDown' })
    expect(onTuneHz).toHaveBeenLastCalledWith(-10_000)
    // Right walks back down and stops at the finest digit rendered.
    for (let i = 0; i < 5; i++) fireEvent.keyDown(readout(), { key: 'ArrowRight' })
    fireEvent.keyDown(readout(), { key: 'ArrowUp' })
    expect(onTuneHz).toHaveBeenLastCalledWith(100)
  })

  it('selection clamps to the digits that EXIST on this band (10 MHz is the top of 14.0740)', () => {
    const onTuneHz = vi.fn()
    render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={onTuneHz} />)
    for (let i = 0; i < 12; i++) fireEvent.keyDown(readout(), { key: 'ArrowLeft' })
    fireEvent.keyDown(readout(), { key: 'ArrowUp' })
    expect(onTuneHz).toHaveBeenLastCalledWith(10_000_000) // not 100 MHz — no such digit is shown
  })

  it('marks the selected digit so a sighted keyboard user can see which one spins', () => {
    const { container } = render(
      <FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={vi.fn()} />,
    )
    expect(container.querySelector('.readout-digit.sel')).toBeNull() // nothing until asked
    fireEvent.keyDown(readout(), { key: 'ArrowLeft' })
    const sel = container.querySelector('.readout-digit.sel')
    expect(sel?.getAttribute('data-decade')).toBe('2')
  })

  it('arrows do NOT propagate — the same guard Space needs (window-level PTT / Esc-abort)', () => {
    const spy = vi.fn()
    window.addEventListener('keydown', spy)
    try {
      render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={vi.fn()} />)
      for (const key of ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']) {
        fireEvent.keyDown(readout(), { key })
      }
      expect(spy).not.toHaveBeenCalled()
    } finally {
      window.removeEventListener('keydown', spy)
    }
  })

  it('Enter and Space still open the editor — the digits did not take the activation keys', () => {
    render(<FrequencyReadout dialMhz={14.074} digitTune editable onCommit={vi.fn()} onTuneHz={vi.fn()} />)
    fireEvent.keyDown(readout(), { key: 'Enter' })
    expect(document.querySelector('input')).not.toBeNull()
  })

  it('does not tune when there is nowhere to send it (no onTuneHz ⇒ arrows are inert)', () => {
    // RTTY/SSTV render the readout display-only when the host passes no frequency setter.
    render(<FrequencyReadout dialMhz={14.074} digitTune />)
    expect(screen.queryByRole('button')).toBeNull()
  })
})

describe('the hover affordance may not move a single glyph', () => {
  beforeAll(() => loadSheets())

  /** Every property that would change the advance width or the line box of an inline digit.
   *  A `font-weight` bump on hover is the classic jitter re-introducer. */
  const LAYOUT_PROPS = [
    'display',
    'width',
    'padding',
    'padding-left',
    'padding-right',
    'padding-top',
    'padding-bottom',
    'margin',
    'margin-left',
    'margin-right',
    'border',
    'border-width',
    'font-size',
    'font-family',
    'font-weight',
    'font-variant-numeric',
    'letter-spacing',
    'transform',
    'line-height',
  ] as const

  // WHAT THIS DOES NOT PROVE, written down rather than chased: jsdom's `matches()` answers
  // `:hover` false, so a rule hung on `.readout-digit:hover` is invisible to the resolver below.
  // The resting state is what the guard computes — and it is the state that fixes every digit's
  // advance width, so a padding/font-weight there is caught. A layout property added to the
  // HOVER rule specifically would not be.
  it('.readout-digit declares no layout-affecting property', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} digitTune />)
    const digit = container.querySelector('.readout-digit')!
    for (const prop of LAYOUT_PROPS) {
      expect(
        css(digit, prop),
        `.readout-digit wins \`${prop}\` — splitting the number into spans is only free while ` +
          'every digit keeps the identical advance. This is how the tuning jitter comes back.',
      ).toBeNull()
    }
    // …and the selected-digit state is paint-only for the same reason.
    digit.classList.add('sel')
    for (const prop of LAYOUT_PROPS) expect(css(digit, prop)).toBeNull()
  })

  it('.readout-val still wins tabular-nums over the split digits', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} digitTune />)
    expect(css(container.querySelector('.readout-val')!, 'font-variant-numeric')).toBe('tabular-nums')
  })

  it('the digits look live on hover, and the affordance follows the ink', () => {
    const { container } = render(<FrequencyReadout dialMhz={14.074} digitTune />)
    const digit = container.querySelector('.readout-digit')!
    expect(css(digit, 'cursor'), 'a live digit must LOOK live under the pointer').not.toBeNull()
    // The out-of-band readout is red (`.readout.blocked .readout-val { color: var(--tx) }`) and
    // that red is the operator's band-edge cue — a digit rule that set its own `color` would
    // override it. The hover tint is mixed from currentColor so it follows the ink instead.
    expect(css(digit, 'color')).toBeNull()
  })
})

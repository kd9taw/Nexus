import { describe, it, expect } from 'vitest'
import {
  clampOffsetHz,
  formatReport,
  genStdMessages,
  gridFromMessage,
  isIgnored,
  snrForCall,
  stdMessageList,
  toggleIgnored,
  txGrid4,
} from './txMessages'

describe('report formatting (WSJT-X sign + two digits)', () => {
  it('formats positive and negative reports with a forced sign + 2 digits', () => {
    expect(formatReport(5)).toBe('+05')
    expect(formatReport(-12)).toBe('-12')
    expect(formatReport(0)).toBe('+00')
    expect(formatReport(15)).toBe('+15')
  })

  it('clamps to the protocol range −30…+49', () => {
    expect(formatReport(-45)).toBe('-30')
    expect(formatReport(60)).toBe('+49')
  })

  it('falls back to −10 for an unheard station', () => {
    expect(formatReport(null)).toBe('-10')
    expect(formatReport(undefined)).toBe('-10')
  })

  it('rounds fractional SNRs', () => {
    expect(formatReport(-9.6)).toBe('-10')
    expect(formatReport(4.4)).toBe('+04')
  })
})

describe('standard message generation (stock Tx1–Tx6)', () => {
  const base = { dxCall: 'K1ABC', myCall: 'KD9TAW', myGrid: 'EN52' }

  it('generates the six stock messages', () => {
    const m = genStdMessages({ ...base, snr: -9 })
    expect(m.tx1).toBe('K1ABC KD9TAW EN52')
    expect(m.tx2).toBe('K1ABC KD9TAW -09')
    expect(m.tx3).toBe('K1ABC KD9TAW R-09')
    expect(m.tx4).toBe('K1ABC KD9TAW RR73')
    expect(m.tx5).toBe('K1ABC KD9TAW 73')
    expect(m.tx6).toBe('CQ KD9TAW EN52')
  })

  it('uses RRR instead of RR73 when prefer-RRR is on', () => {
    expect(genStdMessages({ ...base, preferRrr: true }).tx4).toBe('K1ABC KD9TAW RRR')
    expect(genStdMessages({ ...base, preferRrr: false }).tx4).toBe('K1ABC KD9TAW RR73')
  })

  it('truncates a 6-char locator to the on-air 4-char grid', () => {
    const m = genStdMessages({ ...base, myGrid: 'EN52xa' })
    expect(m.tx1).toBe('K1ABC KD9TAW EN52')
    expect(m.tx6).toBe('CQ KD9TAW EN52')
  })

  it('omits the grid when the locator is missing or invalid (grid fallback)', () => {
    const none = genStdMessages({ ...base, myGrid: '' })
    expect(none.tx1).toBe('K1ABC KD9TAW')
    expect(none.tx6).toBe('CQ KD9TAW')
    expect(genStdMessages({ ...base, myGrid: '????' }).tx6).toBe('CQ KD9TAW')
  })

  it('blanks Tx1–Tx5 (but keeps CQ) with no DX call selected', () => {
    const m = genStdMessages({ ...base, dxCall: '' })
    expect(stdMessageList(m).slice(0, 5)).toEqual(['', '', '', '', ''])
    expect(m.tx6).toBe('CQ KD9TAW EN52')
  })

  it('normalizes callsign + grid case', () => {
    const m = genStdMessages({ dxCall: 'k1abc', myCall: 'kd9taw', myGrid: 'en52' })
    expect(m.tx1).toBe('K1ABC KD9TAW EN52')
  })
})

describe('grid extraction from a decode (single-click populate)', () => {
  it('takes a trailing 4-char grid', () => {
    expect(gridFromMessage('CQ W9XYZ EN52')).toBe('EN52')
    expect(gridFromMessage('CQ DX K2DEF FN20')).toBe('FN20')
  })

  it('NEVER reads RR73 as a grid (the WSJT-X reserved token)', () => {
    expect(gridFromMessage('KD9TAW W9XYZ RR73')).toBeUndefined()
  })

  it('ignores reports, rogers and 73s', () => {
    expect(gridFromMessage('KD9TAW W9XYZ -12')).toBeUndefined()
    expect(gridFromMessage('KD9TAW W9XYZ R-09')).toBeUndefined()
    expect(gridFromMessage('KD9TAW W9XYZ RRR')).toBeUndefined()
    expect(gridFromMessage('KD9TAW W9XYZ 73')).toBeUndefined()
    expect(gridFromMessage('')).toBeUndefined()
  })

  it('txGrid4 validates the locator shape', () => {
    expect(txGrid4('en52')).toBe('EN52')
    expect(txGrid4('ZZ99')).toBe('') // S–Z fields don't exist
    expect(txGrid4(null)).toBe('')
  })
})

describe('snrForCall (the RPT source)', () => {
  const stations = [
    { call: 'K1ABC', snr: -7 },
    { call: 'W9XYZ', snr: 3 },
  ]

  it('matches case-insensitively', () => {
    expect(snrForCall(stations, 'k1abc')).toBe(-7)
    expect(snrForCall(stations, ' W9XYZ ')).toBe(3)
  })

  it('returns null when unheard (→ −10 fallback downstream)', () => {
    expect(snrForCall(stations, 'VK0DX')).toBeNull()
    expect(snrForCall(stations, '')).toBeNull()
  })
})

describe('session ignore set (Alt-double-click)', () => {
  it('toggles a call in (uppercased) and back out, case-insensitively', () => {
    const a = toggleIgnored(new Set(), 'k1abc')
    expect(a.has('K1ABC')).toBe(true)
    expect(isIgnored(a, 'K1abc')).toBe(true)
    const b = toggleIgnored(a, 'K1ABC')
    expect(b.size).toBe(0)
    expect(isIgnored(b, 'K1ABC')).toBe(false)
  })

  it('never mutates the input set (safe for React state)', () => {
    const orig: ReadonlySet<string> = new Set(['W9XYZ'])
    const next = toggleIgnored(orig, 'K1ABC')
    expect(orig.size).toBe(1)
    expect(next.size).toBe(2)
  })

  it('ignores blank calls', () => {
    expect(toggleIgnored(new Set(), '  ').size).toBe(0)
    expect(isIgnored(new Set(['K1ABC']), null)).toBe(false)
  })
})

describe('DF entry clamp (200–2900 Hz)', () => {
  it('rounds and clamps', () => {
    expect(clampOffsetHz(1500.4)).toBe(1500)
    expect(clampOffsetHz(12)).toBe(200)
    expect(clampOffsetHz(9000)).toBe(2900)
  })
})

describe('compound-call (i3=4) display parity', () => {
  // Mirrors qso.rs::compound_form — the panel must show what goes ON AIR.
  it('hashes the DX and drops grids for a compound DX', () => {
    const m = genStdMessages({ dxCall: 'KD9TAW/P', myCall: 'W9XYZ', myGrid: 'EN37', snr: -8 })
    expect(m.tx1).toBe('<KD9TAW/P> W9XYZ')
    expect(m.tx2).toBe('<KD9TAW/P> W9XYZ -08')
    expect(m.tx4).toBe('<KD9TAW/P> W9XYZ RR73')
    expect(m.tx6).toBe('CQ W9XYZ')
  })
  it('a compound SENDER cannot carry a numeric report', () => {
    const m = genStdMessages({ dxCall: 'K1ABC', myCall: 'W9XYZ/P', myGrid: 'EN37', snr: -8 })
    expect(m.tx2).toBe('<K1ABC> W9XYZ/P')
    expect(m.tx3).toBe('<K1ABC> W9XYZ/P RRR')
  })
  it('standard calls are untouched', () => {
    const m = genStdMessages({ dxCall: 'K1ABC', myCall: 'W9XYZ', myGrid: 'EN37', snr: 3 })
    expect(m.tx1).toBe('K1ABC W9XYZ EN37')
    expect(m.tx2).toBe('K1ABC W9XYZ +03')
  })
})

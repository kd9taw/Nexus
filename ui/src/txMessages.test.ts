import { describe, it, expect } from 'vitest'
import {
  clampOffsetHz,
  cqDirFromText,
  formatReport,
  genStdMessages,
  gridFromMessage,
  isIgnored,
  isStdCall,
  packsBesideHash,
  snrForCall,
  suffixConflict,
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

describe('DF entry clamp (200–4000 Hz)', () => {
  it('rounds and clamps', () => {
    expect(clampOffsetHz(1500.4)).toBe(1500)
    expect(clampOffsetHz(12)).toBe(200)
    expect(clampOffsetHz(9000)).toBe(4000)
  })
})

describe('nonstandard-call (hashed) display parity', () => {
  // Mirrors qso.rs::nonstandard_form — the panel must show what goes ON AIR, or
  // snap.qso.txNow stops matching a row.
  it('a /P or /R station keeps its grid and its reports', () => {
    // The reported bug: F4CYH/P's panel read `CQ  F4CYH/P` with no grid, and his Tx1
    // and Tx2 rendered to the SAME string, so he could send neither locator nor report.
    const m = genStdMessages({ dxCall: 'W9XYZ', myCall: 'F4CYH/P', myGrid: 'JN18', snr: -8 })
    expect(m.tx1).toBe('W9XYZ F4CYH/P JN18')
    expect(m.tx2).toBe('W9XYZ F4CYH/P -08')
    expect(m.tx3).toBe('W9XYZ F4CYH/P R-08')
    expect(m.tx4).toBe('W9XYZ F4CYH/P RR73')
    expect(m.tx5).toBe('W9XYZ F4CYH/P 73')
    expect(m.tx6).toBe('CQ F4CYH/P JN18')
    // Working a /P DX is equally in the clear — nothing hashed in either direction.
    const dx = genStdMessages({ dxCall: 'F4CYH/P', myCall: 'W9XYZ', myGrid: 'EN37', snr: -8 })
    expect(dx.tx1).toBe('F4CYH/P W9XYZ EN37')
    expect(dx.tx2).toBe('F4CYH/P W9XYZ -08')
    const r = genStdMessages({ dxCall: 'W9XYZ', myCall: 'KD9TAW/R', myGrid: 'EN52', snr: 3 })
    expect(r.tx1).toBe('W9XYZ KD9TAW/R EN52')
    expect(r.tx2).toBe('W9XYZ KD9TAW/R +03')
  })
  it('hashes a genuinely nonstandard DX, and my grid still rides along', () => {
    const m = genStdMessages({ dxCall: 'PJ4/K1ABC', myCall: 'W9XYZ', myGrid: 'EN37', snr: -8 })
    expect(m.tx1).toBe('<PJ4/K1ABC> W9XYZ EN37')
    expect(m.tx2).toBe('<PJ4/K1ABC> W9XYZ -08')
    expect(m.tx4).toBe('<PJ4/K1ABC> W9XYZ RR73')
    expect(m.tx6).toBe('CQ W9XYZ EN37')
  })
  it('a nonstandard SENDER cannot carry a grid or a numeric report', () => {
    const m = genStdMessages({ dxCall: 'K1ABC', myCall: 'PJ4/W9XYZ', myGrid: 'EN37', snr: -8 })
    expect(m.tx1).toBe('<K1ABC> PJ4/W9XYZ')
    expect(m.tx2).toBe('<K1ABC> PJ4/W9XYZ')
    expect(m.tx3).toBe('<K1ABC> PJ4/W9XYZ RRR')
    expect(m.tx6).toBe('CQ PJ4/W9XYZ') // never hashed — a hashed CQ does not unpack
  })
  it('standard calls are untouched', () => {
    const m = genStdMessages({ dxCall: 'K1ABC', myCall: 'W9XYZ', myGrid: 'EN37', snr: 3 })
    expect(m.tx1).toBe('K1ABC W9XYZ EN37')
    expect(m.tx2).toBe('K1ABC W9XYZ +03')
  })
  it('isStdCall matches WSJT-X stdCall — /P and /R in, everything else out', () => {
    for (const c of ['W9XYZ', 'KD9TAW', 'W1AW', '9A1A', '2E0ABC', 'F4CYH/P', 'KD9TAW/R', 'f4cyh/p'])
      expect(isStdCall(c), c).toBe(true)
    for (const c of ['PJ4/K1ABC', 'KD9TAW/QRP', 'KD9TAW/3', 'YW18FIFA', '', 'AB', '<W9XYZ>'])
      expect(isStdCall(c), c).toBe(false)
  })
  it('suffixConflict is a property of the PAIR — /P beside /R only', () => {
    expect(suffixConflict('KD9TAW/R', 'F4CYH/P')).toBe(true)
    expect(suffixConflict('KD9TAW/P', 'F4CYH/R')).toBe(true)
    for (const [a, b] of [
      ['KD9TAW/P', 'F4CYH/P'],
      ['KD9TAW/R', 'F4CYH/R'],
      ['KD9TAW/P', 'W9XYZ'],
      ['KD9TAW', 'F4CYH/R'],
      ['KD9TAW', 'W9XYZ'],
      ['W1/P', 'F4CYH/R'], // '/' before position 4 is not a suffix to the packer
    ])
      expect(suffixConflict(a, b), `${a} × ${b}`).toBe(false)
  })
  it('packsBesideHash is narrower than isStdCall — any slash is refused', () => {
    for (const c of ['KD9TAW', 'W9XYZ', '9A1A']) expect(packsBesideHash(c), c).toBe(true)
    for (const c of ['KD9TAW/P', 'KD9TAW/R', 'PJ4/K1ABC', 'YW18FIFA'])
      expect(packsBesideHash(c), c).toBe(false)
  })
})

describe('the pairs the packer cannot express — panel parity with the air', () => {
  // Every expectation below is the text the RUST round trip actually recovered off the
  // air (tempo-core/tests/portable_suffix_air.rs, build → pack → waveform → decode),
  // so the panel is pinned to the modem and not to itself.
  const grid = 'EN52'
  const gen = (myCall: string, dxCall: string) =>
    stdMessageList(genStdMessages({ dxCall, myCall, myGrid: grid, snr: -7 }))

  it('a /P × /R pair falls back to the hashed form rather than rename a station', () => {
    // One `i3` per frame says what BOTH suffix bits mean, so `F4CYH/P KD9TAW/R EN52`
    // goes on the air as `F4CYH/P KD9TAW/P EN52` — a station that does not exist. The
    // hashed form carries both calls verbatim; the grid and the number are the price.
    expect(gen('KD9TAW/R', 'F4CYH/P')).toEqual([
      '<F4CYH/P> KD9TAW/R',
      '<F4CYH/P> KD9TAW/R',
      '<F4CYH/P> KD9TAW/R RRR',
      '<F4CYH/P> KD9TAW/R RR73',
      '<F4CYH/P> KD9TAW/R 73',
      'CQ KD9TAW/R EN52', // a CQ has no pair — still an ordinary Type 1 frame
    ])
    expect(gen('KD9TAW/P', 'F4CYH/R')[0]).toBe('<F4CYH/R> KD9TAW/P')
    // Same suffix on both sides packs fine and stays in the clear.
    expect(gen('KD9TAW/P', 'F4CYH/P')[0]).toBe('F4CYH/P KD9TAW/P EN52')
    expect(gen('KD9TAW/R', 'F4CYH/R')[1]).toBe('F4CYH/R KD9TAW/R -07')
  })

  it('a /P operator working a hashed DX rogers with RRR, so Tx3 stays tellable', () => {
    // `packjt77.f90:1183-1184` refuses Type 1/2 when a hash sits beside any slashed
    // call, so grid and number are unsendable. Showing them anyway made Tx1, Tx2 and
    // Tx3 pack to identical bytes and stranded the partner's sequencer.
    const m = gen('KD9TAW/P', 'PJ4/K1ABC')
    expect(m).toEqual([
      '<PJ4/K1ABC> KD9TAW/P',
      '<PJ4/K1ABC> KD9TAW/P',
      '<PJ4/K1ABC> KD9TAW/P RRR',
      '<PJ4/K1ABC> KD9TAW/P RR73',
      '<PJ4/K1ABC> KD9TAW/P 73',
      'CQ KD9TAW/P EN52',
    ])
    expect(m[2]).not.toBe(m[0])
    expect(m[2]).not.toBe(m[1])
  })

  it('every callsign-class pair names both stations, and Tx3–Tx5 never collide', () => {
    for (const my of ['KD9TAW', 'KD9TAW/P', 'KD9TAW/R', 'YW18FIFA'])
      for (const dx of ['W9XYZ', 'F4CYH/P', 'F4CYH/R', 'PJ4/K1ABC']) {
        const m = gen(my, dx)
        for (let i = 0; i < 5; i++) {
          expect(m[i], `${my} × ${dx} tx${i + 1}`).toContain(my)
          expect(m[i], `${my} × ${dx} tx${i + 1}`).toContain(dx)
        }
        for (let a = 2; a < 5; a++)
          for (let b = 0; b < a; b++)
            expect(m[a], `${my} × ${dx}: tx${a + 1} vs tx${b + 1}`).not.toBe(m[b])
      }
  })
})

describe('cqDirFromText — directed CQ parser for Tx6', () => {
  const MY = 'KD9TAW'

  it('plain CQ (no token) returns null', () => {
    expect(cqDirFromText('CQ KD9TAW EN52', MY)).toBeNull()
    expect(cqDirFromText('CQ KD9TAW', MY)).toBeNull()
    expect(cqDirFromText('cq kd9taw en52', MY)).toBeNull()
  })

  it('returns the token for directed CQs — letter tokens', () => {
    expect(cqDirFromText('CQ DX KD9TAW EN52', MY)).toBe('DX')
    expect(cqDirFromText('CQ NA KD9TAW', MY)).toBe('NA')
    expect(cqDirFromText('CQ POTA KD9TAW', MY)).toBe('POTA')
    expect(cqDirFromText('CQ TEST KD9TAW EN52', MY)).toBe('TEST')
  })

  it('returns the token for 3-digit zone directed CQs', () => {
    expect(cqDirFromText('CQ 040 KD9TAW', MY)).toBe('040')
    expect(cqDirFromText('CQ 005 KD9TAW EN52', MY)).toBe('005')
  })

  it('is case-insensitive on keyword and token', () => {
    expect(cqDirFromText('cq dx kd9taw', MY)).toBe('DX')
    expect(cqDirFromText('CQ dx KD9TAW', MY)).toBe('DX')
  })

  it('returns undefined for garbage / wrong callsign', () => {
    expect(cqDirFromText('', MY)).toBeUndefined()
    expect(cqDirFromText('DE KD9TAW', MY)).toBeUndefined()
    expect(cqDirFromText('CQ W1ABC EN52', MY)).toBeUndefined()   // wrong callsign
    expect(cqDirFromText('CQ DX W1ABC EN52', MY)).toBeUndefined() // token present, wrong call
    expect(cqDirFromText('KD9TAW W1ABC EN52', MY)).toBeUndefined() // not a CQ
  })

  it('returns undefined when myCall is blank', () => {
    expect(cqDirFromText('CQ KD9TAW EN52', '')).toBeUndefined()
  })

  it('rejects 2-digit or 4-digit "zone" tokens (only exactly 3 digits)', () => {
    // 2-digit: interpreted as... not a valid token under the rule
    expect(cqDirFromText('CQ 04 KD9TAW', MY)).toBeUndefined()
    // 4-digit: looks like a GRID — invalid position
    expect(cqDirFromText('CQ 0400 KD9TAW', MY)).toBeUndefined()
  })

  it('rejects tokens longer than 4 letters', () => {
    // NEXUS = 5 letters — not a valid token
    expect(cqDirFromText('CQ NEXUS KD9TAW', MY)).toBeUndefined()
  })

  it('plain CQ with trailing invalid grid returns undefined', () => {
    expect(cqDirFromText('CQ KD9TAW EXTRA STUFF', MY)).toBeUndefined()
  })
})

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { processDecodes } from './alerts'
import { pushToast } from './toast'
import type { DecodeRow, Settings } from './types'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
const toasts = vi.mocked(pushToast)

// Node test env has no window; alerts.ts only needs setTimeout + (optional)
// AudioContext, and ensureCtx degrades to silent when the latter is absent.
vi.stubGlobal('window', { setTimeout } as unknown as Window & typeof globalThis)

const settings = { alertMyCall: true, alertNew: true, alertCq: false } as unknown as Settings

let seq = 0
function decode(over: Partial<DecodeRow>): DecodeRow {
  // Unique message per row so the exact-decode dedup never hides a test case.
  return {
    from: 'F5XYZ',
    message: `msg-${seq++}`,
    freqHz: 1500 + seq,
    directedToMe: false,
    newDxcc: false,
    newGrid: false,
    isCq: false,
    ...over,
  } as unknown as DecodeRow
}

beforeEach(() => toasts.mockClear())

describe('processDecodes QSO-aware quieting', () => {
  it('alerts "calling you" while idle/monitoring', () => {
    processDecodes([decode({ directedToMe: true })], settings, undefined, {
      state: 'Listening',
      dxcall: null,
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('calling you')
  })

  // ⚠️ THIS PAIR WAS SPLIT ON 2026-08-22, and the reason is the whole point. It used to assert
  // that CallingCq suppressed "calling you" alongside the mid-QSO states. That silenced the one
  // alert an operator running CQ is waiting for — a station answering THEM. Reported by an
  // operator whose every other alert worked. Calling CQ is not an exchange in progress; the
  // engine itself scores Listening and CallingCq as zero exchanges done.
  it('ANNOUNCES a station answering while we are calling CQ — the event CQ is for', () => {
    processDecodes([decode({ from: 'K1ABC', directedToMe: true })], settings, undefined, {
      state: 'CallingCq',
      dxcall: null,
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('calling you')
  })

  it('announces it during a Field Day CQ run too (FD state strings)', () => {
    processDecodes([decode({ from: 'W1AW', directedToMe: true })], settings, undefined, {
      state: 'CallingCq',
      dxcall: null,
    })
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('and STILL suppresses it once the exchange is under way', () => {
    // The chatty-popup fix, unchanged and still load-bearing: mycall dedups per DECODE, so
    // without this every message of a QSO would beep. Both state vocabularies.
    for (const state of ['AwaitReport', 'AwaitRoger', 'Confirming', 'AwaitExchange', 'AwaitConfirm']) {
      processDecodes([decode({ from: 'K1ABC', directedToMe: true })], settings, undefined, {
        state,
        dxcall: null,
      })
    }
    expect(toasts).not.toHaveBeenCalled()
  })

  it('goes quiet as soon as the answer moves the sequencer on', () => {
    // The sequence that matters end to end, and the reason exempting CallingCq costs nothing:
    // the answer alerts ONCE, then the sequencer leaves CallingCq and the rest of the exchange
    // is silent. A test per state cannot show this — only the transition can.
    processDecodes([decode({ from: 'K1ABC', directedToMe: true })], settings, undefined, {
      state: 'CallingCq',
      dxcall: null,
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    for (const state of ['AwaitReport', 'AwaitRoger', 'Confirming']) {
      processDecodes([decode({ from: 'K1ABC', directedToMe: true, message: `KD9TAW K1ABC ${state}` })], settings, undefined, {
        state,
        dxcall: 'K1ABC',
      })
    }
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('never pops anything about the station currently being worked', () => {
    processDecodes(
      [decode({ from: 'F5XYZ', directedToMe: true, newDxcc: true })],
      settings,
      undefined,
      { state: 'AwaitRoger', dxcall: 'f5xyz' }, // case-insensitive match
    )
    expect(toasts).not.toHaveBeenCalled()
  })

  it('still fires the loud new-DXCC alert for OTHER stations while engaged', () => {
    processDecodes(
      [decode({ from: 'ZL9DX', newDxcc: true, country: 'Auckland Is.' })],
      settings,
      undefined,
      { state: 'AwaitReport', dxcall: 'F5XYZ' },
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('NEW DXCC')
  })

  it('behaves as before when no QSO context is passed', () => {
    processDecodes([decode({ from: 'W1AW', directedToMe: true })], settings)
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('new-grid alerts are quiet: info toast, short, not prominent', () => {
    processDecodes([decode({ from: 'K7XYZ', newGrid: true, gridRarity: 'common' })], settings, undefined, {
      state: 'Listening',
      dxcall: null,
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    const [msg, kind, ttl, opts] = toasts.mock.calls[0]
    expect(msg).toContain('New grid')
    expect(kind).toBe('info')
    expect(ttl).toBe(6000)
    expect((opts as { prominent?: boolean } | undefined)?.prominent).toBeUndefined()
  })

  it('a RARE needed grid earns the loud prominent alert', () => {
    processDecodes(
      [decode({ from: 'K7XYZ/MM', newGrid: true, grid: 'RR73', gridRarity: 'ultraRare' })],
      settings,
      undefined,
      { state: 'Listening', dxcall: null },
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    const [msg, kind, , opts] = toasts.mock.calls[0]
    expect(msg).toContain('ULTRA-RARE grid RR73')
    expect(kind).toBe('success')
    expect(opts).toMatchObject({ prominent: true, actionLabel: 'Work' })
  })

  it('rare-grid alerts dedup per GRID, not per station', () => {
    const ctx = { state: 'Listening', dxcall: null }
    processDecodes(
      [decode({ from: 'K7AAA', newGrid: true, grid: 'JJ00', gridRarity: 'ultraRare' })],
      settings,
      undefined,
      ctx,
    )
    // A second rover in the SAME water grid isn't a second event.
    processDecodes(
      [decode({ from: 'W1BBB', newGrid: true, grid: 'JJ00', gridRarity: 'ultraRare' })],
      settings,
      undefined,
      ctx,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
  })
})

describe('per-alert band scopes (dialMhz gate)', () => {
  const ctx = { state: 'Listening', dxcall: null }

  it('plain new grid is SUPPRESSED on HF by default (scope vhf)', () => {
    processDecodes([decode({ from: 'N1GA', newGrid: true })], settings, undefined, ctx, undefined, 14.074)
    expect(toasts).not.toHaveBeenCalled()
  })

  it('plain new grid fires on 6 m by default', () => {
    processDecodes([decode({ from: 'N2GB', newGrid: true })], settings, undefined, ctx, undefined, 50.313)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('New grid')
  })

  it('unknown dial (no CAT) is permissive — grid alert still fires', () => {
    processDecodes([decode({ from: 'N3GC', newGrid: true })], settings, undefined, ctx)
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('new DXCC still fires on HF (default scope all)', () => {
    processDecodes(
      [decode({ from: 'N4GD', newDxcc: true, country: 'Bhutan' })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('NEW DXCC')
  })

  it('rare 💎 grid is QUIET on HF by default, and fires when the rare scope is widened', () => {
    // Operator ruling, 2026-08-15: "remove the grid alerts for HF bands by default." The rare
    // tier shipped scoped 'all', making it the one grid alert still firing on HF — and on HF
    // nearly every decode is an unworked grid, so even the rare subset read as chatter. The
    // default is now 'vhf' like the plain scope; an HF grid-chaser widens it in Settings.
    const rare = decode({ from: 'N5GE', newGrid: true, grid: 'AA11', gridRarity: 'ultraRare' })
    processDecodes([rare], settings, undefined, ctx, undefined, 14.074)
    expect(toasts, 'default: HF stays quiet, even for an ultra-rare grid').not.toHaveBeenCalled()

    // The opt-in still works — this is a scope change, not a removal.
    const wide = { ...settings, alertRareGridBands: 'all' } as unknown as Settings
    processDecodes([rare], wide, undefined, ctx, undefined, 14.074)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('ULTRA-RARE grid')

    // And on VHF the default still fires — the gems were never the problem there.
    const vhf = decode({ from: 'K0AA', newGrid: true, grid: 'BB22', gridRarity: 'ultraRare' })
    processDecodes([vhf], settings, undefined, ctx, undefined, 144.174)
    expect(toasts).toHaveBeenCalledTimes(2)
  })

  it('explicit Off silences grids even on VHF', () => {
    const s = { ...settings, alertGridBands: 'off', alertRareGridBands: 'off' } as unknown as Settings
    processDecodes([decode({ from: 'N6GF', newGrid: true })], s, undefined, ctx, undefined, 50.313)
    expect(toasts).not.toHaveBeenCalled()
  })

  it('rare scope Off demotes a gem to the quiet toast where plain grids are allowed', () => {
    const s = { ...settings, alertRareGridBands: 'off' } as unknown as Settings
    processDecodes(
      [decode({ from: 'N7GG', newGrid: true, grid: 'BB22', gridRarity: 'rare' })],
      s,
      undefined,
      ctx,
      undefined,
      50.313,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    const [msg, kind] = toasts.mock.calls[0]
    expect(msg).toContain('New grid')
    expect(kind).toBe('info')
  })

  it('legacy master off keeps everything silent regardless of scopes', () => {
    const s = { ...settings, alertNew: false } as unknown as Settings
    processDecodes(
      [decode({ from: 'N8GH', newGrid: true }), decode({ from: 'N9GI', newDxcc: true })],
      s,
      undefined,
      ctx,
      undefined,
      50.313,
    )
    expect(toasts).not.toHaveBeenCalled()
  })
})

describe('grid squares on the watch list', () => {
  const ctx = { state: 'Listening', dxcall: null }
  const hunt = [{ id: 'g1', kind: 'grid' as const, value: 'EM7*' }]

  it('a watched grid fires the loud ⭐ watch tier and consumes the decode', () => {
    processDecodes(
      [decode({ from: 'K4ROV', grid: 'EM79', newGrid: true, gridRarity: 'common' })],
      settings,
      undefined,
      ctx,
      hunt,
      50.313,
    )
    // Exactly ONE toast: the watch hit, not the watch hit plus a generic grid alert.
    expect(toasts).toHaveBeenCalledTimes(1)
    const [msg, , , opts] = toasts.mock.calls[0]
    expect(msg).toContain('⭐ Watch')
    expect(msg).toContain('grid EM7*')
    expect(opts).toMatchObject({ prominent: true, actionLabel: 'Work' })
  })

  it('fires on HF — a square asked for by name outranks the HF grid-quiet default', () => {
    // The 1.5.0 ruling scoped UNWORKED-grid chatter off HF. A grid the operator typed
    // into the watch list is not chatter; the watch branch runs above the scope gates.
    // (A different rover than the test above — the watch dedup is per filter+call and
    // module-global, so reusing K4ROV would test the dedup, not the scope.)
    processDecodes(
      [decode({ from: 'W9ROV', grid: 'EM77', newGrid: true })],
      settings,
      undefined,
      ctx,
      hunt,
      14.074,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('⭐ Watch')
  })
})

// ── Repeat-alert regression (operator, 0.20.x): "someone turned ATNO on and they
// were on the band with them and it alerted over and over ... on each cycle."
describe('alerts do not repeat every cycle', () => {
  it('the same station sending the same message does not re-alert when its measured frequency drifts', () => {
    // THE BUG: the alert dedup key was `from|message|round(freqHz)`. A station's
    // measured audio offset drifts a few Hz between transmissions, so rounded to
    // 1 Hz it minted a NEW key nearly every cycle and alerted again. Frequency is a
    // measurement, not identity.
    const cq = { alertMyCall: true, alertNew: true, alertCq: true } as unknown as Settings
    for (const hz of [1500.2, 1502.9, 1498.4, 1501.1, 1499.7]) {
      processDecodes(
        [
          {
            from: 'K1ABC',
            message: 'CQ K1ABC FN42',
            freqHz: hz,
            directedToMe: false,
            newDxcc: false,
            newGrid: false,
            isCq: true,
          } as unknown as DecodeRow,
        ],
        cq,
      )
    }
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('a new DXCC alerts ONCE even when heavy CQ traffic floods the dedup store', () => {
    // THE OTHER HALF, and the one the operator actually hit. Both kinds shared one
    // 2000-entry set with FIFO eviction. Every CQ decode minted a unique key, so a
    // busy band blew the cap in a minute or two and evicted the OLDEST entries —
    // including the `dxcc:` entry written when the new one was first heard. The ATNO
    // then re-alerted the next time that station decoded, and kept doing so.
    const both = { alertMyCall: true, alertNew: true, alertCq: true } as unknown as Settings
    const atno = () =>
      ({
        from: 'FT5ZM',
        message: 'CQ FT5ZM',
        freqHz: 1200,
        country: 'Amsterdam & St Paul Is.',
        directedToMe: false,
        newDxcc: true,
        newGrid: false,
        isCq: true,
      }) as unknown as DecodeRow

    processDecodes([atno()], both)
    expect(toasts).toHaveBeenCalledTimes(1)

    // Flood well past the old 2000 cap with ordinary CQ traffic.
    for (let i = 0; i < 3000; i++) {
      processDecodes(
        [
          {
            from: `N${i}AA`,
            message: `CQ N${i}AA`,
            freqHz: 1000 + (i % 900),
            directedToMe: false,
            newDxcc: false,
            newGrid: false,
            isCq: true,
          } as unknown as DecodeRow,
        ],
        both,
      )
    }
    toasts.mockClear()

    // The new one decodes again. It must stay silent — it is not a new one twice.
    processDecodes([atno()], both)
    expect(toasts).not.toHaveBeenCalled()
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// THE PHANTOM "IS CALLING YOU" (operator report, seen on 1.9.2 + 1.10.0, with a
// screenshot catching it live: KF0WWR's RR73 at 04:23:12, own 73 at :14, QSO
// Done — and at :29 the toast announces "KF0WWR is calling you").
//
// The mechanism is a STALE DECODE RE-QUALIFYING when the QSO state moves on.
// processDecodes runs on every snapshot over the ROLLING decode window, so a row
// is re-evaluated many times. Mid-QSO, the partner's RR73 (directed to me) is
// suppressed — deliberately without consuming a dedup key. The moment the state
// leaves the QSO (Done, or Listening after a band switch), the SAME old row
// re-evaluates with engaged=false, qualifies as "calling me", finds its key
// unconsumed, and toasts about an exchange that is already over. Same defect
// class as the stale-boundary TX incident: a decode outliving its moment
// replaying a decision.
// ─────────────────────────────────────────────────────────────────────────────
describe('a stale decode must not become a phantom "calling you"', () => {
  it('does not announce the RR73 tail of the QSO you just finished (the screenshot)', () => {
    const rr73 = decode({ from: 'KF0WWR', directedToMe: true, message: 'N5TAN KF0WWR RR73' })
    // Mid-QSO: the RR73 arrives while engaged → correctly silent.
    processDecodes([rr73], settings, undefined, { state: 'Report', dxcall: 'KF0WWR' })
    expect(toasts).not.toHaveBeenCalled()
    // The sequencer sends 73; the state machine reaches Done. The SAME row is
    // still in the window on the next snapshot.
    processDecodes([rr73], settings, undefined, { state: 'Done', dxcall: 'KF0WWR' })
    expect(
      toasts,
      'the finished QSO\'s own RR73 re-qualified as "calling you" after Done',
    ).not.toHaveBeenCalled()
  })

  it('does not replay an old caller after a band switch resets the state machine', () => {
    const old = decode({ from: 'W9STALE', directedToMe: true, message: 'N5TAN W9STALE EN52' })
    // A third station called while a QSO with someone else was running → silent by design.
    processDecodes([old], settings, undefined, { state: 'AwaitReport', dxcall: 'K1ABC' })
    expect(toasts).not.toHaveBeenCalled()
    // Band switch: state resets to Listening, no new decode yet — the stale row rides along.
    processDecodes([old], settings, undefined, { state: 'Listening', dxcall: null })
    expect(
      toasts,
      'a decode from before the band switch replayed as a fresh call',
    ).not.toHaveBeenCalled()
  })

  // The two behaviours the fix must NOT break:

  it('still alerts for the fresh decode that answers your CQ (#167)', () => {
    // The answering decode arrives on a snapshot where dxcall is ALREADY set to
    // the caller (the sequencer set it in the same ingest) — the #167 shape.
    processDecodes(
      [decode({ from: 'N0CALL', directedToMe: true, message: 'N5TAN N0CALL EN34' })],
      settings,
      undefined,
      { state: 'CallingCq', dxcall: 'N0CALL' },
    )
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('still alerts for a genuinely new caller arriving after the QSO is done', () => {
    processDecodes(
      [decode({ from: 'K5NEXT', directedToMe: true, message: 'N5TAN K5NEXT EM12' })],
      settings,
      undefined,
      { state: 'Done', dxcall: 'KF0WWR' },
    )
    expect(toasts).toHaveBeenCalledTimes(1)
  })
})

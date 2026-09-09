// #167 — "someone is calling you" never fired until AFTER the QSO was over.
//
// THE MECHANISM, and it is why the reporter saw the alert arrive at the wrong moment rather
// than not at all. `processDecodes` skipped every decode from the current QSO partner before
// `directedToMe` was ever consulted. The sequencer sets `qso.dxcall` in the SAME ingest that
// produces the decode (pinned backend-side: one ingest of "W9XYZ K1ABC FN42" leaves
// `qso.dxcall == K1ABC`), so the very decode that answers your CQ arrives on a snapshot where
// that station is ALREADY the partner — and was dropped. Their post-RR73 "73" fired only
// because the QSO had ended and dxcall was no longer theirs.
//
// WSJT-X BASELINE: `Highlight::MyCall` applies to EVERY decode whose clean string contains
// your callsign, with no exclusion for the station being worked (widgets/displaytext.cpp:490).
// dxCall drives a DIFFERENT highlight. On by default.
//
// The partner skip was added to stop chatty popups, and that job now belongs where it always
// really belonged: the per-decode dedup plus the `engaged` gate. Both are pinned below — a fix
// that reopened the chatter would be a regression, not a fix.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { __resetAlertsForTest, processDecodes } from './alerts'
import { pushToast } from './toast'
import type { DecodeRow, Settings } from './types'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
const toasts = vi.mocked(pushToast)

// Node test env has no window; alerts.ts needs setTimeout and degrades to silent without
// AudioContext.
vi.stubGlobal('window', { setTimeout } as unknown as Window & typeof globalThis)

const settings = { alertMyCall: true, alertNew: true, alertCq: false } as unknown as Settings

let seq = 0
function decode(over: Partial<DecodeRow>): DecodeRow {
  return {
    from: 'K1ABC',
    message: `msg-${seq++}`,
    freqHz: 1500 + seq,
    directedToMe: false,
    newDxcc: false,
    newGrid: false,
    isCq: false,
    ...over,
  } as unknown as DecodeRow
}

beforeEach(() => {
  toasts.mockClear()
  __resetAlertsForTest()
})

describe('a station answering your CQ alerts on the decode that answers it (#167)', () => {
  it('fires even though the sequencer already made that station the partner', () => {
    // The exact shape of the reporter's snapshot: still CallingCq (the exchange has not
    // started), and dxcall is ALREADY the answering station from the same ingest.
    processDecodes([decode({ from: 'W9XYZ', directedToMe: true })], settings, undefined, {
      state: 'CallingCq',
      dxcall: 'W9XYZ',
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('calling you')
  })

  it('fires for a partner set while merely listening too (no CQ run in progress)', () => {
    processDecodes([decode({ from: 'VK3ZZ', directedToMe: true })], settings, undefined, {
      state: 'Listening',
      dxcall: 'VK3ZZ',
    })
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  // ── The anti-chatter guarantees the partner skip was standing in for ──────────────────

  it('does NOT re-alert when the same call re-sends the same message', () => {
    const d = decode({ from: 'JA1QQ', directedToMe: true, message: 'KD9TAW JA1QQ PM95' })
    processDecodes([d], settings, undefined, { state: 'CallingCq', dxcall: 'JA1QQ' })
    expect(toasts).toHaveBeenCalledTimes(1)
    // Same decode, next cycle — and with the measured audio offset drifted, which is the
    // form the repeat actually takes on the air.
    processDecodes([{ ...d, freqHz: d.freqHz + 3 }], settings, undefined, {
      state: 'CallingCq',
      dxcall: 'JA1QQ',
    })
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('goes quiet for the rest of the exchange — on either path', () => {
    // ⚠️ REWRITTEN 2026-09-02 for the per-station contract. This used to walk one station
    // through BOTH vocabularies and expect silence throughout — which encoded the 1.10.1
    // defect: with Auto on, the answer lands on an AwaitRoger snapshot and was never
    // announced. A real QSO is on ONE path. Initiator path (they answered our CQ): the
    // first fresh partner decode announces, everything after is the same station.
    processDecodes([decode({ from: 'G4ZZZ', directedToMe: true })], settings, undefined, {
      state: 'AwaitRoger',
      dxcall: 'G4ZZZ',
    })
    expect(toasts).toHaveBeenCalledTimes(1)
    for (const state of ['Confirming', 'Done']) {
      processDecodes([decode({ from: 'G4ZZZ', directedToMe: true })], settings, undefined, {
        state,
        dxcall: 'G4ZZZ',
      })
    }
    expect(toasts).toHaveBeenCalledTimes(1)
    // Responder path (we called them): nothing they send back is a call.
    for (const state of ['AwaitReport', 'AwaitRr73', 'Confirming', 'AwaitExchange', 'AwaitConfirm']) {
      processDecodes([decode({ from: 'F6ZZZ', directedToMe: true })], settings, undefined, {
        state,
        dxcall: 'F6ZZZ',
      })
    }
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('still pops nothing ELSE about the station being worked — a partner row can only say "calling you"', () => {
    // A new DXCC we are already mid-QSO with is not news, and letting the mycall kind through
    // must not open the door to the rest of the ladder.
    processDecodes(
      [decode({ from: 'ZL9DX', directedToMe: true, newDxcc: true, country: 'Auckland Is.' })],
      settings,
      undefined,
      { state: 'AwaitRoger', dxcall: 'zl9dx' }, // case-insensitive, as before
    )
    // The one thing a partner row may say is "calling you" (they answered our CQ);
    // the new-DXCC tier never fires for the station being worked.
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('calling you')
    expect(toasts.mock.calls[0][0]).not.toContain('NEW DXCC')
  })

  it('does not let a watch-list entry fire for the station already being worked', () => {
    // The watch tier sits below the partner skip and pre-empts the generic ladder. Admitting
    // the mycall kind must not admit it: mid-QSO the partner stays silent on every channel.
    const watch = [{ id: 'w1', kind: 'call', value: 'PY2AA' }] as never
    processDecodes(
      [decode({ from: 'PY2AA', directedToMe: true })],
      settings,
      undefined,
      { state: 'Confirming', dxcall: 'PY2AA' },
      watch,
    )
    expect(toasts).not.toHaveBeenCalled()
  })
})

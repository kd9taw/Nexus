// @vitest-environment jsdom
//
// THE CLOCK CHIP TELLS THE TRUTH ABOUT A FEATURE THAT ALREADY SHIPPED.
//
// Nexus has corrected the PC-clock-vs-UTC offset inside its own radio loop since
// June 2026: `service.rs` subtracts the measured offset from the system clock so
// TX keys and decode windows land on the true UTC grid. The chip did not know
// that. It painted the raw offset red above one second and its tooltip told the
// operator to "sync via NTP / time.is" — an instruction to go fix a problem the
// app had already fixed, on a station that was working. That is how an operator
// learns to ignore a status chip.
//
// These tests pin the distinction the chip now has to make, and the sharpest of
// them is the negative one: OUTSIDE the too-far-out case, no clock string may
// tell the operator to go set their clock.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'
import { EN } from '../i18n'

afterEach(cleanup)

const base = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  rigMode: 'USB',
  rigConfirmed: true,
  nextSlotMs: 0,
  txEven: true,
  txCycleAuto: true,
  txEnabled: false,
  txAllowed: true,
  transmitting: false,
  tuning: false,
  qsoRecording: false,
  catOk: true,
  dtSec: 0,
  timeSyncOk: true,
  clockOffsetMs: null,
} as unknown as RadioStatus

function renderBar(clock: Record<string, unknown>) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN52xa"
      radio={{ ...base, ...clock } as RadioStatus}
      link={{ tier: 'FT8' } as never}
      bandPlan={[]}
      onSetFrequency={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onHaltTx={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onSetHoldTxFreq={noop}
      tier="FT8"
      onTierChange={noop}
      onOpenGuide={noop}
      field={false}
      onFieldChange={noop}
    />,
  )
}

/** The chip, found by its class rather than its text — the text is the subject. */
function chip(container: HTMLElement): HTMLElement {
  const el = container.querySelector('.timesync')
  expect(el, 'the clock chip renders').not.toBeNull()
  return el as HTMLElement
}

describe('the clock chip states', () => {
  it('says the offset is CORRECTED, and stays calm about it', () => {
    const { container } = renderBar({
      clockOffsetMs: 420,
      clockAgeSecs: 300,
      clockServers: 3,
    })
    const el = chip(container)
    expect(el.textContent).toContain('+0.42s')
    // Not red. A steered station's slot timing is right.
    expect(el.className).toContain('ok')
    expect(el.className).not.toContain('bad')
    const title = el.getAttribute('title') ?? ''
    expect(title).toContain('already correcting')
    expect(title).toContain('5 min')
    expect(title).toContain('3')
  })

  it('goes amber for a large CORRECTED offset, and says why — the log, not the decoding', () => {
    const { container } = renderBar({
      clockOffsetMs: 2_400,
      clockAgeSecs: 60,
      clockServers: 2,
    })
    const el = chip(container)
    expect(el.className).toContain('warn')
    // Still not "bad": the decoding is fine. What is not fine is that
    // `tempo_core::logbook` stamps QSOs from the raw system clock.
    expect(el.className).not.toContain('bad')
    expect(el.getAttribute('title')).toContain(EN['topbar.clock.logNote'])
  })

  it('says the correction has EXPIRED rather than going quiet', () => {
    // THE B2 SYMPTOM. Off-grid long enough for the hold window to end: the
    // backend sends no offset but keeps the age, so the chip can say so. Before
    // 2026-09 the correction just stopped, with nothing on screen changing.
    const { container } = renderBar({ clockOffsetMs: null, clockAgeSecs: 7_200 })
    const el = chip(container)
    expect(el.textContent).toContain('unchecked')
    expect(el.className).toContain('warn')
    expect(el.getAttribute('title')).toContain('aged out')
  })

  it('says the clock is TOO FAR OUT to correct, and only then asks for a fix', () => {
    const { container } = renderBar({ clockOffsetMs: null, clockGrossMs: 91_000 })
    const el = chip(container)
    expect(el.textContent).toContain('+91.00s')
    expect(el.className).toContain('bad')
    const title = el.getAttribute('title') ?? ''
    expect(title).toContain('too far for Nexus to correct')
    expect(title).toContain('Set the clock on the machine')
  })

  it('falls back to the decode-derived verdict when nothing has been measured', () => {
    const { container } = renderBar({ clockOffsetMs: null, timeSyncOk: true })
    expect(chip(container).textContent).toContain(EN['topbar.sync.ok.label'])
  })

  it('says WHO owns the clock, in every state', () => {
    // ⚠️ GUARD 8's operator-facing half. On a station running NetTime or
    // Meinberg, Nexus deliberately touches nothing — and without this the chip
    // is a wall of numbers with no way to tell that nothing here is the
    // operator's to fix. It rides every state, not just the alarming ones.
    const note = 'NetTime is managing this clock — Nexus is leaving it alone'
    const states = [
      { clockOffsetMs: 420, clockAgeSecs: 60, clockServers: 3 },
      { clockOffsetMs: null, clockAgeSecs: 9_000 },
      { clockOffsetMs: null, clockGrossMs: 91_000 },
      { clockOffsetMs: null, timeSyncOk: false },
    ]
    for (const state of states) {
      const { container, unmount } = renderBar({ ...state, clockOwnerNote: note })
      expect(chip(container).getAttribute('title'), JSON.stringify(state)).toContain(note)
      unmount()
    }
  })

  it('says nothing extra before a detection pass has run', () => {
    // The control: an empty note must not leave an empty bracket on the tooltip.
    const { container } = renderBar({ clockOffsetMs: 420, clockAgeSecs: 60, clockOwnerNote: '' })
    expect(chip(container).getAttribute('title')).not.toContain('()')
  })

  it('the gross state wins over a stale offset', () => {
    // A clock now known to be 91 s out must not be reported through yesterday's
    // smaller number — the backend drops the held offset for exactly this
    // reason, and the chip must not resurrect it.
    const { container } = renderBar({
      clockOffsetMs: null,
      clockAgeSecs: 7_200,
      clockGrossMs: 91_000,
    })
    expect(chip(container).className).toContain('bad')
  })
})

describe('no clock string sends the operator to fix a working station', () => {
  // ⚠️ THE GUARD WITH THE MOST VALUE HERE, and it is a property of the CATALOG,
  // not of one render: the shipped strings told operators to install or run
  // other time software. Exactly one state may still say that — the one where
  // Nexus refuses to steer — so every other clock string is checked against the
  // vocabulary that expresses it.
  const FIX_YOUR_CLOCK = ['time.is', 'sync via', 'sync your PC clock']

  const CORRECTING_KEYS = [
    'topbar.clock.corrected.label',
    'topbar.clock.corrected.title',
    'topbar.clock.stale.label',
    'topbar.clock.stale.title',
  ] as const

  it('the corrected and expired strings never do', () => {
    for (const key of CORRECTING_KEYS) {
      const s = EN[key]
      for (const phrase of FIX_YOUR_CLOCK) {
        expect(s.toLowerCase(), `${key} still tells the operator to sync the clock`).not.toContain(
          phrase.toLowerCase(),
        )
      }
    }
  })

  it('the corrected string says the correction is already applied', () => {
    // The positive half: proving the bad phrasing is absent is worthless if the
    // right phrasing is absent too — the string could simply have gone blank.
    expect(EN['topbar.clock.corrected.title']).toContain('already correcting')
  })

  it('and the too-far-out string DOES ask for a fix — the control', () => {
    // If this ever stops matching, the check above is passing because the
    // vocabulary no longer appears anywhere, not because it was moved.
    const gross = EN['topbar.clock.gross.title']
    expect(gross).toContain('Set the clock on the machine')
  })

  it('the settings hint describes a correction, not a chore', () => {
    const hint = EN['settings.digital.clockCheck.hint']
    expect(hint.toLowerCase()).not.toContain('time.is')
    expect(hint).toContain('CORRECT')
  })
})

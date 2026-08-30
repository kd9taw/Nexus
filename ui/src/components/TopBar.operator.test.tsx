// @vitest-environment jsdom
//
// Who is at the key (#25). Two operators swapping the mic every few contacts cannot be asked to
// open Settings, and a wrong operator is SILENT — nothing misbehaves, and it is discovered at
// submission when the log is already wrong. So the indicator must be always visible and must say
// who without being asked.
//
// The paired risk is the opposite: for the single-op station that is nearly everyone, this must
// cost nothing at all. Both directions are tested.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'


beforeAll(() => {
  // Radix DropdownMenu measures and captures pointers; jsdom has neither.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})

afterEach(cleanup)

const radio = {
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
  clockOffsetMs: 0,
} as unknown as RadioStatus

function renderBar(onOpenGuide: () => void, over: Record<string, unknown> = {}) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN52xa"
      radio={radio}
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
      onOpenGuide={onOpenGuide}
      {...over}
    />,
  )
}

describe('the operator at the key', () => {
  it('is not shown at all for a single-op station', () => {
    renderBar(() => {}, { operator: '' })
    expect(
      screen.queryByTitle(/who is at the key/i),
      'an empty operator is the single-op case and must cost no width',
    ).toBeNull()
  })

  it('shows who is operating, without being asked', () => {
    renderBar(() => {}, { operator: 'W1ABC' })
    const chip = screen.getByTitle(/who is at the key/i)
    expect(chip.textContent).toMatch(/W1ABC/)
  })

  it('swaps seats from the roster in one click', () => {
    const onSetOperator = vi.fn()
    renderBar(() => {}, {
      operator: 'W1ABC',
      operatorRoster: ['W1ABC', 'G0PQR'],
      onSetOperator,
    })
    // Radix opens on pointerdown, not click — same gesture the Help menu test uses.
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    fireEvent.click(screen.getByText(/Switch to G0PQR/))
    expect(onSetOperator).toHaveBeenCalledWith('G0PQR')
  })

  it('does not offer to switch to whoever is already operating', () => {
    renderBar(() => {}, {
      operator: 'W1ABC',
      operatorRoster: ['W1ABC', 'G0PQR'],
      onSetOperator: vi.fn(),
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    expect(screen.queryByText(/Switch to W1ABC/)).toBeNull()
  })

  it('can go back to single-op, because that is how an activation ends', () => {
    const onSetOperator = vi.fn()
    renderBar(() => {}, { operator: 'W1ABC', operatorRoster: [], onSetOperator })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    fireEvent.click(screen.getByText(/Single operator/))
    expect(onSetOperator).toHaveBeenCalledWith('')
  })
})

// ── the seat swap has to be REACHABLE, and at a club event that starts from nothing ──────
//
// THE REPORT (operator, 2026-08-30, running a club Field Day): "when people are swapping out,
// there needs to be a button to swap out the operator easily across any mode".
//
// THE CAUSE, and it is the chip's own gate above. The control already existed and was already
// app-wide — but `{operator && operator.trim() !== '' && (…)}` meant it only appeared once
// somebody had ALREADY set an operator, and the only place to do that first set is the Field
// Day dashboard (or Settings). So at the one station that needs it, the seat-swap button was
// invisible until someone found the screen it was supposed to save them from, and every
// operator sitting down in Phone, CW, RTTY or Operate had no way to say who they were.
//
// THE FIX, and its shape matters as much as its existence: the gate widens by exactly one
// case — Field Day is on — and NOT to "always". For the single-op station that is nearly
// everyone the chip must still cost no width at all, and that is the direction pinned first
// below, because it is the one that must not regress.
describe('setting the first operator, when Field Day is on', () => {
  it('still costs a non-Field-Day station nothing at all', () => {
    // The paired risk, pinned BEFORE the new behaviour: widening the gate must not have
    // widened it to everyone.
    renderBar(() => {}, { operator: '', fdActive: false })
    expect(
      screen.queryByTitle(/who is at the key/i),
      'the chip is back on every single-op station — this is the case the gate exists for',
    ).toBeNull()
  })

  it('offers itself before anyone has been set', () => {
    renderBar(() => {}, { operator: '', fdActive: true, onSetOperator: vi.fn() })
    const chip = screen.getByTitle(/who is at the key/i)
    expect(
      chip.textContent,
      'with Field Day on and no operator the chip must still be there, and must read as an ' +
        'invitation rather than as a callsign — this is the multi-op station before the ' +
        'first seat is claimed',
    ).toMatch(/Set operator/i)
  })

  it('claims a seat from the roster in one click, with nobody operating yet', () => {
    const onSetOperator = vi.fn()
    renderBar(() => {}, {
      operator: '',
      fdActive: true,
      operatorRoster: ['W1ABC', 'G0PQR'],
      onSetOperator,
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), { button: 0, ctrlKey: false })
    fireEvent.click(screen.getByText(/Switch to G0PQR/))
    expect(onSetOperator).toHaveBeenCalledWith('G0PQR')
  })

  it('does not offer to clear an operator that is not set', () => {
    // "Single operator (clear)" with nothing to clear is a menu row that does nothing.
    renderBar(() => {}, {
      operator: '',
      fdActive: true,
      operatorRoster: ['W1ABC'],
      onSetOperator: vi.fn(),
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), { button: 0, ctrlKey: false })
    expect(screen.queryByText(/Single operator/)).toBeNull()
  })

  it('says where the first one is typed when the log has never seen an operator', () => {
    // The empty-roster case is the FIRST event, and it is the one the report came from. The
    // roster is built from operators already in the log, so it is empty exactly then — and a
    // menu with nothing in it would be a worse dead end than no chip at all.
    renderBar(() => {}, { operator: '', fdActive: true, operatorRoster: [], onSetOperator: vi.fn() })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), { button: 0, ctrlKey: false })
    expect(
      screen.getByText(/Field Day dashboard/),
      'the menu is empty on a log with no operators in it yet — the chip has to say where ' +
        'the first one is typed instead of opening onto nothing',
    ).toBeTruthy()
  })

  it('says nothing of the sort once the roster has names in it', () => {
    // Positive control for the hint: it is the empty-roster fallback, not a permanent row.
    renderBar(() => {}, {
      operator: '',
      fdActive: true,
      operatorRoster: ['W1ABC'],
      onSetOperator: vi.fn(),
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), { button: 0, ctrlKey: false })
    expect(screen.queryByText(/Field Day dashboard/)).toBeNull()
  })

  it('keeps the narrow save — the chip never routes through the heavyweight settings write', () => {
    // A seat swap is mid-QSO by definition (#54): the read-modify-write of the whole settings
    // struct ends the contact it was made during. The chip's ONLY write path is the
    // `onSetOperator` prop, which App wires to the narrow `setFdOperator` call.
    const onSetOperator = vi.fn()
    renderBar(() => {}, {
      operator: '',
      fdActive: true,
      operatorRoster: ['W1ABC'],
      onSetOperator,
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), { button: 0, ctrlKey: false })
    fireEvent.click(screen.getByText(/Switch to W1ABC/))
    expect(onSetOperator).toHaveBeenCalledTimes(1)
    expect(onSetOperator).toHaveBeenCalledWith('W1ABC')
  })
})

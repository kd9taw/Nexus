// @vitest-environment jsdom
//
// THE AMPLIFIER BOX'S OPERATE/STANDBY BUTTON, by what it renders and what a click sends (operator,
// 2026-10-31: "red being ready to operate and green being on standby. Clearly label things active vs
// standby"). Its rules are the cockpit header strip's (AmpStrip.tsx), held in one hook both use
// (useAmpControl.ts); the colours' contrast in every theme is AmpPane.contrast.test.tsx, and that it is
// on no stop-line census is stop-line.test.tsx.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { AmpPane, resetOperateHold } from './AmpPane'
import { t } from '../../i18n'
import type { AmpStatus } from '../../types'
import { css, loadSheets, resolveVal } from '../../cssCascade.testkit'

// Derived from the real module (AmpStrip.test.tsx's rule): a partial mock leaves every other export
// undefined.
vi.mock('../../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../api')>()),
  ampCommand: vi.fn(async () => true),
}))
import { ampCommand } from '../../api'

beforeAll(() => loadSheets())
beforeEach(() => {
  vi.mocked(ampCommand).mockClear()
  // The press in flight is the window's, not a render's: no test inherits the last one's.
  resetOperateHold()
})
afterEach(cleanup)

const live = (over: Partial<AmpStatus> = {}): AmpStatus => ({
  family: 'spe',
  model: '15K',
  linked: true,
  reason: '',
  operate: true,
  transmitting: false,
  outputWatts: 0,
  swr: 1.1,
  swrAtu: 1.0,
  volts: 48.0,
  amps: 0,
  temp: 30,
  tempCelsius: false,
  alarm: 'none',
  alarmRaised: false,
  warning: 'none',
  warningRaised: false,
  kpaFault: null,
  ...over,
})

const button = () => document.querySelector<HTMLButtonElement>('.amp-opbtn')!

describe('the Amplifier box’s Operate/Standby button', () => {
  it('Operate is RED and says Active; Standby is GREEN and says Standby — in words and shape, not colour alone', () => {
    render(<AmpPane amp={live({ operate: true })} radioOnAir={false} />)
    const op = button()
    expect(op.getAttribute('aria-label')).toBe(`${t('amp.operate')} · ${t('amp.op.active')}`)
    expect(op.textContent).toContain(t('amp.op.active'))
    // Its colour is its class; which colour that class paints, in every theme, is AmpPane.contrast.test.tsx.
    expect(op.classList.contains('is-operate')).toBe(true)
    // The lamp, a shape a colour-blind operator reads without the red.
    expect(op.querySelectorAll('.amp-opbtn-icon circle').length).toBe(2)
    cleanup()

    render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    const sb = button()
    expect(sb.getAttribute('aria-label')).toBe(`${t('amp.standby')} · ${t('amp.op.notActive')}`)
    expect(sb.classList.contains('is-standby')).toBe(true)
    expect(sb.classList.contains('is-operate')).toBe(false)
    // The pause mark, a different shape from Operate's lamp.
    expect(sb.querySelectorAll('.amp-opbtn-icon rect').length).toBe(2)
    expect(sb.querySelectorAll('.amp-opbtn-icon circle').length).toBe(0)
  })

  it('its word is uppercase on screen and big — the title size, not the box’s micro text', () => {
    render(<AmpPane amp={live()} radioOnAir={false} />)
    expect(css(button().querySelector('.amp-opbtn-word')!, 'text-transform')).toBe('uppercase')
    expect(css(button(), 'font-size')).toBe(resolveVal('var(--fs-title)'))
    expect(css(button(), 'width')).toBe('100%')
  })

  it('a click sends OPERATE and moves NOTHING: only the next status frame does', async () => {
    render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    await act(async () => {
      fireEvent.click(button())
    })
    expect(vi.mocked(ampCommand)).toHaveBeenCalledTimes(1)
    expect(vi.mocked(ampCommand)).toHaveBeenCalledWith('operate')
    // OPERATE is a toggle with no idempotent set: an optimistic flip would lie on a lost frame.
    expect(button().getAttribute('aria-label')).toBe(`${t('amp.standby')} · ${t('amp.op.notActive')}`)
    expect(button().classList.contains('is-standby')).toBe(true)
  })

  it('…and the next frame is what turns it red', () => {
    const r = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    r.rerender(<AmpPane amp={live({ operate: true })} radioOnAir={false} />)
    expect(button().classList.contains('is-operate')).toBe(true)
  })

  it('⛔ is dead while the AMPLIFIER reports keyed, and sends nothing', () => {
    render(<AmpPane amp={live({ transmitting: true })} radioOnAir={false} />)
    expect(button().disabled).toBe(true)
    expect(button().title).toContain(t('amp.strip.keyed.title'))
    fireEvent.click(button())
    expect(vi.mocked(ampCommand)).not.toHaveBeenCalled()
  })

  it('⛔ is dead while the RADIO is on air, even with an amplifier that reports idle or no flag', () => {
    for (const transmitting of [false, null]) {
      render(<AmpPane amp={live({ transmitting })} radioOnAir />)
      expect(button().disabled, `amplifier flag ${transmitting}`).toBe(true)
      cleanup()
    }
    // The control: the same amplifier is usable with the radio off the air, so this is the lock and
    // not a button that is always dead.
    render(<AmpPane amp={live({ transmitting: null })} radioOnAir={false} />)
    expect(button().disabled).toBe(false)
  })

  it('a configured amplifier that is not answering draws "—", dead, with the reason in words', () => {
    render(<AmpPane amp={live({ linked: false, reason: 'portBusy', operate: true })} radioOnAir={false} />)
    const b = button()
    expect(b.disabled).toBe(true)
    expect(b.querySelector('.amp-opbtn-word')!.textContent).toBe('—')
    expect(b.querySelector('.amp-opbtn-sub')!.textContent).toBe(t('amp.strip.statusUnknown'))
    // The reason is named in words: on the box's head line above (AmpPane.test.tsx), in the button's
    // spoken name and in its title.
    expect(b.getAttribute('aria-label')).toBe(`${t('amp.strip.statusUnknown')} · ${t('amp.reason.portBusy')}`)
    expect(b.title).toContain(t('amp.reason.portBusy'))
    // Neither colour: a stale Operate from a dead link must not paint the button red.
    expect(b.classList.contains('is-operate') || b.classList.contains('is-standby')).toBe(false)
  })

  it('a linked amplifier with no Operate state is dead too — toggling a state we cannot see is toggling blind', () => {
    render(<AmpPane amp={live({ operate: null })} radioOnAir={false} />)
    expect(button().disabled).toBe(true)
    expect(button().getAttribute('aria-label')).toBe(t('amp.strip.statusUnknown'))
  })

  it('says so when a keystroke is refused, and clears it on the next one that goes through', async () => {
    vi.mocked(ampCommand).mockResolvedValueOnce(false)
    render(<AmpPane amp={live()} radioOnAir={false} />)
    await act(async () => {
      fireEvent.click(button())
    })
    expect(screen.getByRole('status').textContent).toBe(t('amp.strip.refused'))
    // A refused press holds nothing: the button is live for the retry.
    expect(button().disabled).toBe(false)
    await act(async () => {
      fireEvent.click(button())
    })
    expect(screen.queryByText(t('amp.strip.refused'))).toBeNull()
  })

  it('a queued press holds the button — "Sent, waiting" — so a second click cannot toggle it back', async () => {
    render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    await act(async () => {
      fireEvent.click(button())
    })
    expect(button().disabled).toBe(true)
    expect(screen.getByRole('status').textContent).toBe(t('amp.op.waiting'))
    await act(async () => {
      fireEvent.click(button())
    })
    expect(vi.mocked(ampCommand)).toHaveBeenCalledTimes(1)
    // Waiting is about OUR keystroke: the button still says what the amplifier last reported.
    expect(button().classList.contains('is-standby')).toBe(true)
    expect(button().getAttribute('aria-label')).toBe(`${t('amp.standby')} · ${t('amp.op.notActive')}`)
  })

  it('…until the amplifier reports the new state — not on a frame that repeats the old one', async () => {
    const r = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    await act(async () => {
      fireEvent.click(button())
    })
    r.rerender(<AmpPane amp={live({ operate: false, volts: 47.9 })} radioOnAir={false} />)
    expect(button().disabled, 'a frame that still says Standby ended the wait').toBe(true)
    r.rerender(<AmpPane amp={live({ operate: true })} radioOnAir={false} />)
    expect(button().disabled).toBe(false)
    expect(button().classList.contains('is-operate')).toBe(true)
    expect(screen.queryByText(t('amp.op.waiting'))).toBeNull()
  })

  it('…or until it gives up: a press dropped by the transmit interlock never changes the state', async () => {
    vi.useFakeTimers()
    try {
      render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      await act(async () => {
        fireEvent.click(button())
      })
      expect(button().disabled).toBe(true)
      await act(async () => {
        vi.advanceTimersByTime(4900)
      })
      expect(button().disabled, 'gave up early').toBe(true)
      await act(async () => {
        vi.advanceTimersByTime(200)
      })
      expect(button().disabled).toBe(false)
      expect(screen.queryByText(t('amp.op.waiting'))).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('the hold starts at the CLICK: a double click (two events, before the press is even queued) sends one press', async () => {
    // A browser dispatches the two clicks of a double click as separate events, and the second can land
    // while the call that queues the first is still out. Held only once that call answered, the button
    // would take both, and two toggles put the amplifier back where it was.
    render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    fireEvent.click(button())
    fireEvent.click(button())
    await act(async () => {})
    expect(vi.mocked(ampCommand)).toHaveBeenCalledTimes(1)
  })

  it('…held while the call is still out, and a press that then could not be queued says "Not sent"', async () => {
    vi.useFakeTimers()
    try {
      let answer!: (queued: boolean) => void
      vi.mocked(ampCommand).mockImplementationOnce(() => new Promise<boolean>((res) => (answer = res)))
      render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      await act(async () => {
        fireEvent.click(button())
      })
      // The call has not answered: already held, already saying so.
      expect(button().disabled).toBe(true)
      expect(screen.getByRole('status').textContent).toBe(t('amp.op.waiting'))
      // A call that hangs past the wait does not wedge the button.
      await act(async () => {
        vi.advanceTimersByTime(5000)
      })
      expect(button().disabled).toBe(false)
      await act(async () => {
        answer(false)
      })
      expect(screen.getByRole('status').textContent).toBe(t('amp.strip.refused'))
      expect(button().disabled).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('the hold survives a remount: switching screens right after a click does not hand back a live button', async () => {
    vi.useFakeTimers()
    try {
      const first = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      await act(async () => {
        fireEvent.click(button())
      })
      await act(async () => {
        vi.advanceTimersByTime(3000)
      })
      first.unmount()
      // Back on a screen with the box: still held, still saying so, and a click sends nothing.
      render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      expect(button().disabled).toBe(true)
      expect(screen.getByRole('status').textContent).toBe(t('amp.op.waiting'))
      fireEvent.click(button())
      expect(vi.mocked(ampCommand)).toHaveBeenCalledTimes(1)
      // The ORIGINAL deadline, not a fresh five seconds from the remount.
      await act(async () => {
        vi.advanceTimersByTime(1900)
      })
      expect(button().disabled, 'gave up early after the remount').toBe(true)
      await act(async () => {
        vi.advanceTimersByTime(200)
      })
      expect(button().disabled).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('…and a hold that ran out while no box was on screen is over when one comes back', async () => {
    vi.useFakeTimers()
    try {
      const first = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      await act(async () => {
        fireEvent.click(button())
      })
      // Off the box's screen past the deadline: no box was mounted to run the timer.
      first.unmount()
      vi.advanceTimersByTime(6000)
      render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
      expect(button().disabled).toBe(false)
      expect(screen.queryByText(t('amp.op.waiting'))).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('…and every box in the window for the same amplifier holds together; the new state frees them all', async () => {
    const a = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    const b = render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    const buttons = () => [...document.querySelectorAll<HTMLButtonElement>('.amp-opbtn')]
    await act(async () => {
      fireEvent.click(buttons()[0])
    })
    expect(buttons().map((x) => x.disabled)).toEqual([true, true])
    a.rerender(<AmpPane amp={live({ operate: true })} radioOnAir={false} />)
    b.rerender(<AmpPane amp={live({ operate: true })} radioOnAir={false} />)
    expect(buttons().map((x) => x.disabled)).toEqual([false, false])
  })

  it('…but a press to one amplifier does not hold another', async () => {
    render(<AmpPane amp={live({ operate: false })} radioOnAir={false} />)
    await act(async () => {
      fireEvent.click(button())
    })
    cleanup()
    render(<AmpPane amp={live({ operate: false, family: 'kpa', model: '' })} radioOnAir={false} />)
    expect(button().disabled).toBe(false)
  })

  it('every title says Standby does NOT stop a transmission — in every state', () => {
    const states: Array<[string, AmpStatus, boolean]> = [
      ['operate', live({ operate: true }), false],
      ['standby', live({ operate: false }), false],
      ['keyed', live({ operate: true }), true],
      ['unlinked', live({ linked: false, reason: 'noAnswer' }), false],
      ['unknown', live({ operate: null }), false],
    ]
    for (const [name, amp, onAir] of states) {
      render(<AmpPane amp={amp} radioOnAir={onAir} />)
      expect(button().title, name).toMatch(/does NOT stop a transmission/)
      cleanup()
    }
  })
})

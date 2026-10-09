// @vitest-environment jsdom
//
// #384: THE MOUSE WHEEL ON THE PHONE COCKPIT'S SLIDERS. KR4FQG, 2026-09-28, after the 1.15.0 Phone
// panel work: "could the sliders also change with a mouse scroll?" Every slider the cockpit draws
// takes the wheel held over it and moves as a drag of it would, through its own handler.
//
// ONE NOTCH: 2 % on a 0–100 % level (AF, RF, squelch, NR, mic, speech processor, monitor, and the
// Sub receiver's three), 1 % on RF power, 10 Hz on the manual notch, 0.5 dB on the Icom scope
// reference and 5 dB on the Flex one. The scope's own G and Z are PhoneScope's
// (`PhoneScope.wheel.test.tsx`), and what a notch means in general is `WheelRange.test.tsx`.
//
// ⛔ A WHEEL NEVER TRANSMITS. It moves a level the way a drag does, power included, and nothing
// here keys, tunes, arms or stops the transmitter: the last describe holds that for every slider.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CockpitHeader } from './CockpitHeader'
import { SubReceiverStrip } from './SubReceiverStrip'
import type { AppSnapshot, ReceiversStatus, ReceiverStatus } from '../types'
import { t } from '../i18n'
import { WHEEL_REST_MS } from './WheelRange'
import {
  atuTune,
  haltTx,
  playVoiceMessage,
  setAfGain,
  setCompLevel,
  setFlexPanRef,
  setMicGain,
  setMonitorGain,
  setNotchFreq,
  setNrLevel,
  setPtt,
  setRfGain,
  setRfPower,
  setScopeRef,
  setSquelch,
  setSubLevel,
  setTune,
  setTxEnabled,
  stopVoice,
} from '../api'

// THE BUDGET (2026-10-09). The slowest case here, "AF gain", takes 0.31 s and 0.29 s on one core (two runs); a loaded
// full suite on this box has run cases up to 20 times slower than one core, past vitest's 5 s default. 15 s is the
// house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (original) => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = {
    getLicensedBandPlan: [],
    getBandPlan: [],
    getCatCwUnprovenRigModels: [],
    getMeters: { rxLevel: 0, smeterDb: null, cwToneHz: null },
  }
  return Object.fromEntries(
    Object.entries(actual).map(([name, value]) => [
      name,
      typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value,
    ]),
  )
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The scope reports which native feed is streaming, which is what draws the rig-scope pane.
const mocks = vi.hoisted(() => ({ scopeFeed: null as string | null, scopeWheelSliders: undefined as boolean | undefined }))
vi.mock('./PhoneScope', async () => {
  const { useEffect } = await import('react')
  return {
    PhoneScope: ({ onFeed, wheelSliders }: { onFeed?: (source: string, loHz: number, hiHz: number) => void; wheelSliders?: boolean }) => {
      mocks.scopeWheelSliders = wheelSliders
      useEffect(() => {
        if (mocks.scopeFeed) onFeed?.(mocks.scopeFeed, -25_000, 25_000)
        // Once, as a feed is reported: `onFeed` is a new closure on every cockpit render.
        // eslint-disable-next-line react-hooks/exhaustive-deps
      }, [])
      return <div data-testid="scope-stub" />
    },
  }
})
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.clearAllMocks()
  mocks.scopeFeed = null
  mocks.scopeWheelSliders = undefined
})

/** Every Main level reported, each away from its control's seed and from either end of its range,
 *  so a notch either way is a real move. */
const RIG = {
  dialMhz: 14.2,
  band: '20m',
  sideband: 'USB',
  catOk: true,
  rigMode: 'USB',
  transmitting: false,
  txEnabled: false,
  txAllowed: true,
  slot: 0,
  nextSlotMs: 0,
  filterWidthHz: 2400,
  rfPower: 0.5,
  monitorGain: 0.4,
  rfGain: 0.8,
  afGain: 0.5,
  squelch: 0.1,
  micGain: 0.5,
  nrLevel: 0.3,
  notchFreqHz: 1500,
  compLevel: 0.35,
  agc: 'fast',
  nb: false,
  nr: false,
  notch: false,
  manualNotch: false,
  comp: true,
  vox: false,
}
/** An IC-7610's Sub, commandable, with the three levels the radio accepted. */
const SUB: ReceiverStatus = {
  id: 'sub',
  stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' },
  rfGain: 0.6,
  afGain: 0.5,
  squelch: 0.2,
} as ReceiverStatus
const DUAL: ReceiversStatus = {
  main: { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' }, dialMhz: 14.2 },
  sub: SUB,
  subCapability: 'present',
  subCommandable: true,
}

function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio,
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}
const mount = (radio: Record<string, unknown> = RIG) => render(<PhoneCockpit snap={snapWith(radio)} theme="dark" />)
const slider = (key: Parameters<typeof t>[0]) => screen.getByRole('slider', { name: t(key) }) as HTMLInputElement
/** One mouse notch, Chromium's 100 px. Negative is up. */
const notch = (el: Element, dir: 'up' | 'down' = 'up') =>
  fireEvent.wheel(el, { deltaY: dir === 'up' ? -100 : 100, deltaMode: 0 })

describe('a notch over a Phone slider moves it one step, through its own control', () => {
  it.each([
    ['AF gain', 'phone.analog.af.aria', setAfGain, '52', 0.52],
    ['RF gain', 'phone.analog.rf.aria', setRfGain, '82', 0.82],
    ['squelch', 'phone.analog.sql.aria', setSquelch, '12', 0.12],
    ['noise reduction', 'phone.rxDsp.nr.aria', setNrLevel, '32', 0.32],
    ['mic gain', 'phone.mic.aria', setMicGain, '52', 0.52],
    ['speech processor', 'phone.rxDsp.comp.aria', setCompLevel, '37', 0.37],
    ['monitor', 'phone.chain.mon.aria', setMonitorGain, '42', 0.42],
    ['manual notch', 'phone.rxDsp.notchFreq.aria', setNotchFreq, '1510', 1510],
    ['RF power', 'phone.header.power.label', setRfPower, '51', 0.51],
  ] as const)('%s', (_name, key, command, shown, sent) => {
    mount()
    const el = slider(key)
    expect(notch(el), 'the notch scrolled the pane instead').toBe(false)
    expect(slider(key).value).toBe(shown)
    expect(command).toHaveBeenCalledWith(sent)
  })

  it.each([
    ['RF', 'phone.sub.rf.aria', 'rf', '62', 0.62],
    ['AF', 'phone.sub.af.aria', 'af', '52', 0.52],
    ['squelch', 'phone.sub.sql.aria', 'sql', '22', 0.22],
  ] as const)('the Sub receiver’s %s', (_name, key, level, shown, sent) => {
    mount({ ...RIG, receivers: DUAL })
    notch(slider(key))
    expect(slider(key).value).toBe(shown)
    expect(setSubLevel).toHaveBeenCalledWith(level, sent)
  })

  it('the Icom scope reference, half a decibel', async () => {
    mocks.scopeFeed = 'civ'
    mount()
    notch(await screen.findByRole('slider', { name: t('phone.rigScope.ref.aria') }))
    expect(slider('phone.rigScope.ref.aria').value).toBe('5')
    expect(setScopeRef).toHaveBeenCalledWith(5)
  })

  it('the scope’s G and Z: the cockpit hands its scope the wheel', () => {
    mount()
    expect(mocks.scopeWheelSliders, 'Phone did not opt its scope’s G and Z into the wheel').toBe(true)
  })

  it('the Flex panadapter reference, five decibels', async () => {
    mocks.scopeFeed = 'flex'
    mount()
    notch(await screen.findByRole('slider', { name: t('phone.flexPan.ref.aria') }), 'down')
    expect(slider('phone.flexPan.ref.aria').value).toBe('-85')
    expect(setFlexPanRef).toHaveBeenCalledWith(-85)
  })
})

// A drag raises the slider's `…Dragging` flag so the rig's read-back cannot pull it back
// mid-gesture; a wheel burst is a drag, so it raises the same flag until the wheel rests.
describe('a burst of notches holds off the rig’s read-back the way a drag does', () => {
  it('keeps the slider where the wheel put it while an older reading arrives', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
    const view = mount()
    for (let i = 0; i < 3; i++) notch(slider('phone.analog.af.aria'))
    expect(slider('phone.analog.af.aria').value).toBe('56')
    // The poll brings the rig's reading from partway through the burst.
    view.rerender(<PhoneCockpit snap={snapWith({ ...RIG, afGain: 0.52 })} theme="dark" />)
    expect(slider('phone.analog.af.aria').value, 'the read-back pulled the slider back mid-burst').toBe('56')
    act(() => vi.advanceTimersByTime(WHEEL_REST_MS))
    // Once the wheel rests, the rig's reading is the slider's again, as after a drag.
    view.rerender(<PhoneCockpit snap={snapWith({ ...RIG, afGain: 0.6 })} theme="dark" />)
    expect(slider('phone.analog.af.aria').value).toBe('60')
  })
})

// ONE HOST'S CHANGE: the header's power slider and the Sub strip are shared (the header with every
// cockpit, the Sub strip with CW), and only a host that passes a wheel step gets the wheel.
describe('the shared sliders take the wheel only where their host asks', () => {
  it('a cockpit’s header power slider with no wheel step (every cockpit but Phone)', () => {
    const onChange = vi.fn()
    render(<CockpitHeader snap={snapWith(RIG)} modeIndicator={null} bandControl={null} power={{ value: 0.5, unit: 'drive', onChange, label: 'Drive' }} />)
    expect(notch(screen.getByRole('slider', { name: 'Drive' })), 'the drive slider stopped the scroll').toBe(true)
    expect(onChange).not.toHaveBeenCalled()
  })

  it('the Sub strip with no wheel step (CW’s)', () => {
    render(<SubReceiverStrip radio={snapWith({ ...RIG, receivers: DUAL }).radio} radioId={1} catOk />)
    expect(notch(slider('phone.sub.af.aria')), 'the Sub row stopped the scroll').toBe(true)
    expect(slider('phone.sub.af.aria').value).toBe('50')
    expect(setSubLevel).not.toHaveBeenCalled()
  })
})

describe('⛔ a wheel never transmits', () => {
  it('moves every slider both ways and never keys, tunes, arms or stops the transmitter', () => {
    mocks.scopeFeed = 'civ'
    mount({ ...RIG, receivers: DUAL })
    const sliders = screen.getAllByRole('slider') as HTMLInputElement[]
    // Fixture guard: the nine Main controls, the Sub's three and the scope reference.
    expect(sliders.length, 'fixture: not every Phone slider is drawn').toBeGreaterThanOrEqual(13)
    for (const el of sliders) {
      notch(el, 'up')
      notch(el, 'down')
    }
    const transmit = { setPtt, setTune, atuTune, setTxEnabled, haltTx, playVoiceMessage, stopVoice }
    for (const [name, fn] of Object.entries(transmit)) {
      expect(fn, `a wheel reached ${name}`).not.toHaveBeenCalled()
    }
  })
})

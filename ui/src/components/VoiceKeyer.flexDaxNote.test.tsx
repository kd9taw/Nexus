// @vitest-environment jsdom
//
// THE FLEX DAX NOTE. A FlexRadio driven over SmartSDR CAT or Hamlib takes its SSB transmit audio from
// its own mic unless DAX is on for voice in SmartSDR's P/CW panel, so a voice message keys the radio and
// sends no RF. The keyer says so once, after the first message the engine takes, until the operator
// dismisses it. It never holds a message back: the message is sent first, the note follows.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer, type VoiceKeyerForEnter } from './VoiceKeyer'
import { FLEX_DAX_NOTE_KEY, flexVoiceNeedsDax } from '../features/flexDaxNote'
import settings from './__fixtures__/defaultSettings.json'
import type { Settings } from '../types'

const { playVoiceMessage } = vi.hoisted(() => ({
  playVoiceMessage: vi.fn(async (): Promise<unknown> => ({})),
}))

vi.mock('../api', () => ({
  getVoiceMessages: vi.fn(async () => [{ slot: 1, label: 'CQ', file: '/tmp/cq.wav' }]),
  playVoiceMessage,
  stopVoice: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))

beforeEach(() => {
  localStorage.clear()
  playVoiceMessage.mockImplementation(async () => ({}))
})
afterEach(() => {
  cleanup()
  playVoiceMessage.mockReset()
})

const note = (c: HTMLElement) => c.querySelector('.vk-flex-dax-note')

async function mount(flexDaxNote: boolean) {
  const view = render(
    <VoiceKeyer txEnabled keyed={false} voicePlaying={false} flexDaxNote={flexDaxNote} />,
  )
  await act(async () => {})
  return view
}

async function pressF1() {
  await act(async () => {
    fireEvent.keyDown(window, { key: 'F1' })
  })
}

describe('the Flex DAX note in the voice keyer', () => {
  it('shows after the first message on a Flex over CAT, and not again once dismissed', async () => {
    const view = await mount(true)
    expect(note(view.container), 'nothing is said before a message plays').toBeNull()
    await pressF1()
    expect(playVoiceMessage).toHaveBeenCalledWith(1)
    const shown = note(view.container)
    expect(shown, 'the first message brings the note').not.toBeNull()
    expect(shown!.textContent).toMatch(/DAX/)
    expect(shown!.textContent).toMatch(/P\/CW/)

    fireEvent.click(shown!.querySelector('button')!)
    expect(note(view.container), 'dismiss takes it away').toBeNull()
    expect(localStorage.getItem(FLEX_DAX_NOTE_KEY)).toBe('1')

    await pressF1()
    expect(playVoiceMessage).toHaveBeenCalledTimes(2)
    expect(note(view.container), 'a later message does not bring it back').toBeNull()

    cleanup()
    const again = await mount(true)
    await pressF1()
    expect(playVoiceMessage).toHaveBeenCalledTimes(3)
    expect(note(again.container), 'nor does a new session').toBeNull()
  })

  it('never shows for a radio that does not need it', async () => {
    const view = await mount(false)
    await pressF1()
    expect(playVoiceMessage).toHaveBeenCalledWith(1)
    expect(note(view.container)).toBeNull()
  })

  it('does not show for a message the engine refused', async () => {
    playVoiceMessage.mockImplementation(async () => {
      throw new Error('TX is off')
    })
    const view = await mount(true)
    await pressF1()
    expect(playVoiceMessage).toHaveBeenCalledWith(1)
    expect(note(view.container)).toBeNull()
  })

  it('shows after the first message Enter Sends Message plays from the contest strip', async () => {
    let keyer: VoiceKeyerForEnter | null = null
    const view = render(
      <VoiceKeyer
        txEnabled
        keyed={false}
        voicePlaying={false}
        flexDaxNote
        esm={{ lit: [], report: (k) => (keyer = k), onStop: () => {}, onPlay: () => {} }}
      />,
    )
    await act(async () => {})
    expect(keyer).not.toBeNull()
    let why: string | null = 'unset'
    await act(async () => {
      why = await keyer!.play(1)
    })
    expect(why).toBeNull()
    expect(note(view.container)).not.toBeNull()
  })
})

describe('which radio needs the note', () => {
  const base = settings as unknown as Settings
  const s = (over: Partial<Settings>) => ({ ...base, ...over })

  it('a Flex on SmartSDR CAT, over the network or its COM port, or on Hamlib’s SmartSDR backend', () => {
    expect(flexVoiceNeedsDax(s({ rigModel: 2036, rigConn: 'network', rigAddr: '127.0.0.1:5002' }))).toBe(true)
    expect(flexVoiceNeedsDax(s({ rigModel: 2036, rigConn: 'serial' }))).toBe(true)
    expect(flexVoiceNeedsDax(s({ rigModel: 23005, rigConn: 'network' }))).toBe(true)
    // The client opted in without the radio's address cannot serve it: SmartSDR CAT still does.
    expect(flexVoiceNeedsDax(s({ rigModel: 2036, rigConn: 'network', flexNativeCat: true, flexRadioIp: '' }))).toBe(true)
  })

  it('not Nexus’s own Flex client, which switches the radio to DAX itself', () => {
    expect(
      flexVoiceNeedsDax(s({ rigModel: 2036, rigConn: 'network', flexNativeCat: true, flexRadioIp: '192.168.1.50' })),
    ).toBe(false)
  })

  it('not TCI, and not any other radio', () => {
    expect(flexVoiceNeedsDax(s({ rigModel: 2036, rigConn: 'tci' }))).toBe(false)
    expect(flexVoiceNeedsDax(s({ rigModel: 7, rigConn: 'tci' }))).toBe(false)
    expect(flexVoiceNeedsDax(s({ rigModel: 3073, rigConn: 'serial' }))).toBe(false)
    expect(flexVoiceNeedsDax(s({ rigModel: 2048, rigConn: 'network' }))).toBe(false)
    expect(flexVoiceNeedsDax(base)).toBe(false)
  })
})

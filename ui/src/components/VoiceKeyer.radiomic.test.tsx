// @vitest-environment jsdom
//
// THE VOICE KEYER HAS NO REFUSAL OF ITS OWN FOR THE RADIO'S MIC. Until 2026-10-12 it refused while
// the radio had the mic (Nexus's own Flex client, native DAX audio on, Phone at the shack), since a
// recorded message goes out over DAX. The operator's ruling of that day, "Change it now", replaced
// the refusal with a switch around each message: the station asks the radio for DAX, keys once
// the radio reports it, and gives the mic back after the unkey. The engine refuses what the switch
// cannot carry, and a message the station then drops says why on the status lane. So the keyer
// plays by click and by F-key with nothing to say; the cockpit's own test runs it with the radio
// on its mic (PhoneCockpit.esm.test.tsx). The toast module is the real one.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'
import { subscribeToasts, dismissToast, type Toast } from '../toast'

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

let toasts: Toast[] = []
let unsub: () => void = () => {}

beforeEach(() => {
  unsub = subscribeToasts((t) => {
    toasts = t
  })
})
afterEach(() => {
  cleanup()
  for (const t of toasts) dismissToast(t.id)
  unsub()
  playVoiceMessage.mockReset()
})

describe('the voice keyer and the radio’s mic', () => {
  it('plays by click and by F-key, with nothing to say', async () => {
    const view = render(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    await act(async () => {})
    const slot = view.container.querySelector<HTMLButtonElement>('.vk-play')
    expect(slot, 'precondition: F1 holds a message').not.toBeNull()
    await act(async () => {
      fireEvent.click(slot!)
    })
    expect(playVoiceMessage).toHaveBeenCalledWith(1)
    await act(async () => {
      fireEvent.keyDown(window, { key: 'F1' })
    })
    expect(playVoiceMessage).toHaveBeenCalledTimes(2)
    expect(toasts).toEqual([])
  })
})

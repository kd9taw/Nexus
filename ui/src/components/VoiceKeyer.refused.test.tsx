// @vitest-environment jsdom
//
// A VOICE-KEYER MESSAGE THE ENGINE REFUSES SAYS WHY (the operator, 2026-09-30: "Same drop rule").
//
// `Engine::send_voice` ignored a send made outside the licence's privileges (or with TX off)
// without a word, so an F-key pressed on a frequency the operator may not use did nothing and
// said nothing. It now refuses with the reason, and the keyer's own "Could not play F1" toast
// carries it. The toast module is the real one: what is asserted is the sentence the operator
// reads, not a call to a stub.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'
import { subscribeToasts, dismissToast, type Toast } from '../toast'

/** The engine's refusal, verbatim (`Engine::send_voice`). */
const REFUSAL = 'TX locked — this frequency is outside your license privileges'

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

/** Render the keyer with TX on, let its slot list load, and press F1's slot. */
async function playF1() {
  const { container } = render(<VoiceKeyer txEnabled keyed={false} transmitting={false} />)
  await act(async () => {})
  const slot = container.querySelector<HTMLButtonElement>('.vk-play')
  expect(slot, 'precondition: F1 holds a message').not.toBeNull()
  await act(async () => {
    fireEvent.click(slot!)
  })
  expect(playVoiceMessage).toHaveBeenCalledWith(1)
}

describe('a voice-keyer message the engine refuses', () => {
  it("shows the engine's reason in the keyer's toast", async () => {
    playVoiceMessage.mockRejectedValue(REFUSAL)
    await playF1()
    expect(toasts.map((t) => [t.kind, t.message])).toEqual([
      ['error', `Could not play F1: ${REFUSAL}`],
    ])
  })

  it('raises nothing when the engine takes the message', async () => {
    playVoiceMessage.mockResolvedValue({})
    await playF1()
    expect(toasts).toEqual([])
  })
})

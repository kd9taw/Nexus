// @vitest-environment jsdom
//
// THE VOICE KEYER CAN'T PLAY WHILE THE RADIO HAS THE MIC (operator ruling, 2026-10-04, "Refuse
// with a message"). With Nexus's own Flex client and native DAX audio on, Phone at the shack takes
// the radio's own mic while a recorded message goes out over DAX, so the radio would ignore the
// message and the mic would carry the over. A press says so in the operator's language and sends
// nothing, by click and by F-key; without that state the same press plays as before. The toast
// module is the real one: what is asserted is the sentence the operator reads.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'
import { subscribeToasts, dismissToast, type Toast } from '../toast'
import { DE } from '../i18n/de'
import { EN, installCatalog, setLocale } from '../i18n'

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
  setLocale('en')
  for (const t of toasts) dismissToast(t.id)
  unsub()
  playVoiceMessage.mockReset()
})

/** Render the keyer with TX on and let its slot list load. */
async function keyer(radioHasMic: boolean) {
  const view = render(
    <VoiceKeyer txEnabled keyed={false} transmitting={false} radioHasMic={radioHasMic} />,
  )
  await act(async () => {})
  const slot = view.container.querySelector<HTMLButtonElement>('.vk-play')
  expect(slot, 'precondition: F1 holds a message').not.toBeNull()
  return slot!
}

describe('the voice keyer while the radio has the mic', () => {
  it('sends nothing on a click and says why', async () => {
    const slot = await keyer(true)
    await act(async () => {
      fireEvent.click(slot)
    })
    expect(playVoiceMessage).not.toHaveBeenCalled()
    expect(toasts.map((t) => [t.kind, t.message])).toEqual([
      ['info', EN['phone.keyer.radioHasMic']],
    ])
  })

  it('sends nothing on its F-key either, once the radio has taken the mic', async () => {
    // Mounted before the radio took the mic: the F-key handler must see the change.
    const view = render(
      <VoiceKeyer txEnabled keyed={false} transmitting={false} radioHasMic={false} />,
    )
    await act(async () => {})
    view.rerender(<VoiceKeyer txEnabled keyed={false} transmitting={false} radioHasMic />)
    await act(async () => {
      fireEvent.keyDown(window, { key: 'F1' })
    })
    expect(playVoiceMessage).not.toHaveBeenCalled()
    expect(toasts.map((t) => t.message)).toEqual([EN['phone.keyer.radioHasMic']])
  })

  it('says it in the operator’s language', async () => {
    installCatalog('de', DE)
    setLocale('de')
    const slot = await keyer(true)
    await act(async () => {
      fireEvent.click(slot)
    })
    expect(DE['phone.keyer.radioHasMic']).toBeTruthy()
    expect(toasts.map((t) => t.message)).toEqual([DE['phone.keyer.radioHasMic']])
  })

  it('plays as before when the radio does not have the mic', async () => {
    const slot = await keyer(false)
    await act(async () => {
      fireEvent.click(slot)
    })
    expect(playVoiceMessage).toHaveBeenCalledWith(1)
    await act(async () => {
      fireEvent.keyDown(window, { key: 'F1' })
    })
    expect(playVoiceMessage).toHaveBeenCalledTimes(2)
    expect(toasts).toEqual([])
  })
})

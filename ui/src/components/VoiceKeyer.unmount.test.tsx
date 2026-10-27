// @vitest-environment jsdom
//
// THE VOICE KEYER LEAVING THE SCREEN STOPS ONLY A MESSAGE IT STARTED. On a computer driving a
// station, `stop_voice` is a Stop of everything there, so an unmount that sent it every time turned
// TX off at the station whenever the operator left Phone or hid the keyer. It now goes only while
// this pane has a message it started and has not seen end. The ■ Stop button and Esc are the
// operator's own Stop and always send it, played or not.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'

const { stopVoice, playVoiceMessage, pushToast } = vi.hoisted(() => ({
  stopVoice: vi.fn(async (): Promise<unknown> => ({})),
  playVoiceMessage: vi.fn(async (_slot: number): Promise<unknown> => ({})),
  pushToast: vi.fn(),
}))

// Only the keyer's own toasts are watched; a refused send still goes through the real
// withErrorToast, which answers it.
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../toast')>()),
  pushToast,
}))

vi.mock('../api', () => ({
  getVoiceMessages: vi.fn(async () => [{ slot: 1, label: 'CQ', file: '/tmp/cq.wav' }]),
  playVoiceMessage,
  stopVoice,
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  stopVoice.mockClear()
  playVoiceMessage.mockClear()
  pushToast.mockClear()
})

async function renderKeyer() {
  const view = render(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
  await act(async () => {})
  return view
}

async function playF1(container: HTMLElement) {
  const slot = container.querySelector<HTMLButtonElement>('.vk-play')
  expect(slot, 'precondition: F1 holds a message').not.toBeNull()
  await act(async () => {
    fireEvent.click(slot!)
  })
  expect(playVoiceMessage).toHaveBeenCalledWith(1)
}

describe('the voice keyer leaving the screen', () => {
  it('stops a message it started', async () => {
    const { container, unmount } = await renderKeyer()
    await playF1(container)
    expect(stopVoice).not.toHaveBeenCalled()
    unmount()
    expect(stopVoice).toHaveBeenCalledTimes(1)
  })

  it('sends nothing when it played nothing', async () => {
    const { unmount } = await renderKeyer()
    unmount()
    expect(stopVoice).not.toHaveBeenCalled()
  })

  it('sends nothing once the message it played has ended', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed voicePlaying />)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    unmount()
    expect(stopVoice).not.toHaveBeenCalled()
  })

  // The rig unkeying is not this pane's message ending unless the rig keyed AFTER the send.
  it('stops a message pressed while the last one was still on the air', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    // Pressed again before the first message ends: the next one waits for it.
    await playF1(container)
    expect(playVoiceMessage).toHaveBeenCalledTimes(2)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    // The first one's end, long enough ago to count, and the second has not keyed yet.
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    unmount()
    expect(stopVoice, 'the end of the message before it ended the guard of this one').toHaveBeenCalledTimes(1)
  })

  it('stops a message the rig keyed for again after the last one ended', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    await playF1(container)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    unmount()
    expect(stopVoice).toHaveBeenCalledTimes(1)
  })

  it('a refusal of an earlier press does not clear the message sent after it', async () => {
    const { container, unmount } = await renderKeyer()
    let refuse: (e: Error) => void = () => {}
    playVoiceMessage.mockImplementationOnce(
      () => new Promise((_, reject) => (refuse = reject)),
    )
    await playF1(container)
    await playF1(container)
    await act(async () => refuse(new Error('refused')))
    unmount()
    expect(stopVoice, "the first press's refusal cleared the second").toHaveBeenCalledTimes(1)
  })

  // One snapshot reading the rig unkeyed mid-message (a stale one after a link hiccup) is no end.
  it('stops a message after a snapshot briefly read the rig unkeyed', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    // Still keyed, long after the brief unkeyed reading.
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    unmount()
    expect(stopVoice, 'a brief unkeyed snapshot ended the guard').toHaveBeenCalledTimes(1)
  })

  it('stops a message hidden while a snapshot reads the rig unkeyed', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    unmount()
    expect(stopVoice, 'one unkeyed snapshot counted as the end').toHaveBeenCalledTimes(1)
  })
})

// `voicePlaying` is the station's own word that a voice message is queued or on the air (absent
// when false). A hide while it says so stops the message and says an over was cut; a station that
// never says it (an older build) shows no rise, so no message reads as ended and the hide stops it.
describe('the station saying a voice message is playing', () => {
  const said = () => pushToast.mock.calls.map((c) => String(c[0])).join(' | ')

  it('a hide while it plays stops it and says the over was stopped', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    unmount()
    expect(stopVoice).toHaveBeenCalledTimes(1)
    expect(said(), 'the voice over was cut in silence').toMatch(/F1/)
  })

  it('says nothing of an over once the message has ended', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying />)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={false} />)
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    unmount()
    expect(stopVoice).not.toHaveBeenCalled()
    expect(said()).toBe('')
  })

  it('a station that never says it gets its message stopped, and no over claimed', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    const now = Date.now()
    vi.spyOn(Date, 'now').mockReturnValue(now)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={undefined} />)
    vi.spyOn(Date, 'now').mockReturnValue(now + 60_000)
    rerender(<VoiceKeyer txEnabled keyed={false} voicePlaying={undefined} />)
    unmount()
    expect(stopVoice, 'a silent station read as the message ending').toHaveBeenCalledTimes(1)
    expect(said()).toBe('')
  })
})

describe("the voice keyer's own Stop", () => {
  it('■ Stop always sends it, played or not', async () => {
    const { container } = await renderKeyer()
    const stop = container.querySelector<HTMLButtonElement>('.vk-stop')
    expect(stop).not.toBeNull()
    fireEvent.click(stop!)
    expect(stopVoice).toHaveBeenCalledTimes(1)
    await playF1(container)
    fireEvent.click(stop!)
    expect(stopVoice).toHaveBeenCalledTimes(2)
  })

  it('Esc always sends it, played or not', async () => {
    const { container } = await renderKeyer()
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(stopVoice).toHaveBeenCalledTimes(1)
    await playF1(container)
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(stopVoice).toHaveBeenCalledTimes(2)
  })
})

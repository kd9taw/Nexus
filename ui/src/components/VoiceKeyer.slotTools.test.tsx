// @vitest-environment jsdom
//
// A VOICE-KEYER SLOT'S RECORD, IMPORT AND DELETE, AND DELETE'S UNDO. A contest tester wiped a
// recording twice: ● ⤓ ✕ sat right under the slot's ▶, so ✕ was easy to hit reaching for Play.
// The three now sit on the LEFT of the slot, away from Play, which stays where it was, and a
// Delete waits out an Undo window before the recording is really gone: the slot reads empty
// at once, the toast says "Deleted F1 (CQ)" with Undo, and only when the window passes does
// the backend clear the slot. Undo puts back the same recording under the same name.
//
// jsdom lays nothing out, so "left of Play" is pinned here as the order of the slot's children
// under the sheet's own flex direction (resolved from the real styles.css), and measured in a
// real browser separately.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { VoiceKeyer } from './VoiceKeyer'
import { Toasts } from './Toasts'
import { VOICE_UNDO_MS } from '../features/voiceDelete'
import type { VoiceMessage } from '../types'

const api = vi.hoisted(() => {
  const SLOTS: VoiceMessage[] = [
    { slot: 1, label: 'CQ', file: '/voice/slot1.wav' },
    { slot: 2, label: 'My Call', file: '/voice/slot2.wav' },
  ]
  return {
    SLOTS,
    getVoiceMessages: vi.fn(async () => SLOTS.map((m) => ({ ...m }))),
    // The backend's clear: the file goes, the label stays.
    clearVoiceMessage: vi.fn(async (slot: number) =>
      SLOTS.map((m) => (m.slot === slot ? { ...m, file: '' } : { ...m })),
    ),
    playVoiceMessage: vi.fn(async (_slot: number): Promise<unknown> => ({})),
    stopVoice: vi.fn(async () => ({})),
    startVoiceRecording: vi.fn(async () => ({})),
    stopVoiceRecording: vi.fn(async () => []),
    cancelVoiceRecording: vi.fn(async () => ({})),
    importVoiceMessage: vi.fn(async () => []),
  }
})

vi.mock('../api', () => ({
  getVoiceMessages: api.getVoiceMessages,
  clearVoiceMessage: api.clearVoiceMessage,
  playVoiceMessage: api.playVoiceMessage,
  stopVoice: api.stopVoice,
  startVoiceRecording: api.startVoiceRecording,
  stopVoiceRecording: api.stopVoiceRecording,
  cancelVoiceRecording: api.cancelVoiceRecording,
  importVoiceMessage: api.importVoiceMessage,
}))

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(async () => {
  // Drain any window a test left open, so the next test starts with nothing pending.
  await act(async () => {
    vi.advanceTimersByTime(VOICE_UNDO_MS * 2)
  })
  cleanup()
  vi.useRealTimers()
  for (const fn of Object.values(api)) if (typeof fn === 'function') fn.mockClear()
})

async function renderKeyer() {
  const view = render(
    <>
      <VoiceKeyer txEnabled keyed={false} voicePlaying={false} />
      <Toasts />
    </>,
  )
  await act(async () => {})
  return view
}

const slotOf = (n: number) =>
  [...document.querySelectorAll<HTMLElement>('.vk-slot')].find(
    (s) => s.querySelector('.vk-fkey')?.textContent === `F${n}`,
  )!
const deleteOf = (n: number) =>
  [...slotOf(n).querySelectorAll<HTMLButtonElement>('.vk-tool')].find(
    (b) => b.title === 'Clear this recording',
  )!
const playOf = (n: number) => slotOf(n).querySelector<HTMLButtonElement>('.vk-play')!
const undoButton = () => screen.queryByRole('button', { name: /^Undo/ })

async function press(key: string) {
  await act(async () => {
    fireEvent.keyDown(document.body, { key })
  })
}
async function wait(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms)
  })
}
const cleared = () => api.clearVoiceMessage.mock.calls.map((c) => c[0])
const played = () => api.playVoiceMessage.mock.calls.map((c) => c[0])

describe('a voice-keyer slot’s Record, Import and Delete', () => {
  it('sit on the LEFT of the slot, Play after them, in Record, Import, Delete order', async () => {
    // The real sheet, so the direction the slot lays its children out in is the shipped one.
    const style = document.createElement('style')
    style.textContent = readFileSync(resolve(__dirname, '../styles.css'), 'utf8')
    document.head.appendChild(style)
    try {
      await renderKeyer()
      for (const n of [1, 2]) {
        const slot = slotOf(n)
        expect(getComputedStyle(slot).flexDirection, `F${n} lays its children out in a row`).toBe(
          'row',
        )
        const kids = [...slot.children].filter((c) => !(c as HTMLElement).hidden)
        expect(
          kids.map((c) => c.className.split(' ')[0]),
          `F${n}: the tools come first, then Play`,
        ).toEqual(['vk-tools', 'vk-play'])
        const titles = [...slot.querySelectorAll<HTMLButtonElement>('.vk-tools .vk-tool')].map(
          (b) => b.title.split('.')[0],
        )
        expect(titles).toEqual(['Record F' + n, 'Import a ', 'Clear this recording'])
        // Accessible names unchanged: each tool is still its glyph, Play is still its slot.
        expect(
          [...slot.querySelectorAll('.vk-tools .vk-tool')].map((b) => b.textContent),
        ).toEqual(['●', '⤓', '✕'])
      }
    } finally {
      style.remove()
    }
  })
})

describe('Delete waits out an Undo window', () => {
  it('Delete then Undo restores the same recording under the same name, and nothing is cleared', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    // The slot reads empty at once, and the toast says which recording went.
    expect(slotOf(1).classList.contains('has'), 'F1 still reads as recorded').toBe(false)
    expect(document.body.textContent).toContain('Deleted F1 (CQ)')
    expect(cleared(), 'the backend was told to clear before the window passed').toEqual([])

    // Playing in the meantime: F1 is refused (it reads empty), F2 plays, the Undo survives.
    await press('F1')
    await press('F2')
    await act(async () => {
      fireEvent.click(playOf(2))
    })
    expect(played()).toEqual([2, 2])
    await wait(VOICE_UNDO_MS / 2)

    const undo = undoButton()
    expect(undo, 'no Undo on screen').not.toBeNull()
    await act(async () => {
      fireEvent.click(undo!)
    })
    expect(slotOf(1).classList.contains('has'), 'F1 did not come back').toBe(true)
    expect(playOf(1).title).toBe('Play F1 (CQ)')
    await press('F1')
    expect(played()).toEqual([2, 2, 1])

    await wait(VOICE_UNDO_MS * 2)
    expect(cleared(), 'an undone delete reached the backend').toEqual([])
    expect(slotOf(1).classList.contains('has')).toBe(true)
  })

  it('Delete without Undo clears the slot once the window passes, and not before', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    await wait(VOICE_UNDO_MS - 100)
    expect(cleared(), 'cleared inside the window').toEqual([])
    expect(undoButton(), 'the Undo left early').not.toBeNull()
    await wait(200)
    expect(cleared()).toEqual([1])
    expect(undoButton(), 'the Undo outlived the window').toBeNull()
    expect(slotOf(1).classList.contains('has')).toBe(false)
    expect(slotOf(2).classList.contains('has'), 'F2 was touched').toBe(true)
  })

  it('a second Delete after an Undo gets a whole window of its own', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    await wait(VOICE_UNDO_MS - 2000)
    await act(async () => {
      fireEvent.click(undoButton()!)
    })
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    // The first window's end comes and goes; the second delete is still undoable.
    await wait(VOICE_UNDO_MS - 1000)
    expect(cleared(), 'the undone delete’s window cleared the new one').toEqual([])
    expect(undoButton()).not.toBeNull()
    await wait(2000)
    expect(cleared()).toEqual([1])
  })

  it('clicking a deleted slot records over it: the Undo stands until the take is saved, and the window never wipes the take', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    // The empty slot's own button records — the operator reaching for Play inside the window.
    await act(async () => {
      fireEvent.click(playOf(1))
    })
    expect(api.startVoiceRecording).toHaveBeenCalledTimes(1)
    expect(cleared(), 'starting a take cleared the old recording').toEqual([])
    await wait(VOICE_UNDO_MS / 2)
    expect(undoButton(), 'starting a take ended the Undo window').not.toBeNull()

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '■ Stop & save' }))
    })
    expect(api.stopVoiceRecording).toHaveBeenCalledWith(1, 'CQ')
    expect(undoButton(), 'an Undo is still offered after the take was saved').toBeNull()
    await wait(VOICE_UNDO_MS * 2)
    // The save overwrote the file; a clear after it would have wiped the new take.
    expect(cleared(), 'the window cleared the slot over the new take').toEqual([])
  })

  it('hiding the keyer does not change the delete: Undo still works, and no Undo still clears', async () => {
    const view = await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
      fireEvent.click(deleteOf(2))
    })
    view.rerender(<Toasts />) // the keyer's pane goes; the toasts stay
    const undos = screen.getAllByRole('button', { name: /^Undo/ })
    expect(undos).toHaveLength(2)
    await act(async () => {
      fireEvent.click(undos[0])
    })
    await wait(VOICE_UNDO_MS * 2)
    expect(cleared(), 'F1 was undone; F2 was not').toEqual([2])
  })

  it('the Undo is a focusable button in an announced toast', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(deleteOf(1))
    })
    const undo = undoButton()!
    expect(undo.tagName).toBe('BUTTON')
    expect(undo.hasAttribute('disabled')).toBe(false)
    undo.focus()
    expect(document.activeElement).toBe(undo)
    // Radix announces a toast through a live region it fills on the next frame; the message is
    // in it.
    await wait(50)
    const live = [...document.querySelectorAll('[aria-live]')].filter(
      (el) => el.getAttribute('aria-live') !== 'off',
    )
    expect(live.some((el) => (el.textContent ?? '').includes('Deleted F1 (CQ)'))).toBe(true)
  })
})

describe('Play, the F-keys and ■ Stop are unchanged', () => {
  it('a slot’s click and its F-key play it; Esc and ■ Stop stop', async () => {
    await renderKeyer()
    await act(async () => {
      fireEvent.click(playOf(1))
    })
    await press('F2')
    expect(played()).toEqual([1, 2])
    await press('Escape')
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '■ Stop' }))
    })
    expect(api.stopVoice).toHaveBeenCalledTimes(2)
  })
})

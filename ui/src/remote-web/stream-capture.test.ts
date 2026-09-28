import { expect, it } from 'vitest'
import { ClickCount, HeldInput, framePoint, keyMessage, pointerMessage, textMessage, wheelMessage, type PictureBox } from './stream-capture'
import { parseStreamInput } from './stream-protocol'

// A 16:9 picture laid out in a 1600x1000 box: `object-fit: contain` shows it 1600x900, with a 50 px
// bar above and below it that is part of the element and none of the frame.
const BOX: PictureBox = { left: 0, top: 100, width: 1600, height: 1000, videoWidth: 1920, videoHeight: 1080 }
const none = { shiftKey: false, ctrlKey: false, altKey: false, metaKey: false }
const at = (clientX: number, clientY: number, extra: Partial<{ button: number; buttons: number; pointerType: string }> = {}) =>
  ({ ...none, clientX, clientY, button: 0, buttons: 1, ...extra })

it('maps a point on the picture to the frame, and a point on the bars around it to nothing', () => {
  expect(framePoint(BOX, 800, 600)).toEqual({ x: 0.5, y: 0.5 })
  expect(framePoint(BOX, 0, 150)).toEqual({ x: 0, y: 0 })
  expect(framePoint(BOX, 1600, 1050)).toEqual({ x: 1, y: 1 })
  // On the element, in the bars: not on Nexus.
  expect(framePoint(BOX, 800, 120)).toBeNull()
  expect(framePoint(BOX, 800, 1080)).toBeNull()
  // Off the element altogether, and before there is any frame to land on.
  expect(framePoint(BOX, -5, 600)).toBeNull()
  expect(framePoint({ ...BOX, videoWidth: 0, videoHeight: 0 }, 800, 600)).toBeNull()
})

it('pins a drag or its release to the picture\'s edge, and only when asked', () => {
  expect(pointerMessage('move', BOX, at(800, 120))).toBeNull()
  const pinned = pointerMessage('up', BOX, at(1700, 20, { button: 0, buttons: 0 }), true)!
  expect({ x: pinned.x, y: pinned.y }).toEqual({ x: 1, y: 0 })
  expect(() => parseStreamInput(pinned)).not.toThrow()
})

it('builds each input in exactly the contract\'s shape', () => {
  const down = pointerMessage('down', BOX, at(800, 600, { pointerType: 'pen' }), false, 2)!
  expect(down).toEqual({ type: 'pointer', action: 'down', x: 0.5, y: 0.5, button: 0, buttons: 1, modifiers: 0, pointerType: 'pen', clicks: 2 })
  // A move carries no click count, and an unknown pointer type is a mouse.
  expect(pointerMessage('move', BOX, at(800, 600, { button: -1, pointerType: 'stylus' }), false, 2)).toMatchObject({ button: -1, clicks: 0, pointerType: 'mouse' })
  const wheel = wheelMessage(BOX, { ...none, ctrlKey: true, clientX: 800, clientY: 600, deltaX: 0, deltaY: 3, deltaMode: 1 })!
  expect(wheel).toEqual({ type: 'wheel', x: 0.5, y: 0.5, deltaX: 0, deltaY: 3, deltaMode: 1, modifiers: 2 })
  expect(wheelMessage(BOX, { ...none, clientX: 800, clientY: 600, deltaX: 0, deltaY: 1e9, deltaMode: 0 })!.deltaY).toBe(10_000)
  expect(keyMessage('down', { ...none, shiftKey: true, key: 'W', code: 'KeyW', repeat: false }))
    .toEqual({ type: 'key', action: 'down', key: 'W', code: 'KeyW', modifiers: 1, repeat: false })
  for (const message of [down, wheel]) expect(() => parseStreamInput(message)).not.toThrow()
})

it('counts clicks as the browser does for its mouse events: the same spot, the same button, within 500 ms', () => {
  const count = new ClickCount()
  const press = (timeStamp: number, clientX = 100, clientY = 100, button = 0) => count.press({ timeStamp, clientX, clientY, button })
  expect([press(0), press(200), press(400), press(600)], 'single, double, triple, and no further than the contract').toEqual([1, 2, 3, 3])
  expect(press(1200), 'too late: a first click again').toBe(1)
  expect(press(1300, 110), 'too far').toBe(1)
  expect(press(1400, 110, 100, 2), 'another button').toBe(1)
  // Control: close enough in both.
  expect(press(1500, 112, 97, 2)).toBe(2)
})

it('keeps Tab and composition input on the page', () => {
  expect(keyMessage('down', { ...none, key: 'Tab', code: 'Tab', repeat: false })).toBeNull()
  expect(keyMessage('down', { ...none, key: 'a', code: 'KeyA', repeat: false, isComposing: true })).toBeNull()
  expect(keyMessage('down', { ...none, key: 'Process', code: 'KeyA', repeat: false })).toBeNull()
  // Control: an ordinary key is sent.
  expect(keyMessage('down', { ...none, key: 'Escape', code: 'Escape', repeat: false })).not.toBeNull()
})

it('sends a paste as one line of committed text, within the contract\'s 256 characters', () => {
  expect(textMessage('  W1AW\r\n')).toEqual({ type: 'text', text: 'W1AW' })
  expect(textMessage('CQ\nCQ')).toEqual({ type: 'text', text: 'CQ CQ' })
  expect(textMessage('x'.repeat(300))!.text).toHaveLength(256)
  expect(textMessage('\n\t')).toBeNull()
  expect(() => parseStreamInput(textMessage('a'.repeat(300))!)).not.toThrow()
})

it('remembers what is held and releases exactly that: keys, and the press with its own button', () => {
  const held = new HeldInput()
  held.note(keyMessage('down', { ...none, key: ' ', code: 'Space', repeat: false })!)
  held.note(keyMessage('down', { ...none, key: 'a', code: 'KeyA', repeat: false })!)
  held.note(keyMessage('up', { ...none, key: 'a', code: 'KeyA', repeat: false })!)
  held.note(pointerMessage('down', BOX, at(800, 600, { button: 2, buttons: 2 }))!)
  held.note(pointerMessage('move', BOX, at(400, 600, { button: -1, buttons: 2 }))!)
  expect(held.dragging).toBe(true)
  expect(held.releaseAll()).toEqual([
    { type: 'key', action: 'up', key: ' ', code: 'Space', modifiers: 0, repeat: false },
    { type: 'pointer', action: 'up', x: 0.25, y: 0.5, button: 2, buttons: 0, modifiers: 0, pointerType: 'mouse', clicks: 0 },
  ])
  // Released once: nothing is left to release.
  expect(held.releaseAll()).toEqual([])
  expect(held.dragging).toBe(false)
})

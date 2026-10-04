import { expect, it } from 'vitest'
import { ClickCount, HeldInput, PictureZoom, ZOOM_MAX, framePoint, keyMessage, keyPress, pointerMessage, textMessage, typedChange, typedText, wheelMessage, type PictureBox, type Stage } from './stream-capture'
import { STREAM_CONTROL_BYTES, parseStreamInput } from './stream-protocol'

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

it('says what is held as the page re-asserts it: keys by their code, and the pointer\'s buttons', () => {
  const held = new HeldInput()
  expect(held.held()).toEqual({ keys: [], buttons: 0 })
  held.note(keyMessage('down', { ...none, key: ' ', code: 'Space', repeat: false })!)
  held.note(keyMessage('down', { ...none, key: 'q', code: '', repeat: false })!)
  held.note(pointerMessage('down', BOX, at(800, 600, { button: 2, buttons: 2 }))!)
  expect(held.held(), 'a key with no code is never re-asserted').toEqual({ keys: ['Space'], buttons: 2 })
  held.note(keyMessage('up', { ...none, key: ' ', code: 'Space', repeat: false })!)
  held.note(pointerMessage('up', BOX, at(800, 600, { button: 2, buttons: 0 }))!)
  expect(held.held()).toEqual({ keys: [], buttons: 0 })
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

// ── Two fingers zoom and pan the picture on this page (never sent) ────────────────────────────────
// A phone's stage in portrait (412 x 594 CSS pixels under a 321 px header) showing the shack's 3440 x 1440
// window: fitted, the picture is 412 x 172.5 in the middle of it, centred on (206, 618).
const STAGE: Stage = { left: 0, top: 321, width: 412, height: 594, videoWidth: 3440, videoHeight: 1440 }
const fingers = (...points: [number, number][]) => points.map(([x, y]) => ({ x, y }))
/** Where the browser lays the zoomed picture's box out (its CSS transform about the stage's centre),
 *  as getBoundingClientRect reports it to the input bridge. */
const zoomedBox = (zoom: PictureZoom): PictureBox => ({
  left: STAGE.left + STAGE.width / 2 + zoom.x - zoom.zoom * STAGE.width / 2,
  top: STAGE.top + STAGE.height / 2 + zoom.y - zoom.zoom * STAGE.height / 2,
  width: STAGE.width * zoom.zoom, height: STAGE.height * zoom.zoom, videoWidth: STAGE.videoWidth, videoHeight: STAGE.videoHeight,
})

it('zooms by how far two fingers spread, about the point of the picture between them', () => {
  const zoom = new PictureZoom()
  expect(zoom.transform, 'the whole picture: no transform at all').toBe('')
  // About the picture's centre: 20 px apart to 100 px apart is five times.
  zoom.hold(fingers([196, 618], [216, 618]))
  zoom.move(fingers([156, 618], [256, 618]), STAGE)
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y }).toEqual({ zoom: 5, x: 0, y: 0 })
  expect(zoom.transform).toBe('translate(0px, 0px) scale(5)')
  // About a point 100 px right of the centre: that point stays under the fingers.
  zoom.fit()
  zoom.hold(fingers([296, 618], [316, 618]))
  zoom.move(fingers([286, 618], [326, 618]), STAGE)
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y }).toEqual({ zoom: 2, x: -100, y: 0 })
  // ...and the input bridge, reading the box as the browser lays it out, maps the fingers' point to
  // the same point of the frame as before the zoom: framePoint maps through it.
  expect(framePoint(zoomedBox(zoom), 306, 618)!.x).toBeCloseTo(framePoint(zoomedBox(new PictureZoom()), 306, 618)!.x, 9)
  expect(framePoint(zoomedBox(zoom), 306, 618)!.x).toBeCloseTo(0.5 + 100 / 412, 9)
  // A point that was off the frame's edge at the whole picture is on it now.
  expect(framePoint(zoomedBox(new PictureZoom()), 206, 820)).toBeNull()
  zoom.hold(fingers([206, 618], [226, 618]))
  zoom.move(fingers([166, 618], [266, 618]), STAGE)
  expect(framePoint(zoomedBox(zoom), 206, 820)).not.toBeNull()
})

it('never zooms out past the whole picture, nor in past ZOOM_MAX', () => {
  const zoom = new PictureZoom()
  zoom.hold(fingers([186, 618], [226, 618]))
  zoom.move(fingers([201, 618], [211, 618]), STAGE)
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y, transform: zoom.transform }).toEqual({ zoom: 1, x: 0, y: 0, transform: '' })
  zoom.hold(fingers([201, 618], [211, 618]))
  zoom.move(fingers([0, 618], [412, 618]), STAGE)
  expect(zoom.zoom).toBe(ZOOM_MAX)
})

it('pans with the fingers, but never so far that the stage shows past the picture\'s edge; a side smaller than the stage stays centred', () => {
  const zoom = new PictureZoom()
  zoom.hold(fingers([196, 618], [216, 618]))
  zoom.move(fingers([156, 618], [256, 618]), STAGE)
  // Both fingers dragged far right and down: the picture's left edge stops at the stage's, and its top
  // edge at the stage's top (at five times it is 862 px tall in a 594 px stage).
  zoom.hold(fingers([156, 618], [256, 618]))
  zoom.move(fingers([2156, 1118], [2256, 1118]), STAGE)
  expect(zoom.x).toBe((412 * 5 - 412) / 2)
  expect(zoom.y).toBeCloseTo((1440 * (412 / 3440) * 5 - 594) / 2, 9)
  const corner = framePoint(zoomedBox(zoom), 0.5, 321.5)!
  expect(corner.x < 0.001 && corner.y < 0.001, 'the stage\'s top-left corner shows the frame\'s: ' + JSON.stringify(corner)).toBe(true)
  // At twice, the picture is 345 px tall in the 594 px stage: it stays in the middle, however the fingers go.
  zoom.fit()
  zoom.hold(fingers([196, 618], [216, 618]))
  zoom.move(fingers([186, 618], [226, 618]), STAGE)
  zoom.hold(fingers([186, 618], [226, 618]))
  zoom.move(fingers([186, 900], [226, 900]), STAGE)
  expect({ zoom: zoom.zoom, y: zoom.y }).toEqual({ zoom: 2, y: 0 })
})

it('carries on without a jump when a finger lifts: the one left pans, and Fit is the whole picture again', () => {
  const zoom = new PictureZoom()
  zoom.hold(fingers([196, 618], [216, 618]))
  zoom.move(fingers([156, 618], [256, 618]), STAGE)
  zoom.hold(fingers([256, 618]))
  zoom.move(fingers([256, 618]), STAGE)
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y }, 'nothing moved').toEqual({ zoom: 5, x: 0, y: 0 })
  zoom.move(fingers([206, 598]), STAGE)
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y }).toEqual({ zoom: 5, x: -50, y: -20 })
  // The stage turned to landscape under a zoomed picture: kept over the new stage.
  zoom.keep({ ...STAGE, top: 174, width: 915, height: 238 })
  expect(zoom.x).toBe(-50)
  zoom.fit()
  expect({ zoom: zoom.zoom, x: zoom.x, y: zoom.y, transform: zoom.transform }).toEqual({ zoom: 1, x: 0, y: 0, transform: '' })
  // No fingers held, nothing moves.
  zoom.move(fingers([0, 0], [400, 400]), STAGE)
  expect(zoom.zoom).toBe(1)
})

// The typing box (the operator's pick, 2026-10-03): what a phone's keyboard commits, as the contract's `text`, with
// Enter and Backspace as `key`. Unlike a paste, nothing is trimmed: a word goes as it is finished, the space after it
// with the next, and dropping that space would run the words together at the shack.
it('typed text keeps every character, spaces too, drops control characters as the station does, and goes in pieces the contract takes', () => {
  expect(typedText(' ')).toEqual([{ type: 'text', text: ' ' }])
  expect(typedText('CQ DX ')).toEqual([{ type: 'text', text: 'CQ DX ' }])
  expect(typedText('a\u0007b\u007fc\u0085d\ne')).toEqual([{ type: 'text', text: 'abcde' }])
  expect(typedText('\u0000\n')).toEqual([])
  // 256 characters to a message, in order.
  const long = 'x'.repeat(300)
  expect(typedText(long).map(m => m.text.length)).toEqual([256, 44])
  // And never more bytes than the control channel carries: 256 four-byte characters would be 1047 with the message
  // around them, which the link refuses to send. Cut by bytes, in order, and every piece is one the station takes.
  const wide = '📡'.repeat(256)
  const pieces = typedText(wide)
  expect(pieces.map(m => m.text).join('')).toBe(wide)
  expect(pieces.length).toBe(2)
  for (const piece of pieces) {
    expect(new TextEncoder().encode(JSON.stringify(piece)).length).toBeLessThanOrEqual(STREAM_CONTROL_BYTES)
    expect(() => parseStreamInput(piece)).not.toThrow()
  }
  // CONTROL: a paste is trimmed and has its line breaks made spaces, as before.
  expect(textMessage(' CQ\n')).toEqual({ type: 'text', text: 'CQ' })
})

it('Enter and Backspace go as a key pressed and let go, as the contract has them', () => {
  for (const key of ['Enter', 'Backspace'] as const) {
    const [down, up] = keyPress(key)
    expect([down, up]).toEqual([
      { type: 'key', action: 'down', key, code: key, modifiers: 0, repeat: false },
      { type: 'key', action: 'up', key, code: key, modifiers: 0, repeat: false },
    ])
    expect(() => { parseStreamInput(down); parseStreamInput(up) }).not.toThrow()
  }
})

it('a change in the typing box goes as Backspaces past the part both share, then the rest as text', () => {
  const sent = (from: string, to: string) => typedChange(from, to).map(m => m.type === 'text' ? m.text : m.action === 'down' ? '⌫' : '')
    .filter(Boolean)
  expect(sent('', 'W1AW')).toEqual(['W1AW'])
  expect(sent('W1AW', 'W1AW ')).toEqual([' '])
  expect(sent('W1AW', 'W1A')).toEqual(['⌫'])
  // A word the keyboard corrected: back to where they part, then the correction.
  expect(sent('teh ', 'the ')).toEqual(['⌫', '⌫', '⌫', 'he '])
  // A character changed in the middle: back to it, and the rest again.
  expect(sent('K1ABC', 'K2ABC')).toEqual(['⌫', '⌫', '⌫', '⌫', '2ABC'])
  // Characters outside the first plane count as one, as the station counts them.
  expect(sent('a📡b', 'a📡')).toEqual(['⌫'])
  expect(sent('same', 'same')).toEqual([])
  // Each Backspace is a press and its release.
  expect(typedChange('ab', 'a').map(m => m.type === 'key' ? m.action : m.type)).toEqual(['down', 'up'])
})

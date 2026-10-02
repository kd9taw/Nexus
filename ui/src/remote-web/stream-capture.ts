// Input on the stream's picture, turned into the events Nexus's own window receives (S11). Pure
// functions over the browser's events, so the one thing that matters - that only input aimed AT
// THE PICTURE is ever sent - is decided here, where it can be tested without a layout. The shapes
// are the contract's (remote/test/fixtures/stream/channel.json).
//
// WHAT NEVER LEAVES THIS PAGE. A position outside the picture (the letterbox bars around it), a key
// pressed anywhere but on the focused picture, Tab (it moves focus out of the picture, so the
// keyboard can never be trapped in it), and composition input (an IME's work in progress, which
// has no single key to send) are all dropped. The station forwards what does arrive into Nexus's
// window as DOM events; there is no path from here to anything else on the shack PC.
import { MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT, type StreamKey, type StreamPointer, type StreamText, type StreamWheel } from './stream-protocol'

/** The picture's box as laid out, and the frame's own size. */
export type PictureBox = { left: number; top: number; width: number; height: number; videoWidth: number; videoHeight: number }
type Modifiers = { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean }

export function modifiers(event: Modifiers): number {
  return (event.shiftKey ? MOD_SHIFT : 0) | (event.ctrlKey ? MOD_CTRL : 0) | (event.altKey ? MOD_ALT : 0) | (event.metaKey ? MOD_META : 0)
}

function shown(box: PictureBox): { x: number; y: number; width: number; height: number } | null {
  if (!(box.videoWidth > 0 && box.videoHeight > 0 && box.width > 0 && box.height > 0)) return null
  const scale = Math.min(box.width / box.videoWidth, box.height / box.videoHeight)
  const width = box.videoWidth * scale, height = box.videoHeight * scale
  return { x: box.left + (box.width - width) / 2, y: box.top + (box.height - height) / 2, width, height }
}
/** Where on the decoded frame a point lands, 0..1 from its top-left corner, or null when it lands on
 *  the bars `object-fit: contain` leaves around it - or when there is no frame yet to land on. */
export function framePoint(box: PictureBox, clientX: number, clientY: number): { x: number; y: number } | null {
  const picture = shown(box)
  if (!picture) return null
  const x = (clientX - picture.x) / picture.width, y = (clientY - picture.y) / picture.height
  if (!(x >= 0 && x <= 1 && y >= 0 && y <= 1)) return null
  return { x, y }
}
/** The same point pinned to the picture's nearest edge: for a drag that has left the picture, and
 *  the release that ends it, which must still arrive or Nexus is left holding a button. */
function edgePoint(box: PictureBox, clientX: number, clientY: number): { x: number; y: number } | null {
  const picture = shown(box)
  if (!picture) return null
  const clamp = (value: number) => Math.min(1, Math.max(0, Number.isFinite(value) ? value : 0))
  return { x: clamp((clientX - picture.x) / picture.width), y: clamp((clientY - picture.y) / picture.height) }
}

type PointerLike = Modifiers & { clientX: number; clientY: number; button: number; buttons: number; pointerType?: string }
/** One pointer event on the picture. `clicks` is the press's click count (ClickCount), so a double
 *  click at the shack is a double click and not two singles; a move carries none, and a release
 *  with none clicks nothing there. `clamp` pins a point outside the picture to its edge instead of
 *  dropping it. */
export function pointerMessage(action: StreamPointer['action'], box: PictureBox, event: PointerLike, clamp = false, clicks = 0): StreamPointer | null {
  const point = framePoint(box, event.clientX, event.clientY) ?? (clamp ? edgePoint(box, event.clientX, event.clientY) : null)
  if (!point) return null
  const pointerType = event.pointerType === 'touch' || event.pointerType === 'pen' ? event.pointerType : 'mouse'
  return { type: 'pointer', action, x: point.x, y: point.y, button: Math.max(-1, Math.min(4, event.button)), buttons: event.buttons & 31,
    modifiers: modifiers(event), pointerType, clicks: action === 'move' ? 0 : Math.max(0, Math.min(3, clicks | 0)) }
}

/** The click count of a press, counted here because a pointer event does not carry one: Chrome's
 *  pointerdown and pointerup say `detail` 0 whatever the count, and a release sent with 0 clicks
 *  nothing at the shack. Counted as the browser counts its mouse events - a press within 500 ms and
 *  4 px of the last one, with the same button, is the next click of a double or triple click - and
 *  never past the contract's 3. */
export class ClickCount {
  private last: { at: number; x: number; y: number; button: number; count: number } | null = null
  press(event: { timeStamp: number; clientX: number; clientY: number; button: number }): number {
    const last = this.last
    const count = last && event.timeStamp - last.at <= 500 && Math.abs(event.clientX - last.x) <= 4 && Math.abs(event.clientY - last.y) <= 4
      && event.button === last.button ? Math.min(3, last.count + 1) : 1
    this.last = { at: event.timeStamp, x: event.clientX, y: event.clientY, button: event.button, count }
    return count
  }
}

/** One wheel event on the picture, in the browser's own delta mode: the dispatcher in Nexus's window
 *  turns lines and pages into pixels against its own layout, which is the one that scrolls. */
export function wheelMessage(box: PictureBox, event: Modifiers & { clientX: number; clientY: number; deltaX: number; deltaY: number; deltaMode: number }): StreamWheel | null {
  const point = framePoint(box, event.clientX, event.clientY)
  if (!point) return null
  const bound = (value: number) => Math.max(-10_000, Math.min(10_000, Number.isFinite(value) ? value : 0))
  const deltaMode = event.deltaMode === 1 || event.deltaMode === 2 ? event.deltaMode : 0
  return { type: 'wheel', x: point.x, y: point.y, deltaX: bound(event.deltaX), deltaY: bound(event.deltaY), deltaMode, modifiers: modifiers(event) }
}

/** One key on the focused picture, or null for the ones that stay here: Tab (leaving the picture is
 *  the keyboard's way out), and composition input, which is not a key. */
export function keyMessage(action: StreamKey['action'], event: Modifiers & { key: string; code: string; repeat: boolean; isComposing?: boolean }): StreamKey | null {
  if (event.key === 'Tab' || event.isComposing || event.key === 'Process' || event.key === 'Dead' || event.key === 'Unidentified') return null
  if (!event.key || [...event.key].length > 32 || /[\u0000-\u001f\u007f]/.test(event.key) || !/^[A-Za-z0-9]{0,32}$/.test(event.code)) return null
  return { type: 'key', action, key: event.key, code: event.code, modifiers: modifiers(event), repeat: event.repeat }
}

/** Committed text for the focused field at the shack - what a paste on the picture carries. Line
 *  breaks and other control characters are dropped (a field that takes a callsign takes one line),
 *  and it is cut to the contract's 256 characters. Null when nothing is left. */
export function textMessage(text: string): StreamText | null {
  const clean = [...text.replace(/[\u0000-\u001f\u007f]+/g, ' ').trim()].slice(0, 256).join('')
  return clean ? { type: 'text', text: clean } : null
}

/** What this page has pressed at the shack and not yet released. A key or button that went DOWN
 *  in Nexus's window has to come UP there too, whatever happens here: Space is the Phone cockpit's
 *  push-to-talk key, and a key-up this page never saw - focus moved while it was held - would
 *  otherwise leave the rig keyed until the station's own backstops caught it. So every release the
 *  page did not see is sent the moment the picture loses focus. */
export class HeldInput {
  private keys = new Map<string, StreamKey>()
  private pointer: StreamPointer | null = null
  /** Record what is about to be sent; returns it unchanged. */
  note<T extends StreamKey | StreamPointer>(message: T): T {
    if (message.type === 'key') {
      const id = message.code || message.key
      if (message.action === 'down') this.keys.set(id, message); else this.keys.delete(id)
    } else if (message.action === 'down') this.pointer = message
    // A drag keeps the press's own button (a move carries none) and follows the position, so the
    // release that ends it lands where the pointer last was, with the button that was pressed.
    else if (message.action === 'move') this.pointer = this.pointer && message.buttons ? { ...this.pointer, x: message.x, y: message.y, buttons: message.buttons } : null
    else this.pointer = null
    return message
  }
  /** Is a button held on the picture right now? Only then may a drag go past its edge. */
  get dragging(): boolean { return this.pointer !== null }
  /** What is held right now, as the page re-asserts it: each key by its code (a key without one is
   *  never re-asserted, so the shack lets it go at the gap), and the pointer's buttons. */
  held(): { keys: string[]; buttons: number } {
    return { keys: [...this.keys.values()].map(key => key.code).filter(code => code !== ''), buttons: this.pointer?.buttons ?? 0 }
  }
  /** Everything still held, as the releases that end it, and forget it all. */
  releaseAll(): (StreamKey | StreamPointer)[] {
    const releases: (StreamKey | StreamPointer)[] = [...this.keys.values()].map(key => ({ ...key, action: 'up' as const, repeat: false }))
    if (this.pointer) releases.push({ ...this.pointer, action: 'up', buttons: 0, clicks: 0 })
    this.keys.clear(); this.pointer = null
    return releases
  }
}

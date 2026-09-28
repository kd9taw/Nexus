// S11, the shack's half of the stream's input bridge: what a streamed operator does over the
// picture, delivered to THIS window as DOM events, and nothing else.
//
// THE RULE THIS FILE EXISTS TO HOLD (A2). Remote input reaches Nexus's own page and no other
// program on the shack PC. There is no OS-level injection anywhere in the path - the station has no
// SendInput, keybd_event or mouse_event, and WebView2 offers no injection API in the hosting Nexus
// uses - so the bridge is JavaScript inside this page by construction: the station forwards each
// admitted input message to the main window as the Tauri event `remote-stream-input`, and this
// module turns it into synthetic events dispatched at `document.elementFromPoint`. Into the page,
// a DOM event is the only thing it can produce; it opens nothing, and the one thing it says back to
// the station is whether Space is the push-to-talk key here (below).
//
// WHAT A SYNTHETIC EVENT DOES NOT DO BY ITSELF. `isTrusted` is false, so the browser runs none of
// the defaults the UI relies on; this module reproduces the ones that matter - focus on press, a
// click from a press and release, text typed into the focused field, Backspace and Delete, Enter
// submitting a form, Space and Enter pressing a button, wheel scrolling, range sliders dragged, and
// a `<select>` chosen (its native list is a separate window the stream never shows, so a small
// in-page list stands in for it). Handlers written for pointer capture get it too, for the
// synthetic pointer only. What it does not reproduce is named in the WB report and measured on the
// bench (plan §1.4, gesture coverage): HTML drag-and-drop, text selection by dragging, IME
// composition, and the browser's own shortcuts.
//
// ⛔ THE PTT KEY NEVER ARRIVES AS A KEY (the lead's ruling, 2026-09-27). In the Phone cockpit Space
// is push-to-talk: its window handler keys the rig on Space down and unkeys on Space up
// (PhoneCockpit.tsx). A key pair across a network is a stuck transmitter waiting for one lost
// key-up, so a streamed Space never becomes one. Where Space is the PTT key the page holds PTT on
// the stream's own re-asserted channel instead (S7), and this module drops any Space that arrives
// anyway, its key-up and `reset`'s forced release included. Only this window can tell where Space
// is the PTT key - the cockpit's Lock, and whether the key would land in a field, where it is
// typing - so it tells the station after every input it handles and whenever that changes, and
// the station tells the page. Button clicks, Stop, Tune and the FT buttons among them, are
// dispatched like any other: the session's transmit presence covers them.
//
// ⚠️ WHAT GOES DOWN COMES UP. A key or button held here when a stream ends would stay held: a Shift
// that makes every later click a Shift-click, a button still pressed. `reset` - which the station
// sends when a stream ends or its transmit presence lapses - releases everything this module is
// holding.
import { reportStreamPttKey } from '../api'
import { MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT, parseWebviewInput, type StreamKey, type StreamPointer, type StreamWheel } from '../remote-web/stream-protocol'

export const STREAM_INPUT_EVENT = 'remote-stream-input'
/** The one pointer the stream drives. Real pointers get small ids from the browser (1 is the mouse). */
export const STREAM_POINTER_ID = 7331

type Win = Window & typeof globalThis
type Modifiers = { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean }
type AnyInit = PointerEventInit | WheelEventInit | KeyboardEventInit | InputEventInit
const modifiers = (bits: number): Modifiers =>
  ({ shiftKey: !!(bits & MOD_SHIFT), ctrlKey: !!(bits & MOD_CTRL), altKey: !!(bits & MOD_ALT), metaKey: !!(bits & MOD_META) })
const FOCUSABLE = 'a[href], button, input, select, textarea, summary, [tabindex], [contenteditable="true"], [contenteditable=""]'
const TEXT_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number', ''])

/** Places that have Space armed as the station's push-to-talk key right now: the Phone cockpit, for
 *  exactly as long as its own window handler keys the rig on Space. */
let pttKeyArms = 0
const pttKeyWatchers = new Set<() => void>()
/** Arm Space as the push-to-talk key for the stream; returns the disarm. The Phone cockpit calls it
 *  while it has control and Lock is off, which is when its own Space handler keys the rig. */
export function armStreamPttKey(): () => void {
  pttKeyArms++
  for (const f of pttKeyWatchers) f()
  let armed = true
  return () => {
    if (!armed) return
    armed = false
    pttKeyArms--
    for (const f of pttKeyWatchers) f()
  }
}
/** The Phone cockpit's own test for "this Space is typing, not PTT": the key lands in an INPUT or a
 *  TEXTAREA (PhoneCockpit.tsx, `isField`), whatever kind. */
const typedInto = (el: Element) => el.tagName === 'INPUT' || el.tagName === 'TEXTAREA'

export class StreamInputDispatcher {
  private hover: Element | null = null
  private pressed: { target: Element; button: number } | null = null
  private buttons = 0
  private last = { x: 0, y: 0 }
  private keys = new Map<string, StreamKey>()
  /** Where each held key went down, and whether the page cancelled it: Space presses a button on
   *  key-up only when it went down on that same button and nobody cancelled it. */
  private keyDowns = new Map<string, { target: Element; cancelled: boolean }>()
  private captured: Element | null = null
  private range: HTMLInputElement | null = null
  private picker: { select: HTMLSelectElement; list: HTMLElement } | null = null
  private unpatch: (() => void) | null = null
  /** A stream has used this window, so a change in where Space is PTT is worth telling. */
  private attached = false
  private told: boolean | null = null
  private readonly changed = () => queueMicrotask(() => this.tell(false))

  /** `report` carries the one thing this window says back: whether Space is the push-to-talk key. */
  constructor(private readonly win: Win, private readonly report?: (pttKey: boolean) => void) {
    this.patchCapture()
    this.doc.addEventListener('focusin', this.changed, true)
    this.doc.addEventListener('focusout', this.changed, true)
    pttKeyWatchers.add(this.changed)
  }

  /** One message from the station. Anything outside the contract is ignored, never guessed at. */
  handle(raw: unknown): void {
    let input
    try { input = parseWebviewInput(raw) } catch { return }
    if (input.type === 'reset') this.reset()
    else if (input.type === 'pointer') this.pointer(input)
    else if (input.type === 'wheel') this.wheel(input)
    else if (input.type === 'key') this.key(input)
    else this.insert(this.focused(), input.text, 'insertText')
    this.attached = true
    // The answer to every input that can move focus: the page trusts nothing older.
    if (input.type !== 'wheel' && !(input.type === 'pointer' && input.action === 'move')) this.tell(true)
  }

  /** Is Space here the station's push-to-talk key? The Phone cockpit's own rule: armed, and the key
   *  would not land in a field. */
  pttKey(): boolean { return pttKeyArms > 0 && !typedInto(this.focused()) }

  private tell(always: boolean): void {
    if (!this.report || !this.attached) return
    const pttKey = this.pttKey()
    if (!always && pttKey === this.told) return
    this.told = pttKey
    this.report(pttKey)
  }

  /** Let go of everything: every held key comes up, a held button is released without a click, a
   *  drag ends where it was, and an open list closes. */
  reset(): void {
    // Forced releases: each key comes up, and none of them presses anything on the way.
    for (const held of [...this.keys.values()]) this.key({ ...held, action: 'up', repeat: false }, true)
    this.keys.clear(); this.keyDowns.clear()
    if (this.pressed || this.buttons) {
      const target = this.captured ?? this.pressed?.target ?? this.doc.documentElement
      const init = this.pointerInit({ x: this.last.x, y: this.last.y, button: this.pressed?.button ?? 0, buttons: 0, modifiers: 0, pointerType: 'mouse', clicks: 0 })
      this.fire(target, 'pointerup', init, 'pointer')
      this.fire(target, 'mouseup', init)
      this.endRange()
      this.release()
      this.pressed = null; this.buttons = 0
    }
    this.closePicker()
  }

  dispose(): void {
    this.reset(); this.unpatch?.(); this.unpatch = null
    this.doc.removeEventListener('focusin', this.changed, true)
    this.doc.removeEventListener('focusout', this.changed, true)
    pttKeyWatchers.delete(this.changed)
    this.attached = false
  }

  private get doc(): Document { return this.win.document }
  private focused(): Element { return this.doc.activeElement ?? this.doc.body }

  private at(x: number, y: number): Element {
    const clientX = x * this.win.innerWidth, clientY = y * this.win.innerHeight
    return this.doc.elementFromPoint?.(clientX, clientY) ?? this.doc.documentElement
  }
  private pointerInit(p: Omit<StreamPointer, 'type' | 'action'>): PointerEventInit {
    const clientX = p.x * this.win.innerWidth, clientY = p.y * this.win.innerHeight
    return { bubbles: true, cancelable: true, composed: true, clientX, clientY, screenX: clientX, screenY: clientY,
      button: p.button, buttons: p.buttons, detail: p.clicks, pointerId: STREAM_POINTER_ID, pointerType: p.pointerType, isPrimary: true,
      ...modifiers(p.modifiers) }
  }
  /** Dispatch one event; returns whether its default may run (nobody called preventDefault). */
  private fire(target: EventTarget, type: string, init: AnyInit, kind: 'pointer' | 'mouse' | 'wheel' | 'key' | 'input' | 'plain' = 'mouse'): boolean {
    const w = this.win
    const event = kind === 'pointer' ? new (w.PointerEvent ?? w.MouseEvent)(type, init)
      : kind === 'mouse' ? new w.MouseEvent(type, init)
      : kind === 'wheel' ? new w.WheelEvent(type, init)
      : kind === 'key' ? new w.KeyboardEvent(type, init)
      : kind === 'input' ? new (w.InputEvent ?? w.Event)(type, init)
      : new w.Event(type, init)
    return target.dispatchEvent(event)
  }

  private pointer(p: StreamPointer): void {
    this.last = { x: p.x, y: p.y }
    const init = this.pointerInit(p)
    const target = this.captured ?? this.at(p.x, p.y)
    if (!this.captured) this.hoverTo(target, init)
    if (p.action === 'move') {
      this.fire(target, 'pointermove', init, 'pointer')
      this.fire(target, 'mousemove', init)
      if (this.range && p.buttons) this.dragRange(this.range, init.clientX!)
      return
    }
    if (p.action === 'down') {
      if (this.picker && !this.picker.list.contains(target)) this.closePicker()
      this.buttons = p.buttons
      this.pressed = { target, button: p.button }
      const pointerAllowed = this.fire(target, 'pointerdown', init, 'pointer')
      const mouseAllowed = pointerAllowed ? this.fire(target, 'mousedown', init) : true
      if (mouseAllowed && p.button === 0) this.pressDefault(target, init.clientX!)
      return
    }
    // up or cancel
    this.buttons = p.buttons
    if (p.action === 'cancel') {
      this.fire(target, 'pointercancel', init, 'pointer')
      this.endRange(); this.release(); this.pressed = null
      return
    }
    this.fire(target, 'pointerup', init, 'pointer')
    this.fire(target, 'mouseup', init)
    this.endRange()
    const pressed = this.pressed
    this.release(); this.pressed = null
    if (!pressed || p.clicks === 0) return
    const clicked = common(pressed.target, target)
    if (!clicked) return
    if (p.button === 0) {
      this.fire(clicked, 'click', init)
      if (p.clicks === 2) this.fire(clicked, 'dblclick', init)
    } else {
      this.fire(clicked, 'auxclick', init)
      if (p.button === 2) this.fire(clicked, 'contextmenu', init)
    }
  }

  /** The browser's defaults for a primary press: focus moves to what was pressed (or away, if it
   *  cannot take focus); a list opens for a `<select>`; a range slider jumps to the press. */
  private pressDefault(target: Element, clientX: number): void {
    const focusable = target.closest(FOCUSABLE) as HTMLElement | null
    if (focusable && !(focusable as HTMLInputElement).disabled) focusable.focus({ preventScroll: true })
    else if (this.doc.activeElement instanceof this.win.HTMLElement && this.doc.activeElement !== this.doc.body) this.doc.activeElement.blur()
    if (focusable instanceof this.win.HTMLSelectElement && !focusable.disabled) this.openPicker(focusable)
    if (focusable instanceof this.win.HTMLInputElement && focusable.type === 'range' && !focusable.disabled) {
      this.range = focusable
      this.dragRange(focusable, clientX)
    }
  }

  private hoverTo(target: Element, init: PointerEventInit): void {
    const previous = this.hover
    if (previous === target) return
    this.hover = target
    if (previous?.isConnected) {
      this.fire(previous, 'pointerout', { ...init, relatedTarget: target }, 'pointer')
      this.fire(previous, 'mouseout', { ...init, relatedTarget: target })
      for (let node: Element | null = previous; node && !node.contains(target); node = node.parentElement) {
        this.fire(node, 'pointerleave', { ...init, bubbles: false, relatedTarget: target }, 'pointer')
        this.fire(node, 'mouseleave', { ...init, bubbles: false, relatedTarget: target })
      }
    }
    this.fire(target, 'pointerover', { ...init, relatedTarget: previous }, 'pointer')
    this.fire(target, 'mouseover', { ...init, relatedTarget: previous })
    const entered: Element[] = []
    for (let node: Element | null = target; node && !(previous?.isConnected && node.contains(previous)); node = node.parentElement) entered.unshift(node)
    for (const node of entered) {
      this.fire(node, 'pointerenter', { ...init, bubbles: false, relatedTarget: previous }, 'pointer')
      this.fire(node, 'mouseenter', { ...init, bubbles: false, relatedTarget: previous })
    }
  }

  private wheel(w: StreamWheel): void {
    const clientX = w.x * this.win.innerWidth, clientY = w.y * this.win.innerHeight
    const target = this.at(w.x, w.y)
    const allowed = this.fire(target, 'wheel', { bubbles: true, cancelable: true, composed: true, clientX, clientY,
      deltaX: w.deltaX, deltaY: w.deltaY, deltaMode: w.deltaMode, ...modifiers(w.modifiers) }, 'wheel')
    // Ctrl+wheel is zoom in a browser, never a scroll; anything a handler took is its own.
    if (!allowed || w.modifiers & MOD_CTRL) return
    const scroller = scrollable(target, this.win, w.deltaY !== 0 ? 'y' : 'x')
    if (!scroller) return
    const unit = w.deltaMode === 1 ? 16 : w.deltaMode === 2 ? scroller.clientHeight : 1
    scroller.scrollBy?.({ left: w.deltaX * unit, top: w.deltaY * unit, behavior: 'instant' as ScrollBehavior })
  }

  /** `forced` is a release the station asked for (`reset`): the key comes up and nothing is pressed. */
  private key(k: StreamKey, forced = false): void {
    const id = k.code || k.key
    // ⛔ Space where it is the push-to-talk key: dropped, never dispatched (the header says why).
    if (k.code === 'Space' && this.pttKey()) { this.keys.delete(id); this.keyDowns.delete(id); return }
    if (k.action === 'down') this.keys.set(id, k); else this.keys.delete(id)
    const target = this.focused()
    const init: KeyboardEventInit = { bubbles: true, cancelable: true, composed: true, key: k.key, code: k.code, repeat: k.repeat, ...modifiers(k.modifiers) }
    const allowed = this.fire(target, k.action === 'down' ? 'keydown' : 'keyup', init, 'key')
    if (k.action === 'up') {
      const down = this.keyDowns.get(id)
      this.keyDowns.delete(id)
      // Space presses a button, a checkbox or a radio on key-up, as a browser does - the one it went
      // down on, if nobody cancelled it.
      if (allowed && !forced && k.key === ' ' && down && !down.cancelled && down.target === target && pressable(target, this.win)) {
        this.fire(target, 'click', { bubbles: true, cancelable: true })
      }
      return
    }
    if (!k.repeat) this.keyDowns.set(id, { target, cancelled: !allowed })
    if (!allowed) return
    if (k.key === 'Escape' && this.picker) { this.closePicker(); return }
    if (this.picker && (k.key === 'ArrowDown' || k.key === 'ArrowUp' || k.key === 'Enter')) { this.pickerKey(k.key); return }
    const command = k.modifiers & (MOD_CTRL | MOD_META)
    if (target instanceof this.win.HTMLSelectElement) { this.selectKey(target, k.key); return }
    if (isTextField(target, this.win)) {
      if (command) { if (k.key.toLowerCase() === 'a') selectAll(target); return }
      if ([...k.key].length === 1) { this.insert(target, k.key, 'insertText'); return }
      if (k.key === 'Backspace' || k.key === 'Delete') { this.erase(target, k.key === 'Backspace'); return }
      if (k.key === 'ArrowLeft' || k.key === 'ArrowRight' || k.key === 'Home' || k.key === 'End') { moveCaret(target, k.key); return }
      if (k.key === 'Enter') {
        if (target instanceof this.win.HTMLTextAreaElement) this.insert(target, '\n', 'insertLineBreak')
        else target.form?.requestSubmit()
      }
      return
    }
    // Enter presses a button or follows a link on key-down, as a browser does - never a checkbox.
    if (k.key === 'Enter' && !command && activatedByEnter(target, this.win)) {
      this.fire(target, 'click', { bubbles: true, cancelable: true })
    }
  }

  /** Text into the focused field, the way typing puts it there: `beforeinput` may refuse it, and
   *  `input` follows, so a controlled field's own handler sees the change as if it were typed. */
  private insert(target: Element, text: string, inputType: string): void {
    if (!isTextField(target, this.win) || target.readOnly || target.disabled) return
    if (!this.fire(target, 'beforeinput', { bubbles: true, cancelable: true, composed: true, inputType, data: text }, 'input')) return
    const room = target.maxLength > 0 ? Math.max(0, target.maxLength - target.value.length + selectionLength(target)) : Infinity
    const piece = [...text].slice(0, room).join('')
    if (!piece) return
    const start = selectionStart(target), end = selectionEnd(target)
    if (start === null || end === null) setValue(target, target.value + piece, this.win)
    else target.setRangeText(piece, start, end, 'end')
    this.fire(target, 'input', { bubbles: true, composed: true, inputType, data: piece }, 'input')
  }
  private erase(target: HTMLInputElement | HTMLTextAreaElement, backward: boolean): void {
    if (target.readOnly || target.disabled) return
    const inputType = backward ? 'deleteContentBackward' : 'deleteContentForward'
    if (!this.fire(target, 'beforeinput', { bubbles: true, cancelable: true, composed: true, inputType }, 'input')) return
    const start = selectionStart(target), end = selectionEnd(target)
    if (start === null || end === null) setValue(target, backward ? target.value.slice(0, -1) : target.value, this.win)
    else if (start !== end) target.setRangeText('', start, end, 'end')
    else if (backward && start > 0) target.setRangeText('', start - 1, start, 'end')
    else if (!backward && end < target.value.length) target.setRangeText('', start, end + 1, 'end')
    else return
    this.fire(target, 'input', { bubbles: true, composed: true, inputType }, 'input')
  }

  private selectKey(select: HTMLSelectElement, key: string): void {
    const step = key === 'ArrowDown' || key === 'ArrowRight' ? 1 : key === 'ArrowUp' || key === 'ArrowLeft' ? -1 : 0
    if (!step || select.disabled) return
    for (let index = select.selectedIndex + step; index >= 0 && index < select.options.length; index += step) {
      if (!select.options[index].disabled) { this.choose(select, index); return }
    }
  }
  /** A choice made the way a person makes one: the value changes, then `input` and `change`. */
  private choose(select: HTMLSelectElement, index: number): void {
    if (select.selectedIndex === index) return
    select.selectedIndex = index
    this.fire(select, 'input', { bubbles: true, composed: true }, 'plain')
    this.fire(select, 'change', { bubbles: true }, 'plain')
  }
  /** The stand-in for a `<select>`'s native list: that list is a separate window the stream never
   *  shows, so an in-page one opens under the control, in Nexus's own colours, and is chosen from
   *  with the same synthetic clicks as everything else. */
  private openPicker(select: HTMLSelectElement): void {
    this.closePicker()
    const doc = this.doc, rect = select.getBoundingClientRect(), style = this.win.getComputedStyle(select)
    const list = doc.createElement('div')
    list.setAttribute('role', 'listbox')
    list.dataset.streamPicker = ''
    Object.assign(list.style, { position: 'fixed', left: `${rect.left}px`, top: `${rect.bottom}px`, minWidth: `${Math.max(rect.width, 80)}px`,
      maxHeight: `${Math.max(120, this.win.innerHeight - rect.bottom - 8)}px`, overflowY: 'auto', zIndex: '2147483647',
      background: 'var(--bg-elev)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)',
      font: style.font, boxShadow: '0 4px 16px rgb(0 0 0 / 0.35)' } satisfies Partial<CSSStyleDeclaration>)
    Array.from(select.options).forEach((option, index) => {
      const item = doc.createElement('div')
      item.setAttribute('role', 'option')
      item.setAttribute('aria-selected', String(index === select.selectedIndex))
      item.textContent = option.textContent
      Object.assign(item.style, { padding: '4px 10px', cursor: 'default', opacity: option.disabled ? '0.5' : '1',
        background: index === select.selectedIndex ? 'var(--accent)' : 'transparent',
        color: index === select.selectedIndex ? 'var(--bg)' : 'inherit' } satisfies Partial<CSSStyleDeclaration>)
      if (!option.disabled) item.addEventListener('click', () => { this.choose(select, index); this.closePicker(); select.focus({ preventScroll: true }) })
      list.appendChild(item)
    })
    doc.body.appendChild(list)
    this.picker = { select, list }
  }
  private pickerKey(key: string): void {
    const picker = this.picker!
    if (key === 'Enter') { this.closePicker(); return }
    this.selectKey(picker.select, key)
    const { select } = picker
    this.closePicker(); this.openPicker(select)
  }
  private closePicker(): void { this.picker?.list.remove(); this.picker = null }

  private dragRange(range: HTMLInputElement, clientX: number): void {
    const rect = range.getBoundingClientRect()
    if (!(rect.width > 0)) return
    const min = Number(range.min || 0), max = Number(range.max || 100), step = Number(range.step || 1)
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    const raw = min + fraction * (max - min)
    const snapped = step > 0 && Number.isFinite(step) ? min + Math.round((raw - min) / step) * step : raw
    const value = String(Math.min(max, Math.max(min, Number(snapped.toFixed(10)))))
    if (range.value === value) return
    setValue(range, value, this.win)
    this.fire(range, 'input', { bubbles: true, composed: true }, 'plain')
  }
  private endRange(): void {
    const range = this.range
    this.range = null
    if (range) this.fire(range, 'change', { bubbles: true }, 'plain')
  }

  /** Pointer capture, for the synthetic pointer only: a drag handler that captures the pointer
   *  (splitters, the scope, the map) would otherwise throw on an id the browser never issued, and
   *  the drag would never start. Real pointers go to the browser's own implementation, untouched. */
  private patchCapture(): void {
    const proto = this.win.Element.prototype as unknown as {
      setPointerCapture?: (this: Element, id: number) => void; releasePointerCapture?: (this: Element, id: number) => void; hasPointerCapture?: (this: Element, id: number) => boolean }
    const original = { set: proto.setPointerCapture, release: proto.releasePointerCapture, has: proto.hasPointerCapture }
    const self = this
    proto.setPointerCapture = function (this: Element, id: number) {
      if (id !== STREAM_POINTER_ID) return original.set?.call(this, id)
      if (self.captured !== this) { self.release(); self.captured = this; self.fire(this, 'gotpointercapture', { bubbles: true, pointerId: id } as PointerEventInit, 'pointer') }
    }
    proto.releasePointerCapture = function (this: Element, id: number) {
      if (id !== STREAM_POINTER_ID) return original.release?.call(this, id)
      if (self.captured === this) self.release()
    }
    proto.hasPointerCapture = function (this: Element, id: number) {
      return id === STREAM_POINTER_ID ? self.captured === this : !!original.has?.call(this, id)
    }
    this.unpatch = () => {
      if (original.set) proto.setPointerCapture = original.set; else delete proto.setPointerCapture
      if (original.release) proto.releasePointerCapture = original.release; else delete proto.releasePointerCapture
      if (original.has) proto.hasPointerCapture = original.has; else delete proto.hasPointerCapture
    }
  }
  private release(): void {
    const captured = this.captured
    this.captured = null
    if (captured) this.fire(captured, 'lostpointercapture', { bubbles: true, pointerId: STREAM_POINTER_ID } as PointerEventInit, 'pointer')
  }
}

function common(a: Element, b: Element): Element | null {
  for (let node: Element | null = a; node; node = node.parentElement) if (node.contains(b)) return node
  return null
}
function isTextField(el: Element, win: Win): el is HTMLInputElement | HTMLTextAreaElement {
  return el instanceof win.HTMLTextAreaElement || (el instanceof win.HTMLInputElement && TEXT_TYPES.has(el.type))
}
function pressable(el: Element, win: Win): boolean {
  return el instanceof win.HTMLButtonElement || (el instanceof win.HTMLInputElement && ['checkbox', 'radio', 'button', 'submit', 'reset'].includes(el.type))
    || el.getAttribute('role') === 'button'
}
function activatedByEnter(el: Element, win: Win): boolean {
  return el instanceof win.HTMLButtonElement || (el instanceof win.HTMLAnchorElement && el.hasAttribute('href'))
    || (el instanceof win.HTMLInputElement && ['button', 'submit', 'reset'].includes(el.type)) || el.getAttribute('role') === 'button'
}
function scrollable(from: Element, win: Win, axis: 'x' | 'y'): Element | null {
  for (let node: Element | null = from; node; node = node.parentElement) {
    const style = win.getComputedStyle(node), overflow = axis === 'y' ? style.overflowY : style.overflowX
    const room = axis === 'y' ? node.scrollHeight > node.clientHeight : node.scrollWidth > node.clientWidth
    if (room && (overflow === 'auto' || overflow === 'scroll')) return node
  }
  return null
}
/** Number fields have no selection to insert at; everything else does. */
function selectionStart(el: HTMLInputElement | HTMLTextAreaElement): number | null { try { return el.selectionStart } catch { return null } }
function selectionEnd(el: HTMLInputElement | HTMLTextAreaElement): number | null { try { return el.selectionEnd } catch { return null } }
function selectionLength(el: HTMLInputElement | HTMLTextAreaElement): number {
  const start = selectionStart(el), end = selectionEnd(el)
  return start === null || end === null ? 0 : end - start
}
function selectAll(el: HTMLInputElement | HTMLTextAreaElement): void { try { el.setSelectionRange(0, el.value.length) } catch { /* no selection here */ } }
function moveCaret(el: HTMLInputElement | HTMLTextAreaElement, key: string): void {
  const start = selectionStart(el), end = selectionEnd(el)
  if (start === null || end === null) return
  const to = key === 'Home' ? 0 : key === 'End' ? el.value.length : key === 'ArrowLeft' ? (start === end ? Math.max(0, start - 1) : start) : (start === end ? Math.min(el.value.length, end + 1) : end)
  el.setSelectionRange(to, to)
}
/** The element's own value setter, not the instance's: React tracks a controlled field's value on the
 *  instance, and a change set through it would read to React as no change at all. */
function setValue(el: HTMLInputElement | HTMLTextAreaElement, value: string, win: Win): void {
  const proto = el instanceof win.HTMLTextAreaElement ? win.HTMLTextAreaElement.prototype : win.HTMLInputElement.prototype
  Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value)
}

/** Listen for the station's input in the main window. Inert without the Tauri event bridge (a plain
 *  browser preview), and inert until a stream is admitted, since nothing else emits the event. */
export function installStreamInput(win: Win = window): () => void {
  const listen = win.__TAURI__?.event?.listen
  if (!listen) return () => {}
  const dispatcher = new StreamInputDispatcher(win, pttKey => { reportStreamPttKey(pttKey).catch(() => {}) })
  let unlisten: (() => void) | undefined, alive = true
  void listen<unknown>(STREAM_INPUT_EVENT, event => dispatcher.handle(event.payload)).then(
    un => { if (alive) unlisten = un; else un() },
    () => { /* a listener that fails to register must not take the app down */ })
  return () => { alive = false; unlisten?.(); dispatcher.dispose() }
}

// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { useState } from 'react'
import { cleanup, render, screen, within } from '@testing-library/react'
import { installStreamInput, STREAM_INPUT_EVENT, STREAM_POINTER_ID, StreamInputDispatcher } from './stream-input'

// THE CONTRACT: what the station hands this window, from the files its Rust side is tested against.
type Case = { name: string; message: Record<string, unknown> }
const WEBVIEW = JSON.parse(readFileSync(resolve(process.cwd(), '../remote/test/fixtures/stream/webview.json'), 'utf8')) as Record<string, Case[]>
const byName = (name: string) => structuredClone(WEBVIEW.stationToWebview.find(c => c.name === name)!.message)

// jsdom never lays out: `elementFromPoint` does not exist and every box is empty. Each test says
// which element sits under the pointer, and gives the elements it measures a box.
let under: Element | null = null
beforeEach(() => {
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
})
let dispatcher: StreamInputDispatcher | null = null
afterEach(() => { dispatcher?.dispose(); dispatcher = null; cleanup(); delete (document as { elementFromPoint?: unknown }).elementFromPoint })
const bridge = () => (dispatcher = new StreamInputDispatcher(window))
const pointer = (action: string, extra: Record<string, unknown> = {}) =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: action === 'move' ? -1 : 0, buttons: action === 'down' || action === 'move' ? 1 : 0, modifiers: 0, pointerType: 'mouse', clicks: action === 'move' ? 0 : 1, ...extra })
const key = (action: 'down' | 'up', keyValue: string, code: string, modifiers = 0) => ({ type: 'key', action, key: keyValue, code, modifiers, repeat: false })
const click = (d: StreamInputDispatcher, extra: Record<string, unknown> = {}) => { d.handle(pointer('down', extra)); d.handle(pointer('up', extra)) }
const box = (el: Element, rect: { left: number; top: number; width: number; height: number }) => {
  el.getBoundingClientRect = () => ({ ...rect, right: rect.left + rect.width, bottom: rect.top + rect.height, x: rect.left, y: rect.top, toJSON: () => ({}) })
}

it('handles every input the contract hands this window, and nothing it refuses', () => {
  const seen: string[] = []
  const types = ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'wheel', 'keydown', 'keyup', 'input']
  for (const type of types) window.addEventListener(type, event => seen.push(event.type), true)
  const field = document.body.appendChild(document.createElement('input'))
  field.focus()
  under = field
  const d = bridge()
  for (const refused of WEBVIEW.refused) d.handle(refused.message)
  expect(seen, 'nothing the contract refuses reaches the page').toEqual([])
  for (const accepted of WEBVIEW.stationToWebview) {
    const before = seen.length
    d.handle(accepted.message)
    if (accepted.message.type !== 'reset') expect(seen.length, accepted.name).toBeGreaterThan(before)
  }
  expect(field.value, 'the committed text landed in the focused field').toContain('W1AW')
})

it('A2: produces DOM events and nothing else - no command, no window, no request', () => {
  const invoke = vi.fn(), coreInvoke = vi.fn(), open = vi.spyOn(window, 'open').mockImplementation(() => null)
  const tauri = window as unknown as { __TAURI_INTERNALS__?: unknown; __TAURI__?: unknown }
  tauri.__TAURI_INTERNALS__ = { invoke }; tauri.__TAURI__ = { core: { invoke: coreInvoke } }
  const request = vi.fn(); vi.stubGlobal('fetch', request)
  try {
    const target = document.body.appendChild(document.createElement('button'))
    const events: string[] = []
    for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'keydown', 'keyup', 'wheel']) target.addEventListener(type, e => events.push(e.type))
    under = target
    target.focus()
    const d = bridge()
    for (const accepted of WEBVIEW.stationToWebview) d.handle(accepted.message)
    d.reset()
    expect(invoke).not.toHaveBeenCalled()
    expect(coreInvoke).not.toHaveBeenCalled()
    expect(open).not.toHaveBeenCalled()
    expect(request).not.toHaveBeenCalled()
    // Positive control: the same run did reach the page, as DOM events on the element under the pointer.
    expect(events).toEqual(expect.arrayContaining(['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'keydown', 'keyup', 'wheel']))
  } finally { delete tauri.__TAURI_INTERNALS__; delete tauri.__TAURI__; vi.unstubAllGlobals(); open.mockRestore() }
})

it('turns a press and release into the click a React handler sees, and a second click into a double click', () => {
  const onClick = vi.fn(), onDoubleClick = vi.fn()
  render(<button onClick={onClick} onDoubleClick={onDoubleClick}>Tune</button>)
  under = screen.getByRole('button', { name: 'Tune' })
  const d = bridge()
  click(d)
  expect(onClick).toHaveBeenCalledTimes(1)
  click(d, { clicks: 2 })
  expect(onClick).toHaveBeenCalledTimes(2)
  expect(onDoubleClick).toHaveBeenCalledTimes(1)
  // A press on one control released over another is not a click on either.
  const other = document.body.appendChild(document.createElement('div'))
  d.handle(pointer('down'))
  under = other
  d.handle(pointer('up'))
  expect(onClick).toHaveBeenCalledTimes(2)
})

it('moves focus to what was pressed, and away from a field when the press lands on nothing focusable', () => {
  render(<><input aria-label="Call" /><p>Band activity</p></>)
  const field = screen.getByLabelText('Call')
  under = field
  const d = bridge()
  click(d)
  expect(document.activeElement).toBe(field)
  under = screen.getByText('Band activity')
  click(d)
  expect(document.activeElement).toBe(document.body)
})

function Callsign({ onSubmit }: { onSubmit: () => void }) {
  const [call, setCall] = useState('')
  return <form onSubmit={event => { event.preventDefault(); onSubmit() }}>
    <input aria-label="Call" value={call} onChange={event => setCall(event.target.value.toUpperCase())} />
    <output>{call}</output>
  </form>
}
it('types into a React-controlled field, deletes, and submits its form on Enter', () => {
  const submitted = vi.fn()
  render(<Callsign onSubmit={submitted} />)
  const field = screen.getByLabelText('Call') as HTMLInputElement
  under = field
  const d = bridge()
  click(d)
  for (const character of 'w1aw') { d.handle(key('down', character, `Key${character.toUpperCase()}`)); d.handle(key('up', character, `Key${character.toUpperCase()}`)) }
  expect(field.value).toBe('W1AW')
  expect(screen.getByRole('status').textContent, 'React state followed the typing').toBe('W1AW')
  d.handle(key('down', 'Backspace', 'Backspace'))
  expect(field.value).toBe('W1A')
  d.handle({ type: 'text', text: 'X' })
  expect(field.value).toBe('W1AX')
  d.handle(key('down', 'Enter', 'Enter'))
  expect(submitted).toHaveBeenCalledTimes(1)
})

it('presses a focused button with Space on key-up, unless the page cancelled the key-down', () => {
  const onClick = vi.fn()
  render(<button onClick={onClick}>Log QSO</button>)
  const button = screen.getByRole('button', { name: 'Log QSO' })
  button.focus()
  const d = bridge()
  d.handle(key('down', ' ', 'Space'))
  expect(onClick).not.toHaveBeenCalled()
  d.handle(key('up', ' ', 'Space'))
  expect(onClick).toHaveBeenCalledTimes(1)
  // Space that went down somewhere else - focus moved onto the button while it was held - presses nothing.
  button.blur()
  d.handle(key('down', ' ', 'Space'))
  button.focus()
  d.handle(key('up', ' ', 'Space'))
  expect(onClick).toHaveBeenCalledTimes(1)
  // A window handler that takes Space for itself (the Phone cockpit's PTT) cancels the key-down.
  const take = (event: KeyboardEvent) => { if (event.code === 'Space') event.preventDefault() }
  window.addEventListener('keydown', take)
  try {
    d.handle(key('down', ' ', 'Space'))
    d.handle(key('up', ' ', 'Space'))
    expect(onClick).toHaveBeenCalledTimes(1)
  } finally { window.removeEventListener('keydown', take) }
})

function Band() {
  const [band, setBand] = useState('40m')
  return <><select aria-label="Band" value={band} onChange={event => setBand(event.target.value)}>
    <option>80m</option><option>40m</option><option disabled>30m</option><option>20m</option>
  </select><output>{band}</output></>
}
it('opens an in-page list for a select, chooses from it with a click, and steps with the arrow keys', () => {
  render(<Band />)
  const select = screen.getByLabelText('Band') as HTMLSelectElement
  box(select, { left: 100, top: 100, width: 120, height: 30 })
  under = select
  const d = bridge()
  click(d)
  const list = screen.getByRole('listbox')
  expect(within(list).getAllByRole('option').map(o => o.textContent)).toEqual(['80m', '40m', '30m', '20m'])
  under = within(list).getByRole('option', { name: '20m' })
  click(d)
  expect(select.value).toBe('20m')
  expect(screen.getByRole('status').textContent, 'React saw the change').toBe('20m')
  expect(list.isConnected, 'the list closes once chosen').toBe(false)
  expect(document.activeElement).toBe(select)
  // Keyboard: up skips the disabled option.
  d.handle(key('down', 'ArrowUp', 'ArrowUp'))
  expect(select.value).toBe('40m')
  // A press anywhere else closes an open list without choosing.
  under = select
  click(d)
  under = document.body
  d.handle(pointer('down'))
  expect(screen.queryByRole('listbox')).toBeNull()
  expect(select.value).toBe('40m')
})

it('drags a range slider to where the pointer is, and commits it on release', () => {
  const slider = document.body.appendChild(document.createElement('input'))
  Object.assign(slider, { type: 'range', min: '0', max: '100', step: '5', value: '50' })
  const events: string[] = []
  slider.addEventListener('input', () => events.push(`input ${slider.value}`))
  slider.addEventListener('change', () => events.push(`change ${slider.value}`))
  box(slider, { left: 0, top: 0, width: window.innerWidth, height: 20 })
  under = slider
  const d = bridge()
  d.handle(pointer('down', { x: 0.76 }))
  d.handle(pointer('move', { x: 0.24 }))
  expect(events, 'the value follows the drag; nothing is committed yet').toEqual(['input 75', 'input 25'])
  d.handle(pointer('up', { x: 0.24 }))
  expect(events).toEqual(['input 75', 'input 25', 'change 25'])
})

it('scrolls the nearest scroller a wheel is over, unless a handler takes the wheel, and never on Ctrl', () => {
  const scroller = document.body.appendChild(document.createElement('div'))
  scroller.style.overflowY = 'auto'
  Object.defineProperties(scroller, { scrollHeight: { value: 1000 }, clientHeight: { value: 200 } })
  const inner = scroller.appendChild(document.createElement('div'))
  const scrollBy = vi.fn()
  scroller.scrollBy = scrollBy as never
  under = inner
  const d = bridge()
  d.handle({ ...byName('wheel'), deltaMode: 1, deltaY: 3 })
  expect(scrollBy).toHaveBeenCalledWith(expect.objectContaining({ top: 48 }))
  d.handle({ ...byName('wheel'), modifiers: 2 })
  expect(scrollBy).toHaveBeenCalledTimes(1)
  inner.addEventListener('wheel', event => event.preventDefault())
  d.handle(byName('wheel'))
  expect(scrollBy).toHaveBeenCalledTimes(1)
})

it('gives a drag handler pointer capture for the stream\'s pointer, and routes the drag to it', () => {
  const handle = document.body.appendChild(document.createElement('div'))
  const elsewhere = document.body.appendChild(document.createElement('div'))
  const moves: EventTarget[] = [], lost = vi.fn()
  handle.addEventListener('pointerdown', event => handle.setPointerCapture((event as PointerEvent).pointerId))
  handle.addEventListener('pointermove', event => moves.push(event.currentTarget!))
  handle.addEventListener('lostpointercapture', lost)
  under = handle
  const d = bridge()
  expect(() => d.handle(pointer('down'))).not.toThrow()
  expect(handle.hasPointerCapture(STREAM_POINTER_ID)).toBe(true)
  under = elsewhere
  d.handle(pointer('move', { x: 0.9 }))
  expect(moves).toEqual([handle])
  d.handle(pointer('up', { x: 0.9 }))
  expect(lost).toHaveBeenCalledTimes(1)
  expect(handle.hasPointerCapture(STREAM_POINTER_ID)).toBe(false)
})

it('fires enter and leave as the pointer moves from one control to another', () => {
  const log: string[] = []
  render(<><button onMouseEnter={() => log.push('enter A')} onMouseLeave={() => log.push('leave A')}>A</button>
    <button onMouseEnter={() => log.push('enter B')} onMouseLeave={() => log.push('leave B')}>B</button></>)
  const d = bridge()
  under = screen.getByRole('button', { name: 'A' })
  d.handle(pointer('move', { buttons: 0 }))
  under = screen.getByRole('button', { name: 'B' })
  d.handle(pointer('move', { buttons: 0 }))
  expect(log).toEqual(['enter A', 'leave A', 'enter B'])
})

it('reset releases what the stream was holding: Space comes up, the button comes up, and no click fires', () => {
  const windowKeys: string[] = [], onClick = vi.fn()
  const listen = (event: KeyboardEvent) => windowKeys.push(`${event.type} ${event.code}`)
  window.addEventListener('keydown', listen); window.addEventListener('keyup', listen)
  try {
    render(<button onClick={onClick}>PTT</button>)
    under = screen.getByRole('button', { name: 'PTT' })
    const d = bridge()
    // The Phone cockpit keys on Space down and unkeys on Space up, both on the window.
    d.handle(byName("key down, Space, the Phone cockpit's PTT key"))
    d.handle(pointer('down'))
    d.handle({ type: 'reset' })
    expect(windowKeys).toEqual(['keydown Space', 'keyup Space'])
    expect(onClick, 'a released press is not a click').not.toHaveBeenCalled()
    // Control: nothing left to release, so a second reset sends nothing.
    d.handle({ type: 'reset' })
    expect(windowKeys).toHaveLength(2)
    // Space held ON the button when the stream ends: the press is abandoned, as a browser abandons it
    // when the window loses focus mid-press - it never becomes a click.
    screen.getByRole('button', { name: 'PTT' }).focus()
    d.handle(byName("key down, Space, the Phone cockpit's PTT key"))
    d.handle({ type: 'reset' })
    expect(windowKeys.slice(2)).toEqual(['keydown Space', 'keyup Space'])
    expect(onClick).not.toHaveBeenCalled()
  } finally { window.removeEventListener('keydown', listen); window.removeEventListener('keyup', listen) }
})

it('listens only through the event bridge, on the contract\'s event, and undoes everything when removed', async () => {
  expect(installStreamInput(window)).toBeTypeOf('function')
  const handlers = new Map<string, (event: { payload: unknown }) => void>(), unlisten = vi.fn()
  const tauri = window as unknown as { __TAURI__?: unknown }
  tauri.__TAURI__ = { event: { listen: (name: string, handler: (event: { payload: unknown }) => void) => { handlers.set(name, handler); return Promise.resolve(unlisten) } } }
  try {
    const remove = installStreamInput(window)
    await Promise.resolve()
    expect([...handlers.keys()]).toEqual([STREAM_INPUT_EVENT])
    const onClick = vi.fn()
    render(<button onClick={onClick}>Stop TX</button>)
    under = screen.getByRole('button', { name: 'Stop TX' })
    handlers.get(STREAM_INPUT_EVENT)!({ payload: pointer('down') })
    handlers.get(STREAM_INPUT_EVENT)!({ payload: pointer('up') })
    expect(onClick).toHaveBeenCalledTimes(1)
    remove()
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect((Element.prototype as { setPointerCapture?: unknown }).setPointerCapture, 'the capture patch is gone with it').toBeUndefined()
  } finally { delete tauri.__TAURI__ }
})

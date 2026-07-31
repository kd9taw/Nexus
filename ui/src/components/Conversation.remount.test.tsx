// @vitest-environment jsdom
//
// The Tempo chat pane is a CHAT CLIENT (Trillian ruling): pinned-at-bottom
// follows new messages; an operator scrolled up reading history is never
// yanked; and switching peers opens the new conversation pinned to its newest
// message. The old implementation re-pinned on message COUNT alone — switching
// to a same-length conversation kept the previous peer's scroll offset, and a
// new inbound yanked a reading operator to the bottom. App remounts this
// component per peer (key=) so each conversation's scroll state is its own.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { Conversation } from './Conversation'
import type { ChatMessage, Conversation as Conv, RadioStatus, Settings } from '../types'

const msg = (text: string, slot: number): ChatMessage => ({
  from: 'N9UM',
  to: 'KD9TAW',
  text,
  slot,
  directedToMe: true,
  outbound: false,
  snr: -5,
  freqHz: 1500,
  dtSec: 0.1,
  tier: null,
})

const conv = (peer: string, n: number): Conv => ({
  peer,
  messages: Array.from({ length: n }, (_, i) => msg(`${peer} msg ${i}`, i)),
})

const macros: Settings['macros'] = { chat: [], qso: [], band: [] }
const radio = { transmitting: false } as RadioStatus

function pane(peer: string, c: Conv) {
  return (
    <Conversation
      key={peer}
      conversation={c}
      peer={peer}
      radio={radio}
      mode="chat"
      fieldDay={null}
      macros={macros}
      onSend={() => {}}
      onBroadcast={() => {}}
      onCallCq={() => {}}
      beaconOn={false}
      onToggleBeacon={() => {}}
      mycall="KD9TAW"
      mygrid="EN52"
    />
  )
}

/** Stub scroll geometry on a jsdom element (no layout engine). */
function stubGeometry(el: HTMLElement, geo: { scrollHeight: number; clientHeight: number }) {
  Object.defineProperty(el, 'scrollHeight', { configurable: true, get: () => geo.scrollHeight })
  Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => geo.clientHeight })
  return geo
}

afterEach(cleanup)

describe('Conversation scroll discipline', () => {
  it('a reading operator is never yanked by a new inbound message', () => {
    const { container, rerender } = render(pane('N9UM', conv('N9UM', 5)))
    const scroll = container.querySelector('.message-scroll') as HTMLElement
    const geo = stubGeometry(scroll, { scrollHeight: 800, clientHeight: 100 })
    // Scrolled up to read history — well past the slop.
    scroll.scrollTop = 200
    fireEvent.scroll(scroll)
    // A new inbound lands (message count changes — the old code's yank trigger).
    geo.scrollHeight = 900
    rerender(pane('N9UM', conv('N9UM', 6)))
    expect(scroll.scrollTop).toBe(200)
  })

  it('switching peers remounts the pane and opens it pinned to the bottom', () => {
    const { container, rerender } = render(pane('N9UM', conv('N9UM', 5)))
    const scrollA = container.querySelector('.message-scroll') as HTMLElement
    stubGeometry(scrollA, { scrollHeight: 800, clientHeight: 100 })
    scrollA.scrollTop = 200
    fireEvent.scroll(scrollA) // peer A left mid-history
    // SAME message count — the exact case the count-keyed effect missed.
    rerender(pane('W1AW', conv('W1AW', 5)))
    const scrollB = container.querySelector('.message-scroll') as HTMLElement
    // key= made it a fresh element: no inherited offset, no shared pin state.
    expect(scrollB).not.toBe(scrollA)
    stubGeometry(scrollB, { scrollHeight: 600, clientHeight: 100 })
    // The next render finds the fresh pane pinned and snaps it to the newest
    // message (the mount-time snap ran before geometry was stubbed). SAME
    // count again — a count-keyed effect would sit still here.
    rerender(pane('W1AW', conv('W1AW', 5)))
    expect(scrollB.scrollTop).toBe(600)
  })
})

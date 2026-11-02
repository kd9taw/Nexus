// @vitest-environment jsdom
//
// RTTY'S AND PSK'S DOCKS AT 8, 12 AND 16 MACRO KEYS (Settings ▸ RTTY / PSK ▸ Macro keys,
// `macros.rttyMacroCount` / `pskMacroCount`). The count model is `features/macroCount.test.ts`; the
// layout is computed in `cockpit-panes.test.ts`. This file is the wiring, on the seams:
//   · the dock shows the count of keys — at 8 exactly the grid it always drew;
//   · F9–F12 send keys 9–12 at 12 and 16 through the SAME `send()` → `rtty_send` / `psk_send` a
//     click does, and nothing at 8; keys 13–16 are sent by a click only, show no key name and have
//     no shortcut;
//   · the editor edits every key that shows, and saves through the cockpit's one-writer verb;
//   · a saved key past the count is not shown and not sent, and comes back with the count.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import * as api from '../api'
import { frameForAir } from '../features/rttyMacros'
import type { AppSnapshot, PskState, RttyState, Settings } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  getPskState: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  pskArm: vi.fn(),
  pskAutoArm: vi.fn(),
  pskClear: vi.fn(),
  pskAfcReset: vi.fn(),
  pskNet: vi.fn(),
  pskSend: vi.fn(),
  pskSetLatched: vi.fn(),
  pskSetMode: vi.fn(),
  pskType: vi.fn(),
  pskStop: vi.fn(),
  haltTx: vi.fn(),
  atuTune: vi.fn(),
  setRfPower: vi.fn(),
  setTune: vi.fn(),
  setRttyMacros: vi.fn(),
  setPskMacros: vi.fn(),
  setSettings: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const mocked = <T,>(f: T) => f as unknown as ReturnType<typeof vi.fn>

const RTTY_IDLE = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, markHz: 2125,
  spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null, auto: false, seqState: 'idle',
  peer: null, peerExchange: [], heardCq: null,
} as unknown as RttyState
const PSK_IDLE = {
  armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false,
  keyerError: null,
} as unknown as PskState

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

type Macros = Settings['macros']

/** Each mode as this file drives it: its cockpit, its slot attribute, its count field, its sets'
 *  fields, its send seam and its save seam, and what a key's message looks like on its air. */
const MODES = [
  {
    mode: 'RTTY',
    attr: 'data-rtty-macro',
    group: 'RTTY macros',
    render: (macros: Macros) => <RttyCockpit snap={snap} active macros={macros} />,
    sets: (macros: Macros['rttyProfiles'], count?: number): Macros => ({
      chat: [], qso: [], band: [], rttyProfiles: macros, activeRttyProfile: '', ...(count ? { rttyMacroCount: count } : {}),
    }),
    send: () => mocked(api.rttySend),
    save: () => mocked(api.setRttyMacros),
    onAir: (text: string) => frameForAir(text),
  },
  {
    mode: 'PSK',
    attr: 'data-psk-macro',
    group: 'PSK macros',
    render: (macros: Macros) => <PskCockpit snap={snap} active macros={macros} />,
    sets: (macros: Macros['pskProfiles'], count?: number): Macros => ({
      chat: [], qso: [], band: [], pskProfiles: macros, activePskProfile: '', ...(count ? { pskMacroCount: count } : {}),
    }),
    send: () => mocked(api.pskSend),
    save: () => mocked(api.setPskMacros),
    onAir: (text: string) => text,
  },
] as const

/** Every key F1–F16 saved in Everyday, each saying which it is. */
const ALL_SIXTEEN = [
  { name: 'everyday', macros: Array.from({ length: 16 }, (_, i) => ({ key: `F${i + 1}`, label: `K${i + 1}`, text: `MSG ${i + 1}` })) },
]

beforeEach(() => {
  mocked(api.getRttyState).mockReset().mockImplementation(async () => RTTY_IDLE)
  mocked(api.rttyAutoArm).mockReset().mockImplementation(async () => RTTY_IDLE)
  mocked(api.getPskState).mockReset().mockImplementation(async () => PSK_IDLE)
  mocked(api.pskAutoArm).mockReset().mockImplementation(async () => PSK_IDLE)
  mocked(api.rttySend).mockReset().mockImplementation(async () => ({ ...RTTY_IDLE, sending: true }))
  mocked(api.pskSend).mockReset().mockImplementation(async () => ({ ...PSK_IDLE, sending: true }))
  mocked(api.setRttyMacros)
    .mockReset()
    .mockImplementation(async (profiles: unknown, active: unknown) => ({ chat: [], qso: [], band: [], rttyProfiles: profiles, activeRttyProfile: active }))
  mocked(api.setPskMacros)
    .mockReset()
    .mockImplementation(async (profiles: unknown, active: unknown) => ({ chat: [], qso: [], band: [], pskProfiles: profiles, activePskProfile: active }))
  mocked(api.setSettings).mockReset()
})
afterEach(cleanup)

async function flush() {
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}

for (const m of MODES) {
  const slots = () => [...document.querySelectorAll(`[${m.attr}]`)].map((el) => el.getAttribute(m.attr))
  const key = (k: string) => document.querySelector(`[${m.attr}="${k}"] .cw-macro`) as HTMLButtonElement
  const corner = (k: string) => key(k).querySelector('.cw-macro-key')!.textContent
  const pencil = (k: string) => document.querySelector(`[${m.attr}="${k}"] .rtty-macro-edit`) as HTMLButtonElement
  const press = (k: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(window, { key: k, ...init })
  const sentTexts = () => m.send().mock.calls.map((c) => c[0])

  describe(`${m.mode}'s dock at 8, 12 and 16 keys`, () => {
    it('shows eight keys with no count saved — the grid it always drew, in one group', async () => {
      render(m.render(m.sets(ALL_SIXTEEN)))
      await flush()
      expect(slots()).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
      const group = screen.getByRole('group', { name: m.group })
      expect(group.className.split(' ')).toContain('rtty-macros')
      expect(group.querySelector('.macro-quad'), 'eight keys are not grouped').toBeNull()
      expect([...group.children].every((c) => c.hasAttribute(m.attr)), 'the keys are the grid\'s own children').toBe(true)
    })

    for (const n of [12, 16]) {
      it(`shows ${n} keys at ${n}, in groups of four, in keyboard order, in one group`, async () => {
        render(m.render(m.sets(ALL_SIXTEEN, n)))
        await flush()
        const want = Array.from({ length: n }, (_, i) => `F${i + 1}`)
        expect(slots()).toEqual(want)
        const group = screen.getByRole('group', { name: m.group })
        expect(group.className.split(' ')).toEqual(expect.arrayContaining(['macro-bank', `macro-bank-shared-${n}`]))
        const quads = [...group.querySelectorAll(':scope > .macro-quad')]
        expect(quads.map((q) => [...q.children].map((c) => c.getAttribute(m.attr)))).toEqual(
          Array.from({ length: n / 4 }, (_, g) => want.slice(g * 4, g * 4 + 4)),
        )
      })
    }

    it('names F1–F12 on their keys and keys 13–16 not at all; only F1–F12 carry the edit shortcut', async () => {
      render(m.render(m.sets(ALL_SIXTEEN, 16)))
      await flush()
      for (let i = 1; i <= 12; i++) {
        expect(corner(`F${i}`)).toBe(`F${i}`)
        expect(pencil(`F${i}`).getAttribute('aria-keyshortcuts')).toBe(`Control+Shift+F${i}`)
      }
      for (let i = 13; i <= 16; i++) {
        expect(corner(`F${i}`)?.trim(), `key ${i} shows a key name`).toBe('')
        expect(pencil(`F${i}`).hasAttribute('aria-keyshortcuts'), `key ${i} advertises a shortcut`).toBe(false)
        expect(key(`F${i}`).title).toContain('Click to send')
        expect(key(`F${i}`).title).not.toContain(`F${i}`)
        expect(pencil(`F${i}`).getAttribute('aria-label')).toContain(`#${i}`)
      }
    })

    it('F9–F12 send keys 9–12 at 12 through the one send path, as a click does', async () => {
      render(m.render(m.sets(ALL_SIXTEEN, 12)))
      await flush()
      for (const k of ['F9', 'F10', 'F11', 'F12']) {
        const pressed = press(k)
        expect(pressed, `${k}'s default was left to the webview`).toBe(false)
        await flush()
      }
      expect(sentTexts()).toEqual([9, 10, 11, 12].map((i) => m.onAir(`MSG ${i}`)))
      // …the same bytes a click sends.
      m.send().mockClear()
      fireEvent.click(key('F11'))
      await flush()
      expect(sentTexts()).toEqual([m.onAir('MSG 11')])
    })

    it('F9–F12 send nothing at 8 and are left to the system, as they were', async () => {
      render(m.render(m.sets(ALL_SIXTEEN)))
      await flush()
      for (const k of ['F9', 'F10', 'F11', 'F12']) expect(press(k), `${k} was taken at 8`).toBe(true)
      await flush()
      expect(m.send()).not.toHaveBeenCalled()
      // Control: F8 still sends.
      press('F8')
      await flush()
      expect(sentTexts()).toEqual([m.onAir('MSG 8')])
    })

    it('keys 13–16 are sent by a click only — never by a key, F13 included — and Shift+F10 is the system\'s', async () => {
      render(m.render(m.sets(ALL_SIXTEEN, 16)))
      await flush()
      for (const k of ['F13', 'F14', 'F15', 'F16']) expect(press(k), `${k} was taken`).toBe(true)
      expect(press('F10', { shiftKey: true }), 'Shift+F10 (the context menu) was taken').toBe(true)
      for (const mod of ['altKey', 'ctrlKey', 'metaKey']) press('F12', { [mod]: true })
      await flush()
      expect(m.send()).not.toHaveBeenCalled()
      fireEvent.click(key('F15'))
      await flush()
      expect(sentTexts()).toEqual([m.onAir('MSG 15')])
    })

    it('the editor edits a key past F8: Ctrl+Shift+F10 opens F10, the ✎ opens key 16, and Save writes it', async () => {
      render(m.render(m.sets([], 16)))
      await flush()
      // An empty key past the built-ins opens its editor on a click, as an empty F5 does.
      act(() => {
        press('F10', { ctrlKey: true, shiftKey: true })
      })
      expect(document.querySelector('.rtty-macro-editor'), 'Ctrl+Shift+F10 opened no editor').not.toBeNull()
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
      fireEvent.click(key('F16'))
      expect(document.querySelector('.rtty-macro-editor')?.getAttribute('aria-label')).toContain('#16')
      fireEvent.change(screen.getByLabelText(/^title$/i), { target: { value: 'Late' } })
      fireEvent.change(screen.getByLabelText(/^message$/i), { target: { value: 'QRL TNX' } })
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      })
      await flush()
      expect(m.save()).toHaveBeenCalledTimes(1)
      expect(m.save().mock.calls[0][0]).toEqual([{ name: 'everyday', macros: [{ key: 'F16', label: 'Late', text: 'QRL TNX' }] }])
      expect(api.setSettings, 'the settings form wrote a macro').not.toHaveBeenCalled()
      expect(m.send(), 'Save keyed the rig').not.toHaveBeenCalled()
    })

    it('hides the saved keys past the count, keeps them, and shows them again with the count', async () => {
      const { rerender } = render(m.render(m.sets(ALL_SIXTEEN, 12)))
      await flush()
      expect(slots()).toHaveLength(12)
      rerender(m.render(m.sets(ALL_SIXTEEN)))
      await flush()
      expect(slots()).toHaveLength(8)
      rerender(m.render(m.sets(ALL_SIXTEEN, 16)))
      await flush()
      expect(slots()).toHaveLength(16)
      expect(key('F16').querySelector('.cw-macro-label')!.textContent).toBe('K16')
      expect(m.save(), 'changing the count wrote the sets').not.toHaveBeenCalled()
    })
  })
}

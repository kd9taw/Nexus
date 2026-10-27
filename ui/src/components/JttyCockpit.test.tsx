// @vitest-environment jsdom
//
// THE JTTY SCREEN, AS THE ENGINE FEEDS IT. The cockpit shows the engine's lines in WSJT-X's two
// panes (All Decodes: every message received, where it started; QSO Frequency: the ones first
// heard within F Tol of Rx, and what this station sent), WSJT-X's two display options (lower case,
// the start time in front), the engine's notice, and sends the receive controls the engine owns
// (auto-arm on entry, Arm, Erase, Rx and F Tol, the waterfall's range). The dock is WSJT-X's
// transmitter, wired to the engine's send commands; the engine's JTTY TX gate decides whether a
// send goes out, and its refusal is shown to the operator.
//
// The log strip is a stub here that shows what the cockpit hands it; the lookup it then makes is
// JttyCockpit.lookup.test.tsx's, with the real strip.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { JttyCockpit } from './JttyCockpit'
import type { AppSnapshot, JttyLine, JttyState } from '../types'
import * as api from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
// The header's own furniture (readout, digit tuning, the ⊞ menu) is not this file's: it draws the
// slots the cockpit hands it, which is where Rx and F Tol live.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator: ReactNode; bandControl: ReactNode; children?: ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.bandControl}
      {p.children}
    </header>
  ),
}))
/** The waterfall's props as last rendered: the test plays the strip's part (a click, a zoom). */
type WfTarget = 'tx' | 'rx' | 'both'
const wf = vi.hoisted(() => ({
  props: null as null | {
    onTune?: (hz: number, target: 'tx' | 'rx' | 'both') => void
    onViewChange?: (lo: number, hi: number) => void
    onRowDoubleClick?: (utcMs: number) => void
    rxOffsetHz: number
    txOffsetHz: number
  },
}))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { onTune?: (hz: number, target: WfTarget) => void; onViewChange?: (lo: number, hi: number) => void; onRowDoubleClick?: (utcMs: number) => void; rxOffsetHz: number; txOffsetHz: number }) => {
    wf.props = p
    return <div className="waterfall-wrap" />
  },
}))
vi.mock('./RfScopePane', () => ({ RfScopePane: () => null }))
/** The strip as the stub draws it: the call it was filled with, and the last props it was given
 *  (the Log QSO prompt's request and where its outcome goes). The real strip's handling of a
 *  request is tested against the real strip (`JttyCockpit.lookup.test.tsx`). */
type StubProps = {
  cwLive?: { call: string | null; confirmed: boolean } | null
  mode: string
  logOffsetHz?: number
  logRequest?: { id: number; call: string; claim: () => Promise<boolean> } | null
  onLogRequest?: (id: number, outcome: { kind: string; held?: string }) => void
}
const strip = vi.hoisted(() => ({ last: null as StubProps | null }))
vi.mock('./LogEntry', () => ({
  LogEntry: (p: StubProps) => {
    strip.last = p
    return (
      <div data-testid="log-stub" data-mode={p.mode} data-offset={p.logOffsetHz}>
        <span className="stub-call">{p.cwLive?.call ?? ''}</span>
      </div>
    )
  },
}))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const T0 = Date.UTC(2026, 9, 13, 22, 13, 20, 900) // 22:13:20.900 UTC

function line(over: Partial<JttyLine>): JttyLine {
  return {
    id: 1,
    startUtcMs: T0,
    freqHz: 1500,
    dialHz: 14_090_000,
    snr: -12,
    text: 'CQ K1ABC CQ',
    line: '1500 -12  CQ K1ABC CQ',
    state: 'complete',
    rx: true,
    confirmedCalls: [],
    ...over,
  }
}

function state(over: Partial<JttyState> = {}): JttyState {
  return { armed: true, rxHz: 1500, ftolHz: 20, lowHz: 200, highHz: 3000, lines: [], heard: [], notice: null, ...over }
}

const m = <K extends keyof typeof api>(k: K) => vi.mocked(api[k] as (...a: unknown[]) => Promise<unknown>)

async function mount(s: JttyState, active = true) {
  m('getJttyState').mockResolvedValue(s)
  m('jttyAutoArm').mockResolvedValue(s)
  m('jttyArm').mockResolvedValue(s)
  m('jttySetRx').mockResolvedValue(s)
  m('jttySetBand').mockResolvedValue(s)
  m('jttyClear').mockResolvedValue(state({ lines: [] }))
  m('getLicensedBandPlan').mockResolvedValue([])
  const r = render(
    <JttyCockpit snap={snap} active={active} onSetTxEnabled={() => {}} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

const pane = (id: 'all' | 'rx') => document.querySelector(`[data-pane="${id}"] .jtty-lines`) as HTMLElement
const paneLines = (id: 'all' | 'rx') => [...pane(id).querySelectorAll('.jtty-line')].map((el) => el.textContent)

beforeEach(() => {
  localStorage.clear()
  wf.props = null
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the JTTY cockpit shows what the engine decoded', () => {
  it('prints each line exactly as the engine (WSJT-X) wrote it, the QSO pane only the Rx ones, and the notice', async () => {
    const lines = [
      line({ id: 7, line: '1500 -12  CQ K1ABC CQ', rx: true }),
      // A message wrapped by WSJT-X: the engine's line carries the newline and the indent.
      line({ id: 8, startUtcMs: T0 + 4_000, freqHz: 1300, line: '1300  -3  THIS IS A LONG MESSAGE THAT WSJT X WRAPS\n  AT FORTY', rx: false }),
    ]
    await mount(state({ lines, notice: 'JTTY receive audio was lost; decoding restarted.' }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ', '1300  -3  THIS IS A LONG MESSAGE THAT WSJT X WRAPS\n  AT FORTY']))
    expect(paneLines('rx')).toEqual(['1500 -12  CQ K1ABC CQ'])
    const notice = document.querySelector('.jtty-notice')
    expect(notice?.getAttribute('role')).toBe('status')
    expect(notice?.textContent).toContain('JTTY receive audio was lost; decoding restarted.')
  })

  it('All decodes is in the order messages started, QSO frequency in the order they arrived', async () => {
    // The engine lists lines as they arrive; a retro search finds message 9 after 10, though it
    // started first.
    const lines = [
      line({ id: 10, startUtcMs: T0 + 6_000, line: '1500 -10  B', rx: true }),
      line({ id: 9, startUtcMs: T0 + 2_000, line: '1510 -11  A', rx: true }),
    ]
    await mount(state({ lines }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1510 -11  A', '1500 -10  B']))
    expect(paneLines('rx')).toEqual(['1500 -10  B', '1510 -11  A'])
  })

  it('WSJT-X’s two options: lower case, and the UTC start in front (hhmmss, truncated); both kept', async () => {
    await mount(state({ lines: [line({ id: 1, line: '1500 -12  CQ K1ABC CQ' })] }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ']))
    fireEvent.click(screen.getByRole('button', { name: t('jtty.lowerCase.label') }))
    expect(paneLines('all')).toEqual(['1500 -12  cq k1abc cq'])
    fireEvent.click(screen.getByRole('button', { name: t('jtty.includeTime.label') }))
    // 22:13:20.900 is 221320: Qt's hhmmss drops the milliseconds, it never rounds them.
    expect(paneLines('all')).toEqual(['221320 1500 -12  cq k1abc cq'])
    cleanup()
    await mount(state({ lines: [line({ id: 1, line: '1500 -12  CQ K1ABC CQ' })] }))
    await waitFor(() => expect(paneLines('all')).toEqual(['221320 1500 -12  cq k1abc cq']))
    expect(screen.getByRole('button', { name: t('jtty.lowerCase.label') }).getAttribute('aria-pressed')).toBe('true')
  })

  it('a click on a CONFIRMED call fills the log strip; a call heard once is plain text', async () => {
    const lines = [
      line({ id: 1, text: 'CQ W1AW CQ', line: '1500 -12  CQ W1AW CQ', confirmedCalls: ['W1AW'] }),
      line({ id: 2, startUtcMs: T0 + 3_000, text: 'CQ K1ABC CQ', line: '1520 -14  CQ K1ABC CQ', confirmedCalls: [] }),
    ]
    await mount(state({ lines }))
    await waitFor(() => expect(paneLines('all')).toHaveLength(2))
    const all = within(pane('all'))
    expect(all.queryByRole('button', { name: 'K1ABC' }), 'heard once: not a button').toBeNull()
    fireEvent.click(all.getByText('K1ABC', { exact: false }))
    expect(screen.getByTestId('log-stub').querySelector('.stub-call')?.textContent).toBe('')
    fireEvent.click(all.getByRole('button', { name: 'W1AW' }))
    expect(screen.getByTestId('log-stub').querySelector('.stub-call')?.textContent).toBe('W1AW')
    expect(screen.getByTestId('log-stub').dataset.mode).toBe('JTTY')
  })
})

describe('the JTTY log strip logs the frequency WSJT-X logs', () => {
  it('a contact logs at the dial plus JTTY’s Tx frequency, never PSK’s centre; a station that sends no transmitter logs the dial', async () => {
    await mount(state({ tx: txState({ txHz: 1800 }) }))
    await waitFor(() => expect(screen.getByTestId('log-stub').dataset.offset).toBe('1800'))
    cleanup()
    await mount(state())
    expect(screen.getByTestId('log-stub').dataset.offset, 'no transmitter: the dial, as before').toBeUndefined()
  })
})

describe('the JTTY cockpit drives the engine’s receive controls, and nothing that transmits', () => {
  it('auto-arms once on entry, never while hidden, and again on the next entry', async () => {
    const r = await mount(state(), false)
    expect(m('jttyAutoArm')).not.toHaveBeenCalled()
    r.rerender(
      <JttyCockpit snap={snap} active={true} onSetTxEnabled={() => {}} />,
    )
    await waitFor(() => expect(m('jttyAutoArm')).toHaveBeenCalledTimes(1))
    r.rerender(
      <JttyCockpit snap={snap} active={true} onSetTxEnabled={() => {}} />,
    )
    expect(m('jttyAutoArm')).toHaveBeenCalledTimes(1)
  })

  it('Arm/Stop and Erase go to the engine', async () => {
    await mount(state({ armed: true, lines: [line({})] }))
    fireEvent.click(screen.getByRole('button', { name: t('jtty.arm.on.label') }))
    expect(m('jttyArm')).toHaveBeenCalledWith(false)
    fireEvent.click(screen.getByRole('button', { name: t('jtty.erase.label') }))
    expect(m('jttyClear')).toHaveBeenCalledTimes(1)
  })

  it('Rx and F Tol: typed Rx on Enter, F Tol from WSJT-X’s list, a waterfall click sets Rx', async () => {
    await mount(state({ rxHz: 1500, ftolHz: 20 }))
    const rx = screen.getByRole('spinbutton', { name: t('jtty.rx.aria') }) as HTMLInputElement
    expect(rx.value).toBe('1500')
    fireEvent.focus(rx)
    fireEvent.change(rx, { target: { value: '1234' } })
    fireEvent.keyDown(rx, { key: 'Enter' })
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1234, 20)
    const ftol = screen.getByRole('combobox', { name: t('jtty.ftol.aria') }) as HTMLSelectElement
    expect([...ftol.options].map((o) => Number(o.value))).toEqual([2, 5, 10, 20, 50, 100, 150, 200, 250, 300, 350, 400, 450, 500])
    fireEvent.change(ftol, { target: { value: '50' } })
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1500, 50)
    await waitFor(() => expect(wf.props).not.toBeNull())
    act(() => wf.props!.onTune!(1312.6, 'rx'))
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1313, 20)
    expect(m('jttySetTx')).not.toHaveBeenCalled()
  })

  it('Tx: typed on Enter, Shift+click (or right-click) on the waterfall sets it, Ctrl+click both', async () => {
    await mount(state({ tx: txState({ txHz: 1500 }) }))
    m('jttySetTx').mockResolvedValue(state({ tx: txState({ txHz: 1700 }) }))
    const tx = screen.getByRole('spinbutton', { name: t('jtty.tx.aria') }) as HTMLInputElement
    expect(tx.value).toBe('1500')
    fireEvent.focus(tx)
    fireEvent.change(tx, { target: { value: '1700' } })
    fireEvent.keyDown(tx, { key: 'Enter' })
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1700)
    await waitFor(() => expect(wf.props).not.toBeNull())
    expect(wf.props!.txOffsetHz).toBe(1700)
    act(() => wf.props!.onTune!(1622.4, 'tx'))
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1622)
    expect(m('jttySetRx')).not.toHaveBeenCalled()
    act(() => wf.props!.onTune!(1405, 'both'))
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1405)
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1405, 20)
  })

  it('decode again: Decode decodes the kept audio, a waterfall row’s double-click from that row; the engine’s refusal is said', async () => {
    await mount(state())
    m('jttyDecodeAgain').mockResolvedValue(state())
    fireEvent.click(screen.getByRole('button', { name: t('jtty.decode.label') }))
    await flush()
    expect(m('jttyDecodeAgain')).toHaveBeenLastCalledWith(null)
    await waitFor(() => expect(wf.props?.onRowDoubleClick).toBeTruthy())
    act(() => wf.props!.onRowDoubleClick!(T0 + 16_000))
    await flush()
    expect(m('jttyDecodeAgain')).toHaveBeenLastCalledWith(T0 + 16_000)
    m('jttyDecodeAgain').mockRejectedValue('Audio for this waterfall row is no longer available.')
    act(() => wf.props!.onRowDoubleClick!(T0 - 600_000))
    await flush()
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith('Audio for this waterfall row is no longer available.', 'info')
  })

  it('what a review found follows the live lines under its own JTTY review heading, once a review, in each pane', async () => {
    const lines = [
      line({ id: 7, startUtcMs: T0 + 20_000, line: '1500 -10  CQ K1ABC FN42', rx: true }),
      line({ id: 3, startUtcMs: T0 + 2_000, line: '1500 -12  CQ W1AW FN31', rx: true, review: 1 }),
      line({ id: 4, startUtcMs: T0 + 15_000, line: '1500 -11  CQ K1ABC FN42', rx: true, review: 1 }),
      line({ id: 3, startUtcMs: T0 + 2_000, line: '1500 -12  CQ W1AW FN31', rx: true, review: 2 }),
    ]
    await mount(state({ lines }))
    const heading = t('jtty.review.heading')
    const rows = (id: 'all' | 'rx') => [...pane(id).children].map((el) => el.textContent)
    await waitFor(() =>
      expect(rows('all')).toEqual([
        '1500 -10  CQ K1ABC FN42',
        heading,
        '1500 -12  CQ W1AW FN31',
        '1500 -11  CQ K1ABC FN42',
        heading,
        '1500 -12  CQ W1AW FN31',
      ]),
    )
    expect(rows('rx').filter((r) => r === heading)).toHaveLength(2)
  })

  it('hands the waterfall’s visible range to the decoder, and again when it moves', async () => {
    await mount(state())
    await waitFor(() => expect(wf.props).not.toBeNull())
    act(() => wf.props!.onViewChange!(200, 3000))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(200, 3000)
    act(() => wf.props!.onViewChange!(1012.5, 1612.5))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(1012, 1613)
  })

  it('the whole transmitter is on screen, even for a state with no transmitter in it; the engine says why it refuses a send, and the text stays to send again', async () => {
    for (const s of [state(), state({ tx: txState({ refusal: 'TX is off — enable TX first' }) })]) {
      await mount(s)
      for (const key of ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8']) {
        expect(screen.getByRole('button', { name: new RegExp(`^${key}`) }), key).toBeTruthy()
      }
      expect(liveKey(), 'Live').toBeTruthy()
      expect(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), 'the Call box').toBeTruthy()
      expect(screen.getByRole('spinbutton', { name: t('jtty.serial.aria') }), 'the serial').toBeTruthy()
      expect(compose().disabled).toBe(false)
      expect(compose().placeholder).toBe(t('jtty.compose.placeholder'))
      cleanup()
    }
    await mount(state({ tx: txState({ refusal: 'TX is off — enable TX first' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('TX is off — enable TX first')
    m('jttySend').mockRejectedValue('TX is off — enable TX first')
    fireEvent.change(compose(), { target: { value: 'CQ TEST' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledWith('CQ TEST', '', null)
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith('TX is off — enable TX first', 'error')
    expect(compose().value, 'the refused text stays in the box').toBe('CQ TEST')
  })
})

/** A transmitter state, as the engine reports it. */
function txState(over: Partial<NonNullable<JttyState['tx']>> = {}): NonNullable<JttyState['tx']> {
  return { refusal: null, txHz: 1500, serial: 1, sending: false, live: false, left: 0, pending: '', notice: null, ...over }
}

const sent = (consumed: number, text: string, s: JttyState) => ({ consumed, sent: text, state: s })
/** The dock's Live key (a sender), by its name: `TX` and Live, or Sign off while a session runs. */
const liveKey = () =>
  screen.getByRole('button', { name: new RegExp(`^TX\\s*(${t('jtty.live.on.label')}|${t('jtty.live.off.label')})$`) }) as HTMLButtonElement
const compose = () => screen.getByRole('textbox', { name: t('jtty.compose.aria') }) as HTMLInputElement
const flush = () =>
  act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })

describe('the JTTY dock, wired to the engine (a state saying the transmitter is on)', () => {
  it('Send sends the typed text with the Call box, takes it out of the box and shows it sent', async () => {
    const on = state({ tx: txState() })
    await mount(on)
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'w1aw' } })
    m('jttySend').mockResolvedValue(sent(14, 'CQ CQ DE KD9TAW', state({ tx: txState({ left: 1, sending: true, pending: 'CQ CQ DE KD9TAW' }) })))
    fireEvent.change(compose(), { target: { value: 'cq cq de %M' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledWith('cq cq de %M', 'W1AW', null)
    expect(compose().value).toBe('')
    expect(document.querySelector('.jtty-committed')?.textContent).toBe('CQ CQ DE KD9TAW')
    expect(screen.getByRole('button', { name: t('jtty.compose.send.left', { n: 1 }) })).toBeTruthy()
  })

  it('sends go one after another in the order made: an F-key pressed while a send is in flight is neither lost nor first', async () => {
    await mount(state({ tx: txState() }))
    let release: (v: unknown) => void = () => {}
    m('jttySend').mockImplementation(() => new Promise((ok) => (release = ok)))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW 599 001', state({ tx: txState() })))
    fireEvent.change(compose(), { target: { value: 'CQ TEST' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey'), 'the F-key overtook the send in flight').not.toHaveBeenCalled()
    release(sent(7, 'CQ TEST', state({ tx: txState({ left: 1 }) })))
    await flush()
    expect(m('jttyFkey'), 'the F-key was lost').toHaveBeenCalledTimes(1)
    expect(m('jttySend').mock.invocationCallOrder[0]).toBeLessThan(m('jttyFkey').mock.invocationCallOrder[0])
  })

  it('F1–F8, clicked or pressed, send WSJT-X’s templates with the Call box; Alt/Ctrl/Cmd never', async () => {
    await mount(state({ tx: txState() }))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW 599 001', state({ tx: txState() })))
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', null)
    fireEvent.keyDown(window, { key: 'F6' })
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(6, 'W1AW', null)
    m('jttyFkey').mockClear()
    fireEvent.keyDown(window, { key: 'F1', altKey: true })
    fireEvent.keyDown(window, { key: 'F1', ctrlKey: true })
    fireEvent.keyDown(window, { key: 'F1', repeat: true })
    await flush()
    expect(m('jttyFkey')).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: /^F3/ }).textContent).toBe('F3%H TU CQ %M CQ')
  })

  it('while the keys are open for editing, F1–F8 send nothing (a refusal only); sending comes back when editing closes', async () => {
    await mount(state({ tx: txState() }))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW 599 001', state({ tx: txState() })))
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: t('jtty.fkeys.edit.label') }))
    // Pressed on the keyboard, in the page and in a key's own box being edited.
    fireEvent.keyDown(window, { key: 'F2' })
    fireEvent.keyDown(screen.getByRole('textbox', { name: t('jtty.fkeys.edit.aria', { key: 'F6' }) }), { key: 'F6' })
    await flush()
    expect(m('jttyFkey'), 'editing sends nothing').not.toHaveBeenCalled()
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.fkeys.editing.refused'), 'info')
    // …nor are there buttons to click while editing: the keys are their templates.
    expect(document.querySelectorAll('.jtty-macros button.cw-macro')).toHaveLength(0)
    fireEvent.click(screen.getByRole('button', { name: t('jtty.fkeys.done.label') }))
    fireEvent.keyDown(window, { key: 'F2' })
    await flush()
    expect(m('jttyFkey'), 'editing closed: F2 sends again').toHaveBeenLastCalledWith(2, 'W1AW', null)
    fireEvent.click(screen.getByRole('button', { name: /^F6/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(6, 'W1AW', null)
  })

  it('live entry: started with what is typed, typing commits as the engine decides, a pause flushes, Sign off ends it', async () => {
    vi.useFakeTimers()
    try {
      await mount(state({ tx: txState() }))
      const liveOn = state({ tx: txState({ live: true, sending: true }) })
      m('jttyLiveStart').mockResolvedValue(sent(6, 'HELLO ', liveOn))
      // The engine's poll says what it says from now on: a live session, on the air.
      m('getJttyState').mockResolvedValue(liveOn)
      fireEvent.change(compose(), { target: { value: 'HELLO ' } })
      fireEvent.click(liveKey())
      await flush()
      expect(m('jttyLiveStart')).toHaveBeenCalledWith('HELLO ', '', null)
      expect(compose().value).toBe('')
      m('jttyLiveCommit').mockResolvedValue(sent(0, '', liveOn))
      fireEvent.change(compose(), { target: { value: 'HOW ARE' } })
      await flush()
      expect(m('jttyLiveCommit')).toHaveBeenLastCalledWith('HOW ARE', '', false, null)
      m('jttyLiveCommit').mockResolvedValue(sent(7, 'HOW ARE', liveOn))
      await act(async () => {
        vi.advanceTimersByTime(1_500)
      })
      await flush()
      expect(m('jttyLiveCommit')).toHaveBeenLastCalledWith('HOW ARE', '', true, null)
      expect(compose().value).toBe('')
      expect(document.querySelector('.jtty-committed')?.textContent).toBe('HELLO HOW ARE')
      m('jttySignOff').mockResolvedValue(sent(0, 'DE KD9TAW K', state({ tx: txState({ left: 1, sending: true }) })))
      fireEvent.click(liveKey())
      await flush()
      expect(m('jttySignOff')).toHaveBeenCalledWith('', '', null)
    } finally {
      vi.useRealTimers()
    }
  })

  it('Esc/Stop is live exactly while JTTY is on the air, queued or live, and calls jtty_stop', async () => {
    const stopKey = () => screen.getByRole('button', { name: /^esc\s*stop$/i }) as HTMLButtonElement
    await mount(state({ tx: txState() }))
    expect(stopKey().disabled, 'nothing going').toBe(true)
    cleanup()
    for (const going of [{ sending: true }, { left: 2 }, { live: true }]) {
      await mount(state({ tx: txState(going) }))
      m('jttyStop').mockResolvedValue(state({ tx: txState() }))
      expect(stopKey().disabled, JSON.stringify(going)).toBe(false)
      fireEvent.click(stopKey())
      expect(m('jttyStop')).toHaveBeenCalledTimes(1)
      cleanup()
      vi.clearAllMocks()
    }
  })

  it('the serial is typed and sent on Enter; the notice or the refusal shows under the dock', async () => {
    await mount(state({ tx: txState({ serial: 7, refusal: 'TX is off — enable TX first' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('TX is off — enable TX first')
    const serial = screen.getByRole('spinbutton', { name: t('jtty.serial.aria') }) as HTMLInputElement
    expect(serial.value).toBe('7')
    fireEvent.focus(serial)
    fireEvent.change(serial, { target: { value: '12' } })
    fireEvent.keyDown(serial, { key: 'Enter' })
    expect(m('jttySetSerial')).toHaveBeenLastCalledWith(12)
    cleanup()
    await mount(state({ tx: txState({ notice: 'JTTY transmission complete.' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('JTTY transmission complete.')
  })

  it('%R: a call picked from a decode carries that decode’s SNR to every send, and typing in the Call box drops it (WSJT-X’s -10)', async () => {
    const lines = [
      line({ id: 3, snr: -15, text: 'CQ W1AW CQ', line: '1500 -15  CQ W1AW CQ', confirmedCalls: ['W1AW'] }),
      line({ id: -1, startUtcMs: T0 + 5_000, text: 'W1AW TU', line: '1500      W1AW TU', tx: true, snr: 0, confirmedCalls: ['W1AW'] }),
    ]
    await mount(state({ lines, tx: txState() }))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW KD9TAW -15', state({ lines, tx: txState() })))
    await waitFor(() => expect(within(pane('all')).getByRole('button', { name: 'W1AW' })).toBeTruthy())
    fireEvent.click(within(pane('all')).getByRole('button', { name: 'W1AW' }))
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', -15)
    m('jttySend').mockResolvedValue(sent(5, 'W1AW -15', state({ lines, tx: txState() })))
    fireEvent.change(compose(), { target: { value: '%H %R' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenLastCalledWith('%H %R', 'W1AW', -15)
    // Picked from our own line: no decode, no SNR.
    const own = [...pane('rx').querySelectorAll('.jtty-line')].find((el) => el.textContent?.includes('TU'))!
    fireEvent.click(within(own as HTMLElement).getByRole('button', { name: 'W1AW' }))
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', null)
    fireEvent.click(within(pane('all')).getByRole('button', { name: 'W1AW' }))
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'K1ABC' } })
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'K1ABC', null)
  })

  it('F1–F8 show the set the engine sends; an empty key is shown empty and sends nothing', async () => {
    const fkeys = ['%H %M %G', '%H %M %R', '%H %M R%R', '%H %M RRR', '%H %M 73', 'CQ %M %G', '', '']
    await mount(state({ tx: txState({ style: 'ft8', fkeys }) }))
    expect(screen.getByRole('button', { name: /^F2/ }).textContent).toBe('F2%H %M %R')
    const f7 = screen.getByRole('button', { name: /^F7/ }) as HTMLButtonElement
    expect(f7.textContent).toBe(`F7${t('jtty.fkey.empty.label')}`)
    expect(f7.disabled).toBe(true)
    expect((screen.getByRole('combobox', { name: t('jtty.style.aria') }) as HTMLSelectElement).value).toBe('ft8')
  })

  it('the message style and the keys’ messages are Settings, written through the patch seam over the live settings; editing never sends', async () => {
    await mount(state({ tx: txState() }))
    const live = { mycall: 'KD9TAW', jttyFkeys: ['CQ %M CQ', 'MINE %H', '%H TU CQ %M CQ', '%M', '%H', 'TU NOW %Q %E', '%H AGN?', '%E'] }
    m('getSettings').mockResolvedValue(live)
    m('setSettings').mockResolvedValue(snap)
    fireEvent.change(screen.getByRole('combobox', { name: t('jtty.style.aria') }), { target: { value: 'ft8' } })
    await flush()
    expect(m('setSettings')).toHaveBeenLastCalledWith({ ...live, jttyMessageStyle: 'ft8' })
    fireEvent.click(screen.getByRole('button', { name: t('jtty.fkeys.edit.label') }))
    const f1 = screen.getByRole('textbox', { name: t('jtty.fkeys.edit.aria', { key: 'F1' }) }) as HTMLInputElement
    expect(f1.value).toBe('CQ %M CQ')
    fireEvent.change(f1, { target: { value: 'CQ TEST %M' } })
    fireEvent.keyDown(f1, { key: 'Enter' })
    await flush()
    // F2 keeps what the live settings hold, not what this screen showed when it opened.
    expect(m('setSettings')).toHaveBeenLastCalledWith({
      ...live,
      jttyFkeys: ['CQ TEST %M', 'MINE %H', '%H TU CQ %M CQ', '%M', '%H', 'TU NOW %Q %E', '%H AGN?', '%E'],
    })
    fireEvent.keyDown(window, { key: 'F9' })
    expect(m('jttyFkey'), 'editing sends nothing').not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: t('jtty.fkeys.reset.label') }))
    await flush()
    expect(m('setSettings')).toHaveBeenLastCalledWith({
      ...live,
      jttyFkeys: ['CQ %M CQ', '%H %E', '%H TU CQ %M CQ', '%M', '%H', 'TU NOW %Q %E', '%H AGN?', '%E'],
    })
  })

  it('a TU send asks to log the thanked station and changes nothing until pressed: Log QSO hands the strip a request for that call, the engine takes the answer', async () => {
    const prompt = (id: number, call = 'W1AW') => state({ tx: txState({ logPrompt: { id, call, serial: 7, atUtcMs: T0 } }) })
    m('jttyLogPromptAnswer').mockResolvedValue('taken')
    await mount(prompt(1))
    const ask = () => document.querySelector('.jtty-log-prompt') as HTMLElement | null
    const press = () => fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.log') }))
    await waitFor(() => expect(ask()?.textContent).toContain(t('jtty.logPrompt.text', { call: 'W1AW' })))
    expect(screen.getByTestId('log-stub').querySelector('.stub-call')?.textContent, 'the strip is not touched').toBe('')
    expect((screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }) as HTMLInputElement).value, 'nor the Call box').toBe('')
    expect(strip.last?.logRequest ?? null, 'asking requests nothing').toBeNull()
    press()
    const req = strip.last?.logRequest
    expect(req?.call).toBe('W1AW')
    expect(m('jttyLogPromptAnswer'), 'nothing answered before the strip asks').not.toHaveBeenCalled()
    await expect(req!.claim()).resolves.toBe(true)
    expect(m('jttyLogPromptAnswer')).toHaveBeenCalledWith(1)
    // The strip's outcomes: said, and the question stays, until it is logging. Each is the end of
    // that request: the cockpit lets go of it, and the next press is a request of its own.
    act(() => strip.last!.onLogRequest!(req!.id, { kind: 'not-ready' }))
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.notReady'), 'info')
    expect(strip.last?.logRequest ?? null, 'handled: let go').toBeNull()
    press()
    const req2 = strip.last!.logRequest!
    expect(req2.id).not.toBe(req!.id)
    act(() => strip.last!.onLogRequest!(req2.id, { kind: 'other-call', held: 'K1ABC' }))
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.otherCall', { call: 'W1AW', held: 'K1ABC' }), 'info')
    expect(strip.last?.logRequest ?? null).toBeNull()
    expect(ask()).not.toBeNull()
    press()
    const req3 = strip.last!.logRequest!
    act(() => strip.last!.onLogRequest!(req3.id, { kind: 'logging' }))
    expect(ask(), 'answered').toBeNull()
    expect(strip.last?.logRequest ?? null).toBeNull()
    // An outcome for a request already let go changes nothing.
    act(() => strip.last!.onLogRequest!(req!.id, { kind: 'not-ready' }))
    expect(vi.mocked(pushToast)).not.toHaveBeenLastCalledWith(t('jtty.logPrompt.notReady'), 'info')
    // The engine still reports the same prompt on its next poll: it is not asked again.
    m('getJttyState').mockResolvedValue(prompt(1))
    await flush()
    expect(ask()).toBeNull()
    cleanup()
    // Not now: the engine is told, for every screen, and nothing is requested of the strip.
    await mount(prompt(2, 'K1ABC'))
    await waitFor(() => expect(ask()).not.toBeNull())
    fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.dismiss') }))
    expect(ask()).toBeNull()
    expect(m('jttyLogPromptAnswer')).toHaveBeenLastCalledWith(2)
    expect(strip.last?.logRequest ?? null).toBeNull()
  })

  it('an answer that took nothing says why: answered elsewhere, already logged, replaced, or too old', async () => {
    const prompt = (id: number) => state({ tx: txState({ logPrompt: { id, call: 'W1AW', serial: 7, atUtcMs: T0 } }) })
    const ask = () => document.querySelector('.jtty-log-prompt') as HTMLElement | null
    const cases = [
      ['answered', 'jtty.logPrompt.taken'],
      ['logged', 'jtty.logPrompt.logged'],
      ['replaced', 'jtty.logPrompt.replaced'],
      ['expired', 'jtty.logPrompt.expired'],
    ] as const
    for (const [i, [answer, key]] of cases.entries()) {
      m('jttyLogPromptAnswer').mockResolvedValue(answer)
      await mount(prompt(i + 1))
      await waitFor(() => expect(ask()).not.toBeNull())
      fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.log') }))
      const req = strip.last!.logRequest!
      await expect(req.claim(), answer).resolves.toBe(false)
      act(() => strip.last!.onLogRequest!(req.id, { kind: 'taken' }))
      expect(vi.mocked(pushToast), answer).toHaveBeenLastCalledWith(t(key), 'info')
      expect(ask(), answer).toBeNull()
      cleanup()
    }
  })

  it('a Log QSO the strip then refused gives the prompt back, here and on every screen', async () => {
    const prompt = state({ tx: txState({ logPrompt: { id: 3, call: 'W1AW', serial: 7, atUtcMs: T0 } }) })
    m('jttyLogPromptAnswer').mockResolvedValue('taken')
    await mount(prompt)
    const ask = () => document.querySelector('.jtty-log-prompt') as HTMLElement | null
    await waitFor(() => expect(ask()).not.toBeNull())
    fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.log') }))
    const req = strip.last!.logRequest!
    await req.claim()
    act(() => strip.last!.onLogRequest!(req.id, { kind: 'logging' }))
    expect(ask()).toBeNull()
    expect(m('jttyLogPromptRelease'), 'logging: nothing given back').not.toHaveBeenCalled()
    act(() => strip.last!.onLogRequest!(req.id, { kind: 'refused' }))
    expect(m('jttyLogPromptRelease')).toHaveBeenCalledWith(3)
    expect(ask()?.textContent, 'asked again').toContain(t('jtty.logPrompt.text', { call: 'W1AW' }))
  })

  it('a TU that replaced an unanswered prompt says whose it was', async () => {
    await mount(state({ tx: txState({ logPrompt: { id: 2, call: 'K1ABC', serial: 8, atUtcMs: T0, replaced: 'W1AW' } }) }))
    const ask = () => document.querySelector('.jtty-log-prompt') as HTMLElement | null
    await waitFor(() => expect(ask()).not.toBeNull())
    expect(ask()!.textContent).toContain(t('jtty.logPrompt.text', { call: 'K1ABC' }))
    expect(ask()!.textContent).toContain(t('jtty.logPrompt.replacedNote', { call: 'W1AW' }))
    cleanup()
    await mount(state({ tx: txState({ logPrompt: { id: 3, call: 'K1ABC', serial: 8, atUtcMs: T0, replaced: null } }) }))
    await waitFor(() => expect(ask()).not.toBeNull())
    expect(ask()!.textContent, 'control: none replaced').not.toContain(t('jtty.logPrompt.replacedNote', { call: 'W1AW' }))
  })

  it('M1: %R goes only with the call it was taken for, however the Call box changed; the newest decode naming the call gives it (WSJT-X’s snrForSelectedWord)', async () => {
    const lines = [
      line({ id: 3, snr: -15, text: 'CQ W1AW CQ', line: '1500 -15  CQ W1AW CQ', confirmedCalls: ['W1AW'] }),
      line({ id: 4, startUtcMs: T0 + 9_000, snr: -7, text: 'KD9TAW W1AW 599', line: '1500 -7  KD9TAW W1AW 599', confirmedCalls: [] }),
      line({ id: 5, startUtcMs: T0 + 12_000, snr: -20, text: 'CQ K1ABC CQ', line: '1500 -20  CQ K1ABC CQ', confirmedCalls: [] }),
    ]
    await mount(state({ lines, tx: txState() }))
    m('jttyFkey').mockResolvedValue(sent(0, 'x', state({ lines, tx: txState() })))
    const box = () => screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }) as HTMLInputElement
    await waitFor(() => expect(within(pane('all')).getByRole('button', { name: 'W1AW' })).toBeTruthy())
    // Clicked in the older line: the newest decode naming W1AW gives the report.
    fireEvent.click(within(pane('all')).getByRole('button', { name: 'W1AW' }))
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', -7)
    // Typed over, then W1AW clicked again: the box is W1AW again, with W1AW's report.
    fireEvent.change(box(), { target: { value: 'K1ABC' } })
    fireEvent.click(within(pane('all')).getByRole('button', { name: 'W1AW' }))
    expect(box().value).toBe('W1AW')
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', -7)
    // Typed to another station: no report goes with it, and typing W1AW back does not bring one.
    fireEvent.change(box(), { target: { value: 'K1ABC' } })
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'K1ABC', null)
    fireEvent.change(box(), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW', null)
  })

  it('no prompt, no question: a state without one shows nothing', async () => {
    await mount(state({ tx: txState() }))
    expect(document.querySelector('.jtty-log-prompt')).toBeNull()
  })

  it('Ctrl+H types WSJT-X’s chat opener where the caret is; Ctrl+Shift+K clears the box and stops what is going', async () => {
    await mount(state({ tx: txState({ sending: true, left: 1 }) }))
    m('jttyStop').mockResolvedValue(state({ tx: txState() }))
    fireEvent.change(compose(), { target: { value: 'HELLO' } })
    compose().setSelectionRange(0, 0)
    fireEvent.keyDown(compose(), { key: 'h', ctrlKey: true })
    expect(compose().value).toBe('%H %M HELLO')
    expect(m('jttySend'), 'Ctrl+H sends nothing').not.toHaveBeenCalled()
    fireEvent.keyDown(compose(), { key: 'K', ctrlKey: true, shiftKey: true })
    expect(compose().value).toBe('')
    expect(m('jttyStop')).toHaveBeenCalledTimes(1)
    cleanup()
    // Nothing going: Ctrl+Shift+K only clears.
    m('jttyStop').mockClear()
    await mount(state({ tx: txState() }))
    fireEvent.change(compose(), { target: { value: 'HELLO' } })
    fireEvent.keyDown(compose(), { key: 'K', ctrlKey: true, shiftKey: true })
    expect(compose().value).toBe('')
    expect(m('jttyStop')).not.toHaveBeenCalled()
  })

  it('Rx→Tx and Tx→Rx copy one into the other, as WSJT-X’s buttons do; the frame count shows sent/total', async () => {
    await mount(state({ rxHz: 1320, tx: txState({ txHz: 1700, framesSent: 2, framesTotal: 5 }) }))
    m('jttySetTx').mockResolvedValue(state())
    fireEvent.click(screen.getByRole('button', { name: 'Rx→Tx' }))
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1320)
    fireEvent.click(screen.getByRole('button', { name: 'Tx→Rx' }))
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1700, 20)
    expect(document.querySelector('.jtty-frames')?.textContent).toBe(t('jtty.frames', { sent: 2, total: 5 }))
    cleanup()
    await mount(state({ tx: txState() }))
    expect(document.querySelector('.jtty-frames'), 'nothing to count, nothing shown').toBeNull()
  })

  it('what this station sent shows in the QSO frequency pane only', async () => {
    const lines = [
      line({ id: 3, line: '1500 -12  CQ K1ABC CQ', rx: true }),
      line({ id: -1, startUtcMs: T0 + 5_000, line: '1500      W1AW TU', rx: true, tx: true }),
    ]
    await mount(state({ lines, tx: txState() }))
    await waitFor(() => expect(paneLines('rx')).toEqual(['1500 -12  CQ K1ABC CQ', '1500      W1AW TU']))
    expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ'])
  })
})

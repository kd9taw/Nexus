// @vitest-environment jsdom
//
// ⭐ A JTTY CALL IS LOOKED UP ONLY ONCE THE TWO-DECODE RULE HAS CONFIRMED IT (operator ruling,
// 2026-10-12: lookups, spots and Needed only from calls decoded in two separate messages). JTTY's
// CRC-12 lets the odd false decode through, some of them well-formed calls; a callbook lookup of
// one would spend the operator's quota on a station that does not exist and show its name as if
// it had been heard. The engine lists the confirmed calls on each line (`confirmedCalls`), and
// that list is the only thing in the cockpit that can reach a lookup: a confirmed call is a
// button that fills the log strip's Call as a SURE fill, which is what the strip looks up.
//
// The log strip here is the REAL one, so the lookup is the real `qrzLookup` call the strip makes.
// The positive control is the confirmed call on the same screen, under the same wait.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, within, screen } from '@testing-library/react'
import { JttyCockpit } from './JttyCockpit'
import type { AppSnapshot, JttyLine, JttyState } from '../types'
import * as api from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  // The log's questions go unanswered: nothing here needs the log, and null is no answer.
  mod.askLog = vi.fn(async () => {
    throw new Error('no log in this test')
  })
  mod.searchParks = vi.fn(async () => [])
  mod.getLicensedBandPlan = vi.fn(async () => [])
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./RfScopePane', () => ({ RfScopePane: () => null }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  hunt: null,
  b4MatchMode: false,
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const T0 = Date.UTC(2026, 9, 13, 22, 13, 20)
const line = (over: Partial<JttyLine>): JttyLine => ({
  id: 1,
  startUtcMs: T0,
  freqHz: 1500,
  dialHz: 14_090_000,
  snr: -12,
  text: '',
  line: '',
  state: 'complete',
  rx: true,
  confirmedCalls: [],
  ...over,
})

const STATE: JttyState = {
  armed: true,
  rxHz: 1500,
  ftolHz: 20,
  lowHz: 200,
  highHz: 3000,
  heard: [],
  notice: null,
  lines: [
    // Heard twice: confirmed.
    line({ id: 1, text: 'CQ W1AW FN31', line: '1500 -12  CQ W1AW FN31', confirmedCalls: ['W1AW'] }),
    // Heard once: may be a false decode (a fabricated but well-formed call).
    line({ id: 2, startUtcMs: T0 + 3_000, text: 'TU DA2PKN CQ', line: '1520 -19  TU DA2PKN CQ', confirmedCalls: [] }),
  ],
}

const qrz = () => vi.mocked(api.qrzLookup)
const logCall = () => document.querySelector('.le-call') as HTMLInputElement
const allPane = () => document.querySelector('[data-pane="all"] .jtty-lines') as HTMLElement

async function mount() {
  vi.mocked(api.getJttyState).mockResolvedValue(STATE)
  vi.mocked(api.jttyAutoArm).mockResolvedValue(STATE)
  render(
    <JttyCockpit snap={snap} active onSetTxEnabled={() => {}} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  await waitFor(() => expect(allPane().querySelectorAll('.jtty-line')).toHaveLength(2))
  expect(logCall(), 'control: the real log strip is on screen').not.toBeNull()
}

/** Longer than the strip's own settle-then-look-up wait (700 ms), with room to spare. */
const pastTheLookupWait = () => new Promise((r) => setTimeout(r, 1_500))

beforeEach(() => {
  localStorage.clear()
  qrz().mockResolvedValue({ call: 'W1AW', name: 'ARRL HQ', grid: 'FN31', country: 'United States' } as never)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('a JTTY call is looked up only once two separate messages confirmed it', () => {
  it('a call heard once offers nothing to click, and pressing its text looks nothing up', async () => {
    await mount()
    const pane = within(allPane())
    expect(pane.queryByRole('button', { name: 'DA2PKN' }), 'not a button').toBeNull()
    fireEvent.click(pane.getByText('DA2PKN', { exact: false }))
    fireEvent.doubleClick(pane.getByText('DA2PKN', { exact: false }))
    await act(pastTheLookupWait)
    expect(qrz()).not.toHaveBeenCalled()
    expect(logCall().value, 'the log line was not filled').toBe('')
  })

  // THE LOG QSO PROMPT AGAINST THE REAL STRIP. It names the station the TU thanked; asking changes
  // nothing on screen; Log QSO logs through the strip's own Log only the station it names, and
  // only once the engine has handed it this screen (the first answer, for every screen).
  const prompted = (id = 1, call = 'W1AW', lines = STATE.lines): JttyState => ({
    ...STATE,
    lines,
    tx: { refusal: null, txHz: 1500, serial: 8, sending: false, live: false, left: 0, pending: '', notice: null, logPrompt: { id, call, serial: 7, atUtcMs: T0 } },
  })
  const ask = () => document.querySelector('.jtty-log-prompt') as HTMLElement | null
  const pressLogQso = () => fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.log') }))
  const callBox = () => screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }) as HTMLInputElement

  it('Log QSO logs the station the prompt names, through the strip’s own Log, once the engine hands it this screen', async () => {
    vi.mocked(api.logQso).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('taken')
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    vi.mocked(api.getJttyState).mockResolvedValue(prompted())
    await waitFor(() => expect(ask()?.textContent).toContain(t('jtty.logPrompt.text', { call: 'W1AW' })), { timeout: 3_000 })
    await act(pastTheLookupWait)
    expect(api.logQso, 'the question alone logs nothing').not.toHaveBeenCalled()
    pressLogQso()
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    expect(api.jttyLogPromptAnswer).toHaveBeenCalledWith(1)
    expect(vi.mocked(api.logQso).mock.calls[0][0]).toMatchObject({ call: 'W1AW', mode: 'JTTY' })
    await waitFor(() => expect(ask(), 'answered').toBeNull())
  })

  it('Log QSO logs the contact at the dial plus JTTY’s Tx frequency, as the strip’s own Log does', async () => {
    vi.mocked(api.logQso).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('taken')
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    // Tx 1234 Hz, apart from the Rx frequency (1500 Hz): the contact is where the TU was sent.
    const base = prompted()
    vi.mocked(api.getJttyState).mockResolvedValue({ ...base, tx: { ...base.tx!, txHz: 1234 } })
    await waitFor(() => expect(ask()?.textContent).toContain(t('jtty.logPrompt.text', { call: 'W1AW' })), { timeout: 3_000 })
    pressLogQso()
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    const logged = vi.mocked(api.logQso).mock.calls[0][0] as { call: string; mode: string; freqMhz: number }
    expect(logged).toMatchObject({ call: 'W1AW', mode: 'JTTY' })
    expect(logged.freqMhz, '14.090 MHz dial + 1234 Hz Tx').toBe(14.091234)
  })

  it('H1:a strip holding ANOTHER call logs nothing and says so; the strip is left as the operator has it', async () => {
    vi.mocked(api.logQso).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('taken')
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    fireEvent.change(logCall(), { target: { value: 'K1ABC' } })
    vi.mocked(api.getJttyState).mockResolvedValue(prompted())
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    pressLogQso()
    await act(pastTheLookupWait)
    expect(api.logQso, 'K1ABC is not the station the prompt names').not.toHaveBeenCalled()
    expect(api.jttyLogPromptAnswer, 'and the question is still open').not.toHaveBeenCalled()
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.otherCall', { call: 'W1AW', held: 'K1ABC' }), 'info')
    expect(logCall().value).toBe('K1ABC')
    expect(ask()).not.toBeNull()
  })

  it('M2: the prompt moves neither the Call box nor the strip; an empty strip is filled only by the press, for a second press to log', async () => {
    vi.mocked(api.logQso).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('taken')
    vi.mocked(api.jttyFkey).mockResolvedValue(null as never)
    const next = [...STATE.lines, line({ id: 3, startUtcMs: T0 + 6_000, snr: -3, text: 'K1ABC W1AW EN52', line: '1500 -3  K1ABC W1AW EN52', confirmedCalls: ['K1ABC'] })]
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    vi.mocked(api.getJttyState).mockResolvedValue({ ...STATE, lines: next })
    await waitFor(() => expect(within(allPane()).queryByRole('button', { name: 'K1ABC' })).not.toBeNull(), { timeout: 3_000 })
    fireEvent.click(within(allPane()).getByRole('button', { name: 'K1ABC' }))
    await waitFor(() => expect(logCall().value).toBe('K1ABC'))
    // The TU to W1AW starts on the air now, after the operator moved on to K1ABC.
    vi.mocked(api.getJttyState).mockResolvedValue(prompted(1, 'W1AW', next))
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    expect([callBox().value, logCall().value], 'nothing moved').toEqual(['K1ABC', 'K1ABC'])
    fireEvent.click(document.querySelectorAll('.jtty-macros .cw-macro')[1] as HTMLElement)
    await waitFor(() => expect(api.jttyFkey).toHaveBeenCalled())
    expect(vi.mocked(api.jttyFkey).mock.calls[0]).toEqual([2, 'K1ABC', -3])
    cleanup()
    // An empty strip: the prompt leaves it empty; the press fills it and logs nothing; the next logs.
    await mount()
    vi.mocked(api.getJttyState).mockResolvedValue(prompted(2))
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    expect(logCall().value, 'asking fills nothing').toBe('')
    pressLogQso()
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.filled', { call: 'W1AW' }), 'info')
    await act(pastTheLookupWait)
    expect(api.logQso, 'filled, not logged').not.toHaveBeenCalled()
    pressLogQso()
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.logQso).mock.calls[0][0]).toMatchObject({ call: 'W1AW' })
  })

  it('M3: answered on another screen first: nothing is logged here and the question goes; Not now answers it for every screen', async () => {
    vi.mocked(api.logQso).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('answered')
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    vi.mocked(api.getJttyState).mockResolvedValue(prompted(4))
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    pressLogQso()
    await waitFor(() => expect(api.jttyLogPromptAnswer).toHaveBeenCalledWith(4))
    await waitFor(() => expect(ask()).toBeNull())
    expect(api.logQso, 'the other screen has it').not.toHaveBeenCalled()
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.taken'), 'info')
    cleanup()
    vi.mocked(api.jttyLogPromptAnswer).mockClear()
    await mount()
    vi.mocked(api.getJttyState).mockResolvedValue(prompted(5))
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    fireEvent.click(within(ask()!).getByRole('button', { name: t('jtty.logPrompt.dismiss') }))
    expect(ask()).toBeNull()
    expect(api.jttyLogPromptAnswer).toHaveBeenCalledWith(5)
    expect(api.logQso).not.toHaveBeenCalled()
  })

  it('N2: a contact logged with the strip’s own Log closes the prompt, and Log QSO cannot log it again', async () => {
    // The engine as it answers: once W1AW is in the log, its prompt is no longer asked, and an
    // answer of it says it is logged (`a_contact_logged_by_any_route_closes_its_prompt`).
    let inLog = false
    vi.mocked(api.logQso).mockImplementation(async () => {
      inLog = true
      return snap
    })
    vi.mocked(api.jttyLogPromptAnswer).mockImplementation(async () => (inLog ? 'logged' : 'taken'))
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    // The poll answers before the strip's Log here: the prompt is still shown after the log.
    vi.mocked(api.getJttyState).mockResolvedValue(prompted())
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    fireEvent.click(document.querySelector('.le-actions > .le-log-btn') as HTMLElement)
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(logCall().value).toBe(''))
    // Log QSO pressed before the next poll: the first press fills the emptied strip, the second
    // asks the engine, which says the contact is logged. Nothing more is logged.
    pressLogQso()
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    await act(pastTheLookupWait)
    pressLogQso()
    await waitFor(() => expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith(t('jtty.logPrompt.logged'), 'info'))
    await act(pastTheLookupWait)
    expect(vi.mocked(api.logQso).mock.calls.map((c) => (c[0] as { call: string }).call), 'one contact, logged once').toEqual(['W1AW'])
    expect(ask()).toBeNull()
    // The engine's next poll has no prompt: none is shown.
    vi.mocked(api.getJttyState).mockResolvedValue({ ...STATE, tx: { ...prompted().tx!, logPrompt: null } })
    await act(pastTheLookupWait)
    expect(ask()).toBeNull()
  })

  it('a Log QSO the strip could not log gives the prompt back: it can still be answered', async () => {
    // The log refuses (the strip's error toast says why): nothing landed.
    vi.mocked(api.logQso).mockResolvedValueOnce(null as never).mockResolvedValue(snap)
    vi.mocked(api.jttyLogPromptAnswer).mockResolvedValue('taken')
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    vi.mocked(api.getJttyState).mockResolvedValue(prompted(6))
    await waitFor(() => expect(ask()).not.toBeNull(), { timeout: 3_000 })
    await act(pastTheLookupWait)
    pressLogQso()
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(api.jttyLogPromptRelease).toHaveBeenCalledWith(6))
    await waitFor(() => expect(ask(), 'asked again').not.toBeNull())
    expect(logCall().value, 'the contact is still on the strip').toBe('W1AW')
    // Answered again, and this time logged.
    pressLogQso()
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(2))
    expect(api.jttyLogPromptRelease).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(ask()).toBeNull())
  })

  it('POSITIVE CONTROL: the confirmed call, clicked, fills the log line and is looked up once', async () => {
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    await act(pastTheLookupWait)
    expect(qrz()).toHaveBeenCalledTimes(1)
    expect(qrz()).toHaveBeenCalledWith('W1AW')
  })
})

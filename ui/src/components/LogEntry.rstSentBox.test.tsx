// @vitest-environment jsdom
//
// THE OPTIONAL "RST SENT" BOX in a cockpit's contest strip (the logger window's own suite is
// ContestLogger.rstSentBox.test.tsx). Drawn only while the engine sends `rstSentBox`; a typed report
// rides every command the strip logs through: a county line's rows and the satellite strip's.
// Rendered with the props `PhoneCockpit` passes and the ILQP `fieldDay` of LogEntry.countyLine.test.tsx.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { t } from '../i18n'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  // Every row entered the log, unless a test says otherwise.
  contestLogManualRows: vi.fn((_call: string, rows: unknown[]) => Promise.resolve(rows.map(() => true))),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestPlace: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** The Illinois QSO Party as the engine serialises it: RST plus ONE QTH box drawing on the
 *  county list and the state/province list (a `oneOf`, whose free-text arm is how a DX
 *  station's country gets in). */
const ilqp = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

function renderStrip(exchange: 'terrestrial' | 'satellite' = 'terrestrial', fd: FieldDayStatus = ilqp()) {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange={exchange}
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fd}
      fdMode="CW"
    />,
  )
}

const on = (): FieldDayStatus => ({ ...ilqp(), rstSentBox: '599' }) as FieldDayStatus
const sentBox = () => screen.queryByLabelText(t('logEntry.fd.rstSent.label')) as HTMLInputElement | null
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const qthBox = () => screen.getByLabelText('County / State') as HTMLInputElement

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the cockpit contest strip: the optional RST sent box', () => {
  it('is not drawn while the engine sends no rstSentBox, and is drawn before the received report when it does', () => {
    renderStrip()
    expect(sentBox()).toBeNull()
    cleanup()
    renderStrip('terrestrial', on())
    expect(sentBox()!.value).toBe('599')
    const inputs = [...document.querySelectorAll('.le-fd-big input')]
    // Call, then Snt, then Rcv — N1MM's order.
    expect(inputs.indexOf(sentBox()!)).toBe(1)
    expect(inputs[2]).toBe(screen.getByLabelText('RST'))
  })

  it('is never drawn on the Remote, which logs no contest from afar', () => {
    render(
      <LogEntry
        onOpenLogbook={() => {}}
        snap={snap}
        mode="CW"
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        onSpot={() => {}}
        pendingWork={null}
        onConsumeWork={() => {}}
        fieldDay={on()}
        fdMode="CW"
        remote={{ submit: vi.fn(), canSubmit: true, busy: false, pending: false, resetKey: 0, recall: () => null }}
      />,
    )
    // CONTROL: the contest strip itself is drawn there.
    expect(screen.getByLabelText('RST')).toBeTruthy()
    expect(sentBox()).toBeNull()
  })

  it('a county line logs every county with the typed report', async () => {
    renderStrip('terrestrial', on())
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    fireEvent.change(sentBox()!, { target: { value: '579' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK/DUPG' } })
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogManualRows).toHaveBeenCalledTimes(1))
    const args = api.contestLogManualRows.mock.calls[0]
    expect(args[0]).toBe('K9NR')
    expect(args[5]).toBe('579')
  })

  it('the satellite strip logs the typed report through its own command', async () => {
    renderStrip('satellite', on())
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    fireEvent.change(sentBox()!, { target: { value: '449' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogSatellite).toHaveBeenCalledTimes(1))
    expect(api.contestLogSatellite.mock.calls[0][5]).toBe('449')
    // CONTROL: the terrestrial command was not the one.
    expect(api.contestLogManual).not.toHaveBeenCalled()
  })
})

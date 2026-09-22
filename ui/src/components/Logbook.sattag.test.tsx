// @vitest-environment jsdom
//
// EDITING A SATELLITE TAG FROM INSIDE NEXUS.
//
// `docs/guide/satellites.md` used to send an operator who mis-tagged a contact out of the
// application entirely: "Fix them there, with Nexus closed — correct the name, or delete both
// fields from the record. There is no way to do it from inside Nexus." For a thousand
// operators that is a defect, not a limitation.
//
// ⭐ BUT THE OBVIOUS FIX IS THE WRONG ONE. The same passage says why the edit form carries
// neither field: "an edit that leaves them blank deliberately *preserves* what is stored, so
// that an ordinary busted-call fix cannot silently strip a satellite tag off a record that
// earned it." Put the two fields on that form and a blank box means "leave it alone" and
// "clear it" at once — and the form submits blanks for every field the operator did not
// touch. The tag would come off contacts nobody was editing the tag of.
//
// So the repair is an act the operator CHOOSES: a row menu whose "Not via satellite" entry is
// unmistakably a decision. The last test here is the other half of the bargain — the edit form
// still carries neither field, so the preserve rule is untouched.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../api', () => {
  const noop = () => vi.fn()
  const getLog = vi.fn()
  return {
    getLog,
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
    deleteQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQso: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    // The real table LoTW accepts is the backend's (Engine::LOTW_SAT_NAMES); three of its
    // names are enough to prove the picker offers what the backend hands it.
    lotwSatNames: vi.fn(async () => ['AO-91', 'ARISS', 'SO-50']),
    setSatTag: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: noop(), markQslCard: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

/** One contact. `tagged` gives it the PROP_MODE/SAT_NAME pair a mis-tag leaves behind. */
function logRow(tagged: boolean) {
  return {
    call: 'K0ABC', grid: 'EN37', band: '70cm', freqMhz: 435.5, mode: 'SSB',
    rstSent: '59', rstRcvd: '57', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000,
    confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    ...(tagged ? { propMode: 'SAT', satName: 'RS-44' } : {}),
  }
}

async function renderLog(tagged: boolean) {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([logRow(tagged)])
  const { container } = render(
    <Logbook defaultBand="70cm" defaultFreqMhz={435.5} defaultMode="SSB" />,
  )
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  // By its accessible name, the way an operator finds it — the row carries a QSL select too,
  // so a bare `querySelector('select')` would answer with the wrong control.
  const sat = await waitFor(() => {
    const el = container.querySelector(
      'select[aria-label="Set satellite for K0ABC"]',
    ) as HTMLSelectElement
    expect(el).not.toBeNull()
    return el
  })
  return { container, sat }
}

const values = (s: HTMLSelectElement) => [...s.options].map((o) => o.value)
const setSatTag = () => api.setSatTag as ReturnType<typeof vi.fn>

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

describe('correcting a satellite tag from inside Nexus', () => {
  it('offers the satellites the backend says LoTW accepts, and nothing typed', async () => {
    const { sat } = await renderLog(true)
    // A PICKER, not a text box: TQSL matches SAT_NAME against its own list and rejects
    // anything else ("AO7" for "AO-7"), and one rejected record takes its whole signed batch
    // with it through the compliant upload funnel. A typed name is a permanent guess.
    expect(values(sat)).toEqual(expect.arrayContaining(['AO-91', 'ARISS', 'SO-50']))
    expect(sat.querySelector('input'), 'a free-text box would let a typo reach the record')
      .toBeNull()
  })

  it('corrects a wrong designator to the one the operator picked', async () => {
    const { sat } = await renderLog(true)
    fireEvent.change(sat, { target: { value: 'AO-91' } })
    // The row on screen, never its position (a Remote delete shifts positions).
    await waitFor(() => expect(setSatTag()).toHaveBeenCalledWith(logRow(true), 'AO-91'))
  })

  it('REMOVES the tag when the operator says the contact was not via satellite', async () => {
    const { sat } = await renderLog(true)
    expect(values(sat), 'the way out has to be reachable').toContain('clear')
    fireEvent.change(sat, { target: { value: 'clear' } })
    // `null`, and only `null`, is the removal — the same shape as markQslSent's withdrawal.
    await waitFor(() => expect(setSatTag()).toHaveBeenCalledWith(logRow(true), null))
  })

  it('does not offer the removal on a contact that carries no tag', async () => {
    const { sat } = await renderLog(false)
    // Positive control: the picker itself rendered, so the missing entry below is a real
    // absence and not a dead query against a control that was never there.
    expect(values(sat)).toContain('SO-50')
    expect(values(sat)).not.toContain('clear')
  })

  it('treats the placeholder as a non-choice and writes nothing', async () => {
    const { sat } = await renderLog(true)
    // '' is where the select sits when the operator has chosen nothing. Honouring it as a
    // removal would erase a tag nobody asked to erase — the same trap an empty QSL-sent code
    // is refused for.
    fireEvent.change(sat, { target: { value: '' } })
    expect(setSatTag()).not.toHaveBeenCalled()
  })
})

// ⭐ THE INVARIANT THE MENU ABOVE EXISTS TO PROTECT. If a later change "helpfully" adds the
// two fields to the edit form, this reddens — which is the point. A blank box there would be
// ambiguous between leave-alone and clear, and the backend's preserve rule
// (`Logbook::update_record`) reads a missing field as leave-alone, so the ambiguity would
// resolve the wrong way for every operator who fixed a busted call on a satellite contact.
describe('the edit form still carries neither satellite field', () => {
  it('sends no propMode or satName when a busted call is corrected', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([logRow(true)])
    const { container } = render(
      <Logbook defaultBand="70cm" defaultFreqMhz={435.5} defaultMode="SSB" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    fireEvent.click(container.querySelector('button[aria-label="Edit K0ABC"]') as HTMLButtonElement)
    await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())

    const call = container.querySelector(
      '.logbook-form .settings-input-row input',
    ) as HTMLInputElement
    expect(call.value).toBe('K0ABC')
    fireEvent.change(call, { target: { value: 'K0ABD' } })
    fireEvent.click(
      container.querySelector('.logbook-form button[type="submit"]') as HTMLButtonElement,
    )

    const editQso = api.editQso as ReturnType<typeof vi.fn>
    await waitFor(() => expect(editQso).toHaveBeenCalled())
    const sent = editQso.mock.calls[0][1] as Record<string, unknown>
    // Positive control, with DISAGREEING values: the payload really is a busted-call fix and
    // really did change something, so the two absences below cannot pass on an empty payload.
    expect(sent.call).toBe('K0ABD')
    expect(sent.band).toBe('70cm')
    expect(sent.propMode, 'the form must not carry PROP_MODE — blank would mean clear').toBe(
      undefined,
    )
    expect(sent.satName, 'the form must not carry SAT_NAME — blank would mean clear').toBe(
      undefined,
    )
    // …and the stored tag is what the backend restores from, untouched by this save: the row
    // the form was opened on still carries it.
    expect((editQso.mock.calls[0][0] as Record<string, unknown>).satName).toBe('RS-44')
  })
})

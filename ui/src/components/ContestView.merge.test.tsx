// @vitest-environment jsdom
//
// THE END-OF-CONTEST MERGE HAS A BUTTON.
//
// §11 item 5 promised the merge would be "one click". Batch 5 shipped
// `Engine::fd_merge_to_general`, `fd_set_upload`, the tauri commands and the default-OFF
// upload control — and nothing on screen called them, so the whole feature was
// unreachable. Failing-first: every assertion in this file was watched failing against
// that build (no button, no report, no count) before the control landed.
//
// The three properties that matter, and each is here because the merge WRITES INTO THE
// OPERATOR'S GENERAL LOGBOOK:
//
//   1. it says what it will do BEFORE it does it — how many contacts, to which logbook;
//   2. it reports what it DID — added versus already there;
//   3. a second press is VISIBLY a no-op. The merge is idempotent (batch 5 proved that
//      across a real save/reload), but an idempotent action with no report is
//      indistinguishable from a broken button, and the operator's next move after
//      "nothing happened" is to press it again or to merge by hand.
//
// It also renders WITH the upload switch and its ClubLog hint, because the switch decides
// whether what this button merges is queued for upload, and the hint names the limitation
// neither of them closes.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup, fireEvent } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, FdMergeReport } from '../types'

const fdMergeToGeneral = vi.fn<() => Promise<FdMergeReport>>()

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  fdMergeToGeneral: () => fdMergeToGeneral(),
  saveTextToDownloads: vi.fn(async () => {}),
  openPanelWindow: vi.fn(async () => {}),
}))

/** The ClubLog limitation, as the ENGINE words it — it travels on the DTO rather than
 *  living in a catalog, so a renderer cannot show the switch without it. */
const HINT =
  "ClubLog's catch-up sweep re-queues every logged QSO it never accepted the next time " +
  'you save a ClubLog password, regardless of this switch.'

const qso = (call: string) => ({
  call,
  class: '2A',
  section: 'WI',
  band: '20m',
  mode: 'DIG',
  whenUnix: 1_750_000_000,
  mex: '3A WI',
})

const FD = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
    ],
    running: false,
    state: 'sp',
    qsoCount: 3,
    sections: 1,
    points: 6,
    log: [qso('W1AW'), qso('K9ABC'), qso('N0XYZ')],
    event: 'arrlfd',
    upload: { enabled: false, destinations: [], available: ['wrl', 'qrz', 'clublog'], hint: HINT },
    ...over,
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const show = async (fd: FieldDayStatus = FD()) => {
  render(<ContestView fieldDay={fd} tier="FT8" onSetMode={() => {}} />)
  await settle()
}

beforeEach(() => fdMergeToGeneral.mockReset())
afterEach(cleanup)

describe('the end-of-contest merge is reachable, and says what it will do', () => {
  it('names the count and the destination before it is pressed', async () => {
    await show()
    // The whole point of the BEFORE half: the operator reads the number of contacts and
    // where they are going off the button itself, not off a confirmation they have to
    // trigger to see.
    const btn = screen.getByRole('button', { name: /Merge 3 contacts into my logbook/ })
    expect(btn).toBeTruthy()
    expect(btn.getAttribute('title')).toContain('general logbook')
    // No report until something has actually happened — an empty report line would read
    // as a merge that ran.
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('agrees with the log when there is one contact, and refuses when there are none', async () => {
    await show(FD({ log: [qso('W1AW')], qsoCount: 1 }))
    expect(screen.getByRole('button', { name: /Merge 1 contact into my logbook/ })).toBeTruthy()
    cleanup()
    await show(FD({ log: [], qsoCount: 0 }))
    const empty = screen.getByRole('button', { name: /Merge 0 contacts into my logbook/ })
    expect((empty as HTMLButtonElement).disabled).toBe(true)
  })

  it('reports what it did, and a second press is visibly a no-op', async () => {
    fdMergeToGeneral.mockResolvedValueOnce({ added: 3, already: 0, refused: 0, queued: false })
    await show()

    fireEvent.click(screen.getByRole('button', { name: /Merge 3 contacts/ }))
    await settle()
    expect(fdMergeToGeneral).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('status').textContent).toContain('Added 3')
    expect(screen.getByRole('status').textContent).toContain('0 already in your logbook')

    // The SECOND press. The merge is idempotent, so the engine adds nothing — and the
    // operator has to be able to SEE that rather than infer it from a screen that did
    // not change.
    fdMergeToGeneral.mockResolvedValueOnce({ added: 0, already: 3, refused: 0, queued: false })
    fireEvent.click(screen.getByRole('button', { name: /Merge 3 contacts/ }))
    await settle()
    expect(fdMergeToGeneral).toHaveBeenCalledTimes(2)
    const after = screen.getByRole('status').textContent ?? ''
    expect(after).toContain('Added 0')
    expect(after).toContain('3 already in your logbook')
  })

  it('says when the merged rows were also queued, and when rows were refused', async () => {
    fdMergeToGeneral.mockResolvedValueOnce({ added: 2, already: 0, refused: 1, queued: true })
    await show()
    fireEvent.click(screen.getByRole('button', { name: /Merge 3 contacts/ }))
    await settle()
    const line = screen.getByRole('status').textContent ?? ''
    // A refusal is REPORTED, never silent — a count that does not add up must be visible.
    expect(line).toContain('1 refused')
    expect(line).toContain('queued for upload')
  })

  it("surfaces the engine's own refusal rather than a second copy of it", async () => {
    fdMergeToGeneral.mockRejectedValueOnce('Field Day mode is not active')
    await show()
    fireEvent.click(screen.getByRole('button', { name: /Merge 3 contacts/ }))
    await settle()
    expect(screen.getByRole('alert').textContent).toContain('Field Day mode is not active')
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('renders with the upload switch and the ClubLog limitation it does not close', async () => {
    await show()
    // The operator must see the limitation in the same place they trigger the merge:
    // "upload: off" plus a ClubLog upload anyway is us misleading them, not ClubLog
    // surprising them.
    expect(screen.getByRole('button', { name: /Merge 3 contacts/ })).toBeTruthy()
    expect(screen.getByRole('switch')).toBeTruthy()
    expect(screen.getByRole('note').textContent).toContain('catch-up sweep')
  })
})

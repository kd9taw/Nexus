// @vitest-environment jsdom
//
// THE CONTEST LOGGER WINDOW, FROM THE CONTEST SCREEN'S SIDE.
//
//   - it opens from the docked screen's header, and from the torn-off scoreboard — the window with
//     the operator box, the sections and who is on what band — and from nowhere it already is;
//   - in the logger window the contest switch, Running / S&P and the scoring panel are read-only
//     (Running calls CQ in the digital cockpits; the switch and the scoring save through the
//     whole-settings path, which keeps an over already planned from keying), a line says where
//     they are, and everything else on the screen still works.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup, within } from '@testing-library/react'
import { ContestView, FieldDayScoreboard } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' })),
  setSettings: vi.fn(async () => ({})),
  getFdRuleset: vi.fn(async () => null),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** ARRL Field Day, running, so the bonus panel and its power chips are on the screen. */
const fieldDay = {
  running: true,
  state: 'Running',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'arrlfd',
  composing: [
    { key: 'CLASS', raw: '2A' },
    { key: 'SECTION', raw: 'IL' },
  ],
  receives: [],
} as unknown as FieldDayStatus

const settle = () =>
  act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the contest logger window, from the contest screen', () => {
  it("opens from the docked screen's header", async () => {
    render(<ContestView fieldDay={fieldDay} fdActive />)
    await settle()
    fireEvent.click(screen.getByRole('button', { name: t('fieldDay.logger.label') }))
    expect(api.openPanelWindow).toHaveBeenCalledWith('contestlog')
  })

  it('opens from the torn-off scoreboard, and not twice from the docked one', async () => {
    const settings = { ...defaultSettings, fdOperator: 'KD9TAW' } as unknown as Settings
    render(<FieldDayScoreboard fieldDay={fieldDay} settings={settings} onSaveOperator={() => {}} detached />)
    fireEvent.click(screen.getByRole('button', { name: t('fieldDay.logger.label') }))
    expect(api.openPanelWindow).toHaveBeenCalledWith('contestlog')
    cleanup()
    // Docked, the scoreboard keeps its own pop-out and leaves the logger to the screen's header.
    render(<FieldDayScoreboard fieldDay={fieldDay} settings={settings} onSaveOperator={() => {}} />)
    expect(screen.queryByRole('button', { name: t('fieldDay.logger.label') })).toBeNull()
    expect(screen.getByRole('button', { name: t('fieldDay.popOut.label') })).toBeTruthy()
  })

  it('in the logger window: the switch, Running / S&P and scoring are read-only, and the line says so', async () => {
    const onSetMode = vi.fn()
    render(<ContestView fieldDay={fieldDay} fdActive onSetMode={onSetMode} logger />)
    await settle()
    const sw = screen.getByRole('switch', { name: t('settings.fieldDay.mode.aria.disable') }) as HTMLButtonElement
    const run = screen.getByRole('button', { name: t('fieldDay.role.running') }) as HTMLButtonElement
    const sp = screen.getByRole('button', { name: t('fieldDay.role.sp') }) as HTMLButtonElement
    const bonuses = document.querySelectorAll<HTMLInputElement>('.fd-bonus-row input[type="checkbox"]')
    const plans = document.querySelectorAll<HTMLButtonElement>('.fd-bonus-plan')
    const power = document.querySelectorAll<HTMLButtonElement>('.fd-power-chip')
    // Control: the panel is on screen, so a disabled count of zero cannot pass for "none".
    expect(bonuses.length).toBeGreaterThan(10)
    expect(power.length).toBe(3)
    for (const el of [sw, run, sp, ...bonuses, ...plans, ...power]) {
      expect(el.disabled, `${el.className || el.textContent} is read-only in the logger window`).toBe(true)
      fireEvent.click(el)
    }
    await settle()
    expect(onSetMode).not.toHaveBeenCalled()
    expect(api.setSettings).not.toHaveBeenCalled()
    expect(screen.getByText(t('fieldDay.logger.note'))).toBeTruthy()
    // The window does not offer itself…
    expect(screen.queryByRole('button', { name: t('fieldDay.logger.label') })).toBeNull()
    // …and the rest of the screen works: the exports and the operator box.
    const exports = document.querySelector('.fd-export')!
    expect((within(exports as HTMLElement).getByRole('button', { name: t('fieldDay.export.cabrillo.label') }) as HTMLButtonElement).disabled).toBe(false)
    expect((screen.getByRole('textbox', { name: t('fieldDay.operator.aria') }) as HTMLInputElement).readOnly).toBe(false)
  })

  it('on the docked screen the same controls stay live (control)', async () => {
    const onSetMode = vi.fn()
    render(<ContestView fieldDay={fieldDay} fdActive onSetMode={onSetMode} />)
    await settle()
    const sp = screen.getByRole('button', { name: t('fieldDay.role.sp') }) as HTMLButtonElement
    expect(sp.disabled).toBe(false)
    fireEvent.click(sp)
    expect(onSetMode).toHaveBeenCalledWith('fieldday-sp')
    expect(screen.queryByText(t('fieldDay.logger.note'))).toBeNull()
  })
})

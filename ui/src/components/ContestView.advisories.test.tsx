// @vitest-environment jsdom
//
// The ContestView event banner hosts the warn-only advisories (FdAdvisories) —
// the integration half of FdAdvisories.test.tsx: the banner actually mounts the
// component and feeds it the DTO props App supplies. Failing-first: the banned-
// chip test was watched failing with the banner mount removed before landing.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, act, cleanup, waitFor, within } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'
import type { FdRulesetDto } from '../api'
import { contestName } from '../fdEvent'
import { t } from '../i18n'

vi.mock('../api', () => ({
  // The contest screen's Removed list, read on mount: none removed.
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  // The keyboard modes' sub-mode, read the way the PSK cockpit reads it.
  getPskState: vi.fn(async () => ({ mode: 'qpsk31' })),
}))

const WFD_RULES: FdRulesetDto = {
  event: 'wfd',
  rulesYear: 2026,
  bannedModes: ['FST4', 'FT4', 'FT8', 'JT4', 'JT9', 'JT65', 'Q65', 'MSK144', 'WSPR', 'FST4W', 'ECHO'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
}
const SFD_RULES: FdRulesetDto = { ...WFD_RULES, event: 'arrlfd', bannedModes: [] }

const fd = (over: Partial<FieldDayStatus> = {}): FieldDayStatus => ({
  composing: [
    { key: 'CLASS', raw: '1O' },
    { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
  ],
  running: true,
  state: 'sp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'wfd',
  rulesYear: 2026,
  rulesGenerated: '2026-08-29T00:00:00Z',
  ...over,
})

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(cleanup)

/** What the banner shows right after the contest's window: where its advisories stand. */
const afterWindow = () => document.querySelector('.fd-event-banner > .fd-event-subtitle')!.nextElementSibling

describe('the event banner hosts the warn-only advisories', () => {
  // ⭐ A CHIP, NOT THE AMBER LINE (2026-10-12): the operator found the line "very verbose and yellow
  // and distracting", and picked the log line's own chip here too: "FT8 · no credit", the whole
  // sentence on hover.
  it('says a banned mode in the no-credit chip, FT8 at WFD, inside the banner', async () => {
    render(
      <ContestView fieldDay={fd()} onSetMode={() => {}} fdActive fdRuleset={WFD_RULES} tier="FT8" />,
    )
    await settle()
    const sentence = t('fieldDay.advisory.banned', { mode: 'FT8', event: contestName('wfd'), year: 2026 })
    expect(afterWindow()?.textContent).toBe('FT8 · no credit')
    const banner = document.querySelector<HTMLElement>('.fd-event-banner')!
    expect(within(banner).getByRole('status', { name: 'FT8 · no credit', description: sentence })).toBe(afterWindow())
    expect(afterWindow()!.getAttribute('title')).toBe(sentence)
    // The line is gone: the sentence is not drawn in the banner.
    expect(banner.querySelector('.fd-advisory')).toBeNull()
    expect(banner.textContent).not.toContain(sentence)
  })

  it('shows no chip at ARRL FD, and none with the master switch off', async () => {
    render(
      <ContestView
        fieldDay={fd({ event: 'arrlfd' })}
        onSetMode={() => {}}
        fdActive
        fdRuleset={SFD_RULES}
        tier="FT8"
      />,
    )
    await settle()
    expect(document.querySelector('.fd-advisory, .le-fd-nocredit')).toBeNull()
    cleanup()
    render(
      <ContestView fieldDay={fd()} onSetMode={() => {}} fdActive={false} fdRuleset={WFD_RULES} tier="FT8" />,
    )
    await settle()
    expect(document.querySelector('.fd-advisory, .le-fd-nocredit')).toBeNull()
  })

  it('assistance stays DORMANT with the shipped (all-allowed) rules even with live sources — and the restricted control shows through the same path', async () => {
    // Dormant: the DTO's assistanceOn is live, the shipped policy allows everything.
    render(
      <ContestView
        fieldDay={fd({ assistanceOn: ['DX cluster / RBN', 'PSK Reporter needs'] })}
        onSetMode={() => {}}
        fdActive
        fdRuleset={WFD_RULES}
        tier="CW"
      />,
    )
    await settle()
    expect(document.querySelector('.fd-advisory')).toBeNull()
    cleanup()
    // Positive control: a restricted ruleset flips the very same wiring visible.
    render(
      <ContestView
        fieldDay={fd({ assistanceOn: ['DX cluster / RBN', 'PSK Reporter needs'] })}
        onSetMode={() => {}}
        fdActive
        fdRuleset={{ ...WFD_RULES, clusterAllowed: false }}
        tier="CW"
      />,
    )
    await settle()
    expect(
      document.querySelector('.fd-event-banner .fd-advisory.cluster'),
      'restricted + live must surface in the banner',
    ).not.toBeNull()
  })
})

// ⭐ THE CHIP NAMES THE MODE THE STATION IS ON (2026-10-11). Rehearsing the Illinois QSO Party on
// phone, the header said "FT8 is not permitted at Illinois QSO Party" — the digital tier was still
// FT8, and the chip read the tier whatever the station was doing. Given the station's section, the
// chip reads that section's own mode: the tier on the digital section, the mode its contacts log
// under on the others. The rules line beside the switch says what the contest permits, as before.
describe('the banned-mode chip names the mode the station is on', () => {
  const ILQP_RULES: FdRulesetDto = { ...WFD_RULES, event: 'ilqp', bannedModes: ['FT8', 'FT4'] }
  const ilqp = () => fd({ event: 'ilqp' })
  const chip = () => document.querySelector('.fd-event-banner .le-fd-nocredit')
  const said = (mode: string) => t('fieldDay.advisory.banned', { mode, event: contestName('ilqp'), year: 2026 })

  it.each([
    ['phone', 'Ft8', null],
    ['cw', 'Ft8', null],
    ['rtty', 'Ft4', null],
    ['keyboard', 'Ft8', null],
    ['digital', 'Ft8', 'FT8'],
    ['digital', 'Ft4', 'FT4'],
    ['digital', 'Q65', null],
  ] as const)('on the %s section with the tier at %s', async (section, tier, banned) => {
    render(<ContestView fieldDay={ilqp()} onSetMode={() => {}} fdActive fdRuleset={ILQP_RULES} tier={tier} section={section} />)
    await settle()
    if (banned === null) {
      expect(chip(), `no chip on ${section}`).toBeNull()
      expect(document.querySelector('.fd-advisory'), `no line on ${section}`).toBeNull()
    } else {
      expect(afterWindow()?.textContent).toBe(`${banned} · no credit`)
      expect(chip()!.getAttribute('title')).toBe(said(banned))
    }
    expect(document.querySelector('.fd-mode-line.banned')?.textContent).toBe(t('fieldDay.mode.rule.banned', { modes: 'FT8, FT4' }))
  })

  it('a keyboard mode is named by the PSK cockpit’s own sub-mode, so a contest that bans it is heard', async () => {
    render(
      <ContestView
        fieldDay={ilqp()}
        onSetMode={() => {}}
        fdActive
        fdRuleset={{ ...ILQP_RULES, bannedModes: ['QPSK31'] }}
        tier="Ft8"
        section="keyboard"
      />,
    )
    await waitFor(() => expect(afterWindow()?.textContent).toBe('QPSK31 · no credit'))
    expect(chip()!.getAttribute('title')).toBe(said('QPSK31'))
  })

  it('on the JTTY screen the chip names JTTY, the keyboard mode the station reports, never the PSK cockpit’s', async () => {
    render(
      <ContestView
        fieldDay={ilqp()}
        onSetMode={() => {}}
        fdActive
        fdRuleset={{ ...ILQP_RULES, bannedModes: ['JTTY', 'PSK31', 'QPSK31'] }}
        tier="Ft8"
        section="keyboard"
        keyboardMode="JTTY"
      />,
    )
    await settle()
    expect(afterWindow()?.textContent).toBe('JTTY · no credit')
    expect(chip()!.getAttribute('title')).toBe(said('JTTY'))
  })

  it('with no section (the Remote page’s observation carries none) the chip reads the tier, as it always did', async () => {
    render(<ContestView fieldDay={ilqp()} onSetMode={() => {}} fdActive fdRuleset={ILQP_RULES} tier="Ft8" />)
    await settle()
    expect(afterWindow()?.textContent).toBe('FT8 · no credit')
    expect(chip()!.getAttribute('title')).toBe(said('FT8'))
  })
})

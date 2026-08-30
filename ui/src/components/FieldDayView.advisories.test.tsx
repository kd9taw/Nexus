// @vitest-environment jsdom
//
// The FieldDayView event banner hosts the warn-only advisories (FdAdvisories) —
// the integration half of FdAdvisories.test.tsx: the banner actually mounts the
// component and feeds it the DTO props App supplies. Failing-first: the banned-
// chip test was watched failing with the banner mount removed before landing.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { FieldDayView } from './FieldDayView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'
import type { FdRulesetDto } from '../api'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
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
  myClass: '1O',
  mySection: 'IL',
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

describe('the event banner hosts the warn-only advisories', () => {
  it('shows the banned-mode chip for FT8 at WFD, inside the banner', async () => {
    render(
      <FieldDayView fieldDay={fd()} onSetMode={() => {}} fdActive fdRuleset={WFD_RULES} tier="FT8" />,
    )
    await settle()
    const chip = document.querySelector('.fd-event-banner .fd-advisory.banned')
    expect(chip, 'FT8 at WFD must warn in the banner').not.toBeNull()
    expect(chip!.textContent).toContain('Winter Field Day')
  })

  it('shows no chip at ARRL FD, and none with the master switch off', async () => {
    render(
      <FieldDayView
        fieldDay={fd({ event: 'arrlfd' })}
        onSetMode={() => {}}
        fdActive
        fdRuleset={SFD_RULES}
        tier="FT8"
      />,
    )
    await settle()
    expect(document.querySelector('.fd-advisory')).toBeNull()
    cleanup()
    render(
      <FieldDayView fieldDay={fd()} onSetMode={() => {}} fdActive={false} fdRuleset={WFD_RULES} tier="FT8" />,
    )
    await settle()
    expect(document.querySelector('.fd-advisory')).toBeNull()
  })

  it('assistance stays DORMANT with the shipped (all-allowed) rules even with live sources — and the restricted control shows through the same path', async () => {
    // Dormant: the DTO's assistanceOn is live, the shipped policy allows everything.
    render(
      <FieldDayView
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
      <FieldDayView
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

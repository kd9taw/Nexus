// @vitest-environment jsdom
//
// The warn-only FD advisories, driven by FIXTURE DTOs (the component is pure by
// design — see FdAdvisories.tsx). Two properties matter enough to pin:
//
//   WARN, NEVER REMOVE — the advisory is a passive status line. Nothing here
//   renders a control, a `disabled`, or a panel-vocabulary id.
//
//   DORMANT = INVISIBLE — the 2026 seed ships assistance fully allowed, so the
//   assistance advisory renders NOWHERE today. That invisibility is pinned WITH
//   a restricted-fixture positive control right beside it, so the dormancy test
//   cannot pass vacuously (a component that never renders anything would fail
//   the control).
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { FdAdvisories } from './FdAdvisories'
import type { FdRulesetDto } from '../api'

// The shipped 2026 WFD ruleset facts, as get_fd_ruleset serves them (dormant
// assistance; the WSJT-suite ban).
const WFD: FdRulesetDto = {
  event: 'wfd',
  rulesYear: 2026,
  bannedModes: ['FST4', 'FT4', 'FT8', 'JT4', 'JT9', 'JT65', 'Q65', 'MSK144', 'WSPR', 'FST4W', 'ECHO'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
}

// ARRL FD bans nothing.
const SFD: FdRulesetDto = {
  event: 'arrlfd',
  rulesYear: 2026,
  bannedModes: [],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
}

const advisories = () => [...document.querySelectorAll('.fd-advisory')]

afterEach(cleanup)

describe('the banned-mode chip (the first consumer bannedModes has ever had)', () => {
  it('renders for a banned tier at WFD, citing event + rules year', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT8" />)
    const chip = document.querySelector('.fd-advisory.banned')
    expect(chip, 'FT8 is banned at WFD — the chip must render').not.toBeNull()
    expect(chip!.textContent).toContain('FT8')
    expect(chip!.textContent).toContain('Winter Field Day')
    expect(chip!.textContent).toContain('2026')
  })

  it('is absent for a legal tier at WFD (FT2 is not a WSJT mode)', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT2" />)
    expect(advisories()).toHaveLength(0)
  })

  it('is absent at ARRL FD (bans nothing), and absent with the master switch off', () => {
    render(<FdAdvisories fdActive ruleset={SFD} activeMode="FT8" />)
    expect(advisories()).toHaveLength(0)
    cleanup()
    render(<FdAdvisories fdActive={false} ruleset={WFD} activeMode="FT8" />)
    expect(advisories()).toHaveLength(0)
  })

  it('warns only — no control, no disabled, anywhere', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT8" />)
    expect(advisories().length).toBeGreaterThan(0)
    expect(document.querySelector('button, input, select, a, [disabled]')).toBeNull()
  })
})

describe('the assistance advisory', () => {
  const LIVE = ['AI CW decoder', 'DX cluster / RBN', 'PSK Reporter needs']

  it('DORMANT (the shipped 2026 seed): everything allowed ⇒ invisible even with every source live', () => {
    render(<FdAdvisories fdActive ruleset={WFD} assistanceOn={LIVE} showAssistance />)
    expect(advisories()).toHaveLength(0)
  })

  it('positive control: a restricted fixture with the same live sources IS visible', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false, spottingAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={LIVE} showAssistance />)
    const cluster = document.querySelector('.fd-advisory.cluster')
    const spotting = document.querySelector('.fd-advisory.spotting')
    expect(cluster, 'cluster restricted + cluster live ⇒ advisory').not.toBeNull()
    expect(cluster!.textContent).toContain('DX cluster / RBN')
    expect(cluster!.textContent).toContain('2026')
    expect(spotting, 'spotting restricted + spot feeds live ⇒ advisory').not.toBeNull()
    expect(spotting!.textContent).toContain('PSK Reporter needs')
  })

  it('restricted but nothing relevant live ⇒ invisible (the advisory cites live sources, never hypothetical ones)', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false, spottingAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={[]} showAssistance />)
    expect(advisories()).toHaveLength(0)
  })

  it('never renders on a surface that did not opt in (the cockpit header hosts only the banned chip)', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={LIVE} />)
    expect(advisories()).toHaveLength(0)
  })
})

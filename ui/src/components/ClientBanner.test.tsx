// @vitest-environment jsdom
// The banner of a computer driving another station. On a station's own screen it must say
// nothing and ask once; on a driving computer it must say which station and how the link stands,
// in every state the backend names, with why a refusal happened, and Leave must start this
// computer's Nexus again as itself. In a test build it also carries the road switch, which a
// shipped build's status never names, so it draws none there.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { BenchPosition, ClientStatus } from '../api'
import { EN } from '../i18n/en'

let desktop = true
const mockStatus = vi.fn<() => Promise<ClientStatus | null>>()
const mockLeave = vi.fn<(pairAgain?: boolean) => Promise<void>>()
const mockSwitch = vi.fn<(to: BenchPosition) => Promise<BenchPosition>>()
const mockStillHere = vi.fn<() => Promise<void>>()
const mockTake = vi.fn<() => Promise<void>>()
vi.mock('../api', () => ({
  isTauri: () => desktop,
  clientStatus: () => mockStatus(),
  leaveClientMode: (pairAgain?: boolean) => mockLeave(pairAgain),
  benchRoadSwitch: (to: BenchPosition) => mockSwitch(to),
  clientStillHere: () => mockStillHere(),
  clientTakeControl: () => mockTake(),
}))
import { ClientBanner, ClientInput, CLIENT_STATUS_EVERY_MS, STILL_HERE_EVERY_MS } from './ClientBanner'

const AT = '192.168.1.20:42075'
const fill = (text: string, values: Record<string, string>) =>
  text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => values[name] ?? '')

beforeEach(() => {
  desktop = true
  vi.useFakeTimers()
  mockStatus.mockReset()
  mockLeave.mockReset().mockResolvedValue(undefined)
  mockSwitch.mockReset()
  mockStillHere.mockReset().mockResolvedValue(undefined)
  mockTake.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** Renders the banner and lets its first question be answered. */
async function shown(): Promise<HTMLElement> {
  const { container } = render(<ClientBanner />)
  await act(async () => {
    await Promise.resolve()
  })
  return container
}

describe('on a station’s own screen', () => {
  it('shows nothing, and asks once', async () => {
    mockStatus.mockResolvedValue(null)
    const container = await shown()
    expect(container.querySelector('.client-banner')).toBeNull()
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS * 10)
    })
    expect(mockStatus).toHaveBeenCalledTimes(1)
  })

  it('outside the desktop app it asks nothing at all', async () => {
    desktop = false
    const container = await shown()
    expect(container.querySelector('.client-banner')).toBeNull()
    expect(mockStatus).not.toHaveBeenCalled()
  })
})

describe('on a computer driving a station', () => {
  it('says which station and how the link stands, and follows it as it changes', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(
      fill(EN['client.banner.live'], { station: AT }),
    )
    mockStatus.mockResolvedValue({ station: AT, phase: 'notLive', reason: null })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
      await Promise.resolve()
    })
    const banner = screen.getByRole('status')
    expect(banner.textContent).toContain(fill(EN['client.banner.notLive'], { station: AT }))
    expect(banner.getAttribute('data-phase')).toBe('notLive')
  })

  it('names each state: connecting, another device in control', async () => {
    for (const [phase, key] of [
      ['connecting', 'client.banner.connecting'],
      ['busy', 'client.banner.busy'],
    ] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase, reason: null })
      await shown()
      expect(screen.getByRole('status').textContent).toContain(fill(EN[key], { station: AT }))
      cleanup()
    }
  })

  it('a refusal says why, in the words of Stations on this network where they have them', async () => {
    for (const [reason, sentence] of [
      ['updateStation', EN['lanWindow.reason.updateStation']],
      ['keyChanged', EN['lanWindow.reason.keyChanged']],
      ['nameUnresolved', EN['lanWindow.reason.nameUnresolved']],
      ['otherBuild', EN['client.reason.otherBuild']],
      ['localPermissionRequired', EN['client.reason.localPermissionRequired']],
      ['somethingNew', EN['client.reason.unknown']],
    ] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason })
      await shown()
      expect(screen.getByRole('status').textContent).toContain(
        fill(EN['client.banner.refused'], { station: AT, reason: sentence }),
      )
      cleanup()
    }
  })

  // The lockout after an update here: the station refuses the newer build. The banner must say
  // which two versions disagree and which side to update, not only "the other one".
  it('a refusal for another build names both versions and what to do', async () => {
    for (const [reason, key, computer, station] of [
      ['updateStation', 'client.reason.builds.updateStation', '1.20.0', '1.19.0'],
      ['updateComputer', 'client.reason.builds.updateComputer', '1.19.0', '1.20.0'],
      ['otherBuild', 'client.reason.builds.otherBuild', '1.20.0-test1', '1.20.0-test2'],
    ] as const) {
      mockStatus.mockResolvedValue({
        station: AT,
        phase: 'refused',
        reason,
        computerBuild: computer,
        stationBuild: station,
      })
      await shown()
      const text = screen.getByRole('status').textContent ?? ''
      expect(text).toContain(fill(EN[key], { computer, station }))
      expect(text).toContain(computer)
      expect(text).toContain(station)
      cleanup()
    }
    // CONTROL: the station's version not heard (a refusal before its welcome): the plain sentence.
    mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason: 'updateStation', computerBuild: '1.20.0' })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(EN['lanWindow.reason.updateStation'])
  })

  it('Leave starts this computer’s Nexus again as itself, once', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await shown()
    const leave = screen.getByRole('button', { name: EN['client.banner.leave'] })
    fireEvent.click(leave)
    expect(mockLeave).toHaveBeenCalledTimes(1)
    expect(mockLeave).toHaveBeenCalledWith(undefined)
    expect((leave as HTMLButtonElement).disabled).toBe(true)
  })

  it('a station that no longer knows this computer, or answers with another key, offers Pair again: Leave, to pair again', async () => {
    for (const reason of ['notPaired', 'keyChanged'] as const) {
      mockLeave.mockClear()
      mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason })
      await shown()
      const again = screen.getByRole('button', { name: EN['client.banner.pairAgain'] })
      fireEvent.click(again)
      expect(mockLeave, reason).toHaveBeenCalledWith(true)
      expect((again as HTMLButtonElement).disabled).toBe(true)
      expect((screen.getByRole('button', { name: EN['client.banner.leave'] }) as HTMLButtonElement).disabled).toBe(true)
      cleanup()
    }
    // CONTROL: any other refusal, and a station signed in to from elsewhere, offer only Leave.
    for (const status of [
      { station: AT, phase: 'refused', reason: 'updateStation' },
      { station: AT, phase: 'live', reason: null },
      { station: AT, phase: 'refused', reason: 'notPaired', away: true },
    ] as const) {
      mockStatus.mockResolvedValue(status as ClientStatus)
      await shown()
      expect(screen.queryByRole('button', { name: EN['client.banner.pairAgain'] }), JSON.stringify(status)).toBeNull()
      cleanup()
    }
  })
})

describe('this computer’s over, as the station says it', () => {
  const gaps = EN['client.over.gaps'] as { one: string; other: string }

  it('counts the holes on the air, in amber, and says nothing of an over with none', async () => {
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      over: { phase: 'onAir', holes: 1, holeMs: 120 },
    })
    const container = await shown()
    const over = container.querySelector('.client-banner-over')
    expect(over?.textContent).toBe(fill(gaps.one, { count: '1', seconds: '0.1' }))
    expect(over?.getAttribute('data-over')).toBe('onAir')
    cleanup()
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      over: { phase: 'onAir', holes: 3, holeMs: 450 },
    })
    const more = await shown()
    expect(more.querySelector('.client-banner-over')?.textContent).toBe(
      fill(gaps.other, { count: '3', seconds: '0.5' }),
    )
    cleanup()
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      over: { phase: 'onAir', holes: 0, holeMs: 0 },
    })
    const none = await shown()
    expect(none.querySelector('.client-banner-over')).toBeNull()
  })

  it('says why an over ended, each end in its own words, and when the station went quiet', async () => {
    const ends = [
      ['audioGap', 'client.over.ended.audioGap'],
      ['pttGap', 'client.over.ended.pttGap'],
      ['stopped', 'client.over.ended.stopped'],
      ['presence', 'client.over.ended.presence'],
      ['ceiling', 'client.over.ended.ceiling'],
      ['watchdog', 'client.over.ended.watchdog'],
      ['routeChanged', 'client.over.ended.routeChanged'],
      ['refused', 'client.over.ended.refused'],
    ] as const
    const said = new Set<string>()
    for (const [why, key] of ends) {
      mockStatus.mockResolvedValue({
        station: AT,
        phase: 'live',
        reason: null,
        over: { phase: 'ended', why },
      })
      const container = await shown()
      const text = container.querySelector('.client-banner-over')?.textContent ?? ''
      expect(text).toBe(EN[key])
      said.add(text)
      cleanup()
    }
    expect(said.size).toBe(ends.length)
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      over: { phase: 'noWord' },
    })
    const quiet = await shown()
    expect(quiet.querySelector('.client-banner-over')?.textContent).toBe(EN['client.over.noWord'])
  })
})

describe('a test build’s road switch', () => {
  /** Lets the switch's answer, and the renders after it, land. */
  async function settled(): Promise<void> {
    await act(async () => {
      for (let i = 0; i < 5; i++) await Promise.resolve()
    })
  }

  it('is drawn only when the status names it: a shipped build’s never does', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    const container = await shown()
    expect(container.querySelector('.client-banner-bench')).toBeNull()
    expect(screen.queryByRole('group')).toBeNull()
    expect(screen.getAllByRole('button').map((b) => b.textContent)).toEqual([
      EN['client.banner.leave'],
    ])
    expect(screen.getByRole('status').hasAttribute('data-bench')).toBe(false)
    for (const key of ['client.bench.normal', 'client.bench.blockLink', 'client.bench.blockStopRoad'] as const) {
      expect(container.textContent).not.toContain(EN[key])
    }
    expect(mockSwitch).not.toHaveBeenCalled()
  })

  it('moves with one click each, marks a blocked road, and offers one road at a time', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, bench: 'normal' })
    const container = await shown()
    const banner = screen.getByRole('status')
    const group = within(banner).getByRole('group', { name: EN['client.bench.label'] })
    const button = (key: 'client.bench.normal' | 'client.bench.blockLink' | 'client.bench.blockStopRoad') =>
      within(group).getByRole('button', { name: EN[key] }) as HTMLButtonElement
    const pressed = () =>
      within(group)
        .getAllByRole('button')
        .filter((b) => b.getAttribute('aria-pressed') === 'true')
        .map((b) => b.textContent)
    const mark = () => container.querySelector('.client-banner-bench-mark')?.textContent ?? null

    expect(pressed()).toEqual([EN['client.bench.normal']])
    expect(banner.getAttribute('data-bench')).toBe('normal')
    expect(mark()).toBeNull()
    expect([button('client.bench.blockLink').disabled, button('client.bench.blockStopRoad').disabled]).toEqual([false, false])

    for (const [key, to, sentence, other] of [
      ['client.bench.blockLink', 'blockLink', EN['client.bench.blocked.link'], 'client.bench.blockStopRoad'],
      ['client.bench.blockStopRoad', 'blockStopRoad', EN['client.bench.blocked.stopRoad'], 'client.bench.blockLink'],
    ] as const) {
      mockSwitch.mockResolvedValueOnce(to)
      fireEvent.click(button(key))
      await settled()
      expect(mockSwitch).toHaveBeenLastCalledWith(to)
      // The switch's own answer is drawn at once, before the next ask after the link.
      expect(pressed()).toEqual([EN[key]])
      expect(banner.getAttribute('data-bench')).toBe(to)
      expect(mark()).toBe(sentence)
      expect(button(other).disabled).toBe(true)
      expect(button('client.bench.normal').disabled).toBe(false)

      mockSwitch.mockResolvedValueOnce('normal')
      fireEvent.click(button('client.bench.normal'))
      await settled()
      expect(mockSwitch).toHaveBeenLastCalledWith('normal')
      expect(pressed()).toEqual([EN['client.bench.normal']])
      expect(mark()).toBeNull()
    }
  })

  it('a move the switch refuses changes nothing drawn, and the status then says where it is', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, bench: 'normal' })
    const container = await shown()
    mockSwitch.mockRejectedValueOnce('oneRoadAtATime')
    fireEvent.click(screen.getByRole('button', { name: EN['client.bench.blockLink'] }))
    await settled()
    expect(screen.getByRole('status').getAttribute('data-bench')).toBe('normal')
    expect(container.querySelector('.client-banner-bench-mark')).toBeNull()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, bench: 'blockStopRoad' })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
      await Promise.resolve()
    })
    expect(screen.getByRole('status').getAttribute('data-bench')).toBe('blockStopRoad')
    expect(container.querySelector('.client-banner-bench-mark')?.textContent).toBe(
      EN['client.bench.blocked.stopRoad'],
    )
  })
})

describe('over the internet road', () => {
  it('says it is the internet, and when this network blocks the datagram roads, that transmitting is off', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: true, udpBlocked: true })
    await shown()
    const banner = screen.getByRole('status')
    expect(banner.textContent).toContain(EN['client.banner.internet.live'])
    expect(banner.textContent).not.toContain(AT)
    expect(banner.textContent).toContain(EN['client.banner.udpBlocked'])
    cleanup()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: true, udpBlocked: false })
    await shown()
    expect(screen.getByRole('status').textContent).not.toContain(EN['client.banner.udpBlocked'])
  })

  it('asks “Still there?” with a button that answers it, and after nobody did, offers Connect again', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: true, stillThere: true })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(EN['client.banner.stillThere'])
    fireEvent.click(screen.getByRole('button', { name: EN['client.banner.stillHere'] }))
    expect(mockStillHere).toHaveBeenCalled()
    cleanup()
    mockStillHere.mockClear()
    mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason: 'idle', internet: true })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(EN['client.reason.idle'])
    fireEvent.click(screen.getByRole('button', { name: EN['client.banner.connectAgain'] }))
    expect(mockStillHere).toHaveBeenCalledTimes(1)
  })

  it('reports a click, a key or the wheel at most every few seconds, and only on the internet', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: true })
    await shown()
    fireEvent.pointerDown(window)
    fireEvent.keyDown(window, { key: 'a' })
    expect(mockStillHere).toHaveBeenCalledTimes(1)
    await act(async () => { vi.advanceTimersByTime(STILL_HERE_EVERY_MS) })
    fireEvent.wheel(window)
    expect(mockStillHere).toHaveBeenCalledTimes(2)
    cleanup()
    mockStillHere.mockClear()
    // CONTROL: on this network nothing is reported.
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: false })
    await shown()
    await act(async () => { vi.advanceTimersByTime(STILL_HERE_EVERY_MS) })
    fireEvent.pointerDown(window)
    expect(mockStillHere).not.toHaveBeenCalled()
  })

  it('a pop-out reports its input as the banner does, and draws nothing', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, internet: true })
    const { container } = render(<ClientInput />)
    await act(async () => { await Promise.resolve() })
    expect(container.innerHTML).toBe('')
    fireEvent.keyDown(window, { key: 'a' })
    expect(mockStillHere).toHaveBeenCalledTimes(1)
    await act(async () => { vi.advanceTimersByTime(STILL_HERE_EVERY_MS) })
    fireEvent.pointerDown(window)
    expect(mockStillHere).toHaveBeenCalledTimes(2)
    cleanup()
    mockStillHere.mockClear()
    // CONTROL: on this network, and on a station's own screen, nothing is reported.
    for (const status of [{ station: AT, phase: 'live' as const, reason: null, internet: false }, null]) {
      mockStatus.mockResolvedValue(status)
      render(<ClientInput />)
      await act(async () => { await Promise.resolve() })
      await act(async () => { vi.advanceTimersByTime(STILL_HERE_EVERY_MS) })
      fireEvent.pointerDown(window)
      cleanup()
    }
    expect(mockStillHere).not.toHaveBeenCalled()
  })

  it('says each of the internet road’s refusals in its own words', async () => {
    for (const reason of ['internetNotIncluded', 'notApprovedThere', 'waitingApproval', 'internetNotReady',
      'stationUnavailable', 'serviceUnavailable', 'tryLater', 'accessDenied', 'pipeClosed'] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason })
      await shown()
      expect(screen.getByRole('status').textContent, reason).toContain((EN as Record<string, unknown>)[`client.reason.${reason}`] as string)
      cleanup()
    }
  })

  it('says a road held for the station’s key, or turned off for another one, in its own words', async () => {
    for (const reason of ['stationKeyUnconfirmed', 'stationKeyDisagrees'] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason })
      await shown()
      expect(screen.getByRole('status').textContent, reason).toContain(EN[`client.reason.${reason}`])
      cleanup()
    }
  })
})


describe('Take control, while another device holds the station', () => {
  const take = () => screen.queryByRole('button', { name: EN['client.banner.takeControl'] })

  it('is asked inline first, and only the confirmed answer takes control', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'busy', reason: null })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(fill(EN['client.banner.busy'], { station: AT }))
    fireEvent.click(take()!)
    const question = screen.getByRole('group', { name: EN['client.banner.takeControl'] })
    expect(question.textContent).toContain(EN['client.banner.takeControl.confirm'])
    expect(mockTake).not.toHaveBeenCalled()
    fireEvent.click(within(question).getByRole('button', { name: EN['client.banner.takeControl.cancel'] }))
    expect(screen.queryByRole('group', { name: EN['client.banner.takeControl'] })).toBeNull()
    expect(mockTake).not.toHaveBeenCalled()
    fireEvent.click(take()!)
    fireEvent.click(
      within(screen.getByRole('group', { name: EN['client.banner.takeControl'] })).getByRole('button', {
        name: EN['client.banner.takeControl'],
      }),
    )
    expect(mockTake).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('group', { name: EN['client.banner.takeControl'] })).toBeNull()
  })

  it('says another computer took control from this one, and offers to take it back', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'busy', reason: 'takenOver' })
    await shown()
    const said = screen.getByRole('status').textContent
    expect(said).toContain(EN['client.banner.takenOver'])
    expect(said).not.toContain(fill(EN['client.banner.busy'], { station: AT }))
    expect(take()).not.toBeNull()
  })

  it('is offered only while another device holds the station, and a question asked goes with it', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await shown()
    expect(take()).toBeNull()
    mockStatus.mockResolvedValue({ station: AT, phase: 'busy', reason: null })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
    })
    fireEvent.click(take()!)
    expect(screen.getByRole('group', { name: EN['client.banner.takeControl'] })).not.toBeNull()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
    })
    expect(screen.queryByRole('group', { name: EN['client.banner.takeControl'] })).toBeNull()
    expect(take()).toBeNull()
    mockStatus.mockResolvedValue({ station: AT, phase: 'busy', reason: null })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
    })
    expect(screen.queryByRole('group', { name: EN['client.banner.takeControl'] })).toBeNull()
    expect(mockTake).not.toHaveBeenCalled()
  })
})

describe('signing in again for the station', () => {
  it('says while Operate’s launch signs in again, and by what day to sign in when it did not; nothing otherwise', async () => {
    // 1 802 592 000 s is 2027-02-14 UTC.
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, signInBy: 1_802_592_000, signingAgain: true })
    await shown()
    let banner = screen.getByRole('status').textContent ?? ''
    expect(banner).toContain(EN['client.banner.signingAgain'])
    expect(banner, 'one line at a time').not.toContain(fill(EN['client.banner.signInBy'], { date: '2027-02-14' }))
    cleanup()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, signInBy: 1_802_592_000, signingAgain: false })
    await shown()
    banner = screen.getByRole('status').textContent ?? ''
    expect(banner).toContain(fill(EN['client.banner.signInBy'], { date: '2027-02-14' }))
    expect(banner).not.toContain(EN['client.banner.signingAgain'])
    cleanup()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, signInBy: null, signingAgain: false })
    await shown()
    banner = screen.getByRole('status').textContent ?? ''
    expect(banner).not.toContain(EN['client.banner.signingAgain'])
    expect(banner).not.toContain(fill(EN['client.banner.signInBy'], { date: '' }).slice(0, 12))
  })
})

describe('a station kept by its service id (signed in to from elsewhere)', () => {
  const AWAY = '20000000-0000-4000-8000-0000000000a1'
  it('says the internet is its road while it connects and when it is refused, and never shows its id', async () => {
    mockStatus.mockResolvedValue({ station: AWAY, phase: 'connecting', reason: null, away: true })
    await shown()
    let banner = screen.getByRole('status').textContent ?? ''
    expect(banner).toContain(EN['client.banner.internet.connecting'])
    expect(banner).not.toContain(AWAY)
    cleanup()
    mockStatus.mockResolvedValue({ station: AWAY, phase: 'refused', reason: 'internetNotIncluded', away: true })
    await shown()
    banner = screen.getByRole('status').textContent ?? ''
    expect(banner).toContain(fill(EN['client.banner.internet.refused'], { reason: EN['client.reason.internetNotIncluded'] }))
    expect(banner).not.toContain(AWAY)
    cleanup()
    mockStatus.mockResolvedValue({ station: AWAY, phase: 'live', reason: null, away: true, internet: true })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(EN['client.banner.internet.live'])
  })

  it('CONTROL: a station on this network still says where it is', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason: 'keyChanged', away: false })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(
      fill(EN['client.banner.refused'], { station: AT, reason: EN['lanWindow.reason.keyChanged'] }),
    )
  })
})

describe('why a PTT press here did not key the station (the client-status refusal reason)', () => {
  it('renders a plain message for each refusal value, beside the over line', async () => {
    const cases: Array<[NonNullable<ClientStatus['pttRefusal']>, string]> = [
      ['notPhone', EN['client.ptt.refusal.notPhone']],
      ['txDisabled', EN['client.ptt.refusal.txDisabled']],
      ['outsideLicence', EN['client.ptt.refusal.outsideLicence']],
      ['tuning', EN['client.ptt.refusal.tuning']],
      ['otherOwner', EN['client.ptt.refusal.otherOwner']],
      ['clockRepair', EN['client.ptt.refusal.clockRepair']],
      ['onHold', EN['client.ptt.refusal.onHold']],
      ['noPresence', EN['client.ptt.refusal.noPresence']],
      ['noFeed', EN['client.ptt.refusal.noFeed']],
      ['noAudio', EN['client.ptt.refusal.noAudio']],
    ]
    for (const [refusal, text] of cases) {
      mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, pttRefusal: refusal })
      const container = await shown()
      const line = container.querySelector(`.client-banner-over[data-ptt-refusal="${refusal}"]`)
      expect(line?.textContent, refusal).toBe(text)
      cleanup()
    }
  })

  it('shows nothing when a press would key (null) or the station build does not send the field', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, pttRefusal: null })
    let container = await shown()
    expect(container.querySelector('[data-ptt-refusal]')).toBeNull()
    cleanup()
    // Older station build: the field is simply absent.
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    container = await shown()
    expect(container.querySelector('[data-ptt-refusal]')).toBeNull()
  })
})

describe('the station holding this computer from transmit (the client-status hold)', () => {
  const holdLine = (container: HTMLElement) =>
    container.querySelector('.client-banner-over[data-tx-hold]')?.textContent

  it('says Allow to transmit at the shack, in any mode, with no time when the station sent none', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, txHold: { ownerNotified: false } })
    const container = await shown()
    expect(holdLine(container)).toBe(EN['client.ptt.refusal.onHold'])
  })

  it('adds the minutes left, rounded up, only when the station sent them', async () => {
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      txHold: { ownerNotified: true, leftSecs: 541 },
    })
    const container = await shown()
    expect(holdLine(container)).toBe(fill(EN['client.ptt.refusal.onHoldLeft'], { minutes: '10' }))
  })

  it('stands in for the PTT on-hold line rather than saying it twice', async () => {
    mockStatus.mockResolvedValue({
      station: AT,
      phase: 'live',
      reason: null,
      pttRefusal: 'onHold',
      txHold: { ownerNotified: true, leftSecs: 60 },
    })
    const container = await shown()
    expect(container.querySelector('[data-ptt-refusal]')).toBeNull()
    expect(holdLine(container)).toBe(fill(EN['client.ptt.refusal.onHoldLeft'], { minutes: '1' }))
  })

  it('shows nothing when not held, or from a station build that does not say', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, txHold: null })
    let container = await shown()
    expect(container.querySelector('[data-tx-hold]')).toBeNull()
    cleanup()
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null, pttRefusal: 'onHold' })
    container = await shown()
    expect(container.querySelector('[data-tx-hold]')).toBeNull()
    expect(container.querySelector('[data-ptt-refusal="onHold"]')?.textContent).toBe(
      EN['client.ptt.refusal.onHold'],
    )
  })
})

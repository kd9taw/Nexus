// @vitest-environment jsdom
//
// ★ EVERY OPERATOR-TYPED NUMBER, ON A COMMA-DECIMAL LOCALE. The Greek-Windows report, 2026-08.
//
// Most of the world writes `14,074`, not `14.074`. `parseFloat('14,074')` is `14` and reports
// success; `Number('14,074')` is `NaN`; `Number('')` is `0`. Before `parseOperatorNumber` there
// were four numeric input sites in the tree and only ONE of them handled it — `FrequencyReadout`,
// alone and undocumented. The other three were wrong, and one of them TRANSMITTED the wrong
// value:
//
//   - AprsCockpit beaconed `parseFloat(lat)`, so a Greek operator at 37,98 °N put 37 °N on the
//     air — a hundred kilometres of position error, radiated, with nothing to warn them;
//   - SpotDialog spotted the DX cluster on the wrong frequency;
//   - MemoriesView used bare `Number(v)` with NO NaN guard, so an entry the field rejected
//     committed the channel to 0 MHz.
//
// Each `it` below pins ONE site, with the comma case AND a dot-decimal positive control — the
// control is not decoration, it is what proves the fix did not simply break the locale that
// already worked.
//
// These tests were written to FAIL first. On the pre-fix tree: APRS beaconed (37, -23),
// SpotDialog posted 14, MemoriesView discarded the edit, and only FrequencyReadout was green.
//
// ⚠️ MemoriesView needed MORE than the shared parser, and the last block here is why. Its
// decimal fields were `<input type="number">`, which never hands JavaScript the typed text —
// the UA sanitises with LOCALE rules first, and on a comma-decimal locale `.` is the GROUP
// separator, so `14.074` arrives as `"14074"`. jsdom does not implement that layer, so no
// behavioural test in this file can see it; the guard has to assert the input TYPE instead.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup, within } from '@testing-library/react'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    arm: 'off' as const,
    audioPeak: 0,
    lastAudioUnix: null,
    drains: 0,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    lastFrameSeenUnix: null,
    framePeak: 0,
    maxFramePeak: 0,
    frameClippedSamples: 0,
    radioName: 'IC-9700',
    bandRadioCount: 1,
  })),
  getAprsIsStatus: vi.fn(async () => ({
    enabled: false,
    connected: false,
    verified: false,
    packets: 0,
    lastPacketUnix: null,
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
  postSpot: vi.fn(async () => {}),
}))

import { AprsCockpit } from './AprsCockpit'
import { SpotDialog } from './SpotDialog'
import { MemoriesView } from './MemoriesView'
import { FrequencyReadout } from './FrequencyReadout'
import { aprsSendBeacon, postSpot } from '../api'
import { addMemory, emptyBank, memoriesStore } from '../features/memories'

afterEach(cleanup)

// ── APRS: the one that goes on the air ────────────────────────────────────────────────────
describe('AprsCockpit beacon latitude/longitude', () => {
  beforeEach(() => vi.clearAllMocks())

  /** Mount and let the mount-time polls settle. */
  async function mountAprs() {
    const view = render(
      <AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />,
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
      await Promise.resolve()
    })
    return view
  }

  const latLon = (c: HTMLElement) => {
    const beacon = c.querySelector('.aprs-beacon') as HTMLElement
    const inputs = beacon.querySelectorAll('input')
    return { lat: inputs[0] as HTMLInputElement, lon: inputs[1] as HTMLInputElement }
  }

  it('beacons the position a comma-decimal operator actually typed', async () => {
    const { container } = await mountAprs()
    const { lat, lon } = latLon(container)
    fireEvent.change(lat, { target: { value: '37,98' } })
    fireEvent.change(lon, { target: { value: '-23,73' } })
    fireEvent.click(screen.getByRole('button', { name: /beacon/i }))
    await act(async () => {
      await Promise.resolve()
    })
    // ⚠️ THIS IS TRANSMITTED. parseFloat gave (37, -23) — Athens reported from a field
    // 110 km away, with the operator told only "Beacon queued".
    expect(
      vi.mocked(aprsSendBeacon).mock.calls[0]?.slice(0, 2),
      'A comma-decimal latitude must beacon the real position, not the integer part.',
    ).toEqual([37.98, -23.73])
  })

  it('POSITIVE CONTROL: a dot-decimal position is unchanged', async () => {
    const { container } = await mountAprs()
    const { lat, lon } = latLon(container)
    fireEvent.change(lat, { target: { value: '37.98' } })
    fireEvent.change(lon, { target: { value: '-23.73' } })
    fireEvent.click(screen.getByRole('button', { name: /beacon/i }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(vi.mocked(aprsSendBeacon).mock.calls[0]?.slice(0, 2)).toEqual([37.98, -23.73])
  })

  it('refuses garbage rather than transmitting it', async () => {
    const { container } = await mountAprs()
    const { lat, lon } = latLon(container)
    fireEvent.change(lat, { target: { value: '37,9,8' } })
    fireEvent.change(lon, { target: { value: '-23.73' } })
    fireEvent.click(screen.getByRole('button', { name: /beacon/i }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(aprsSendBeacon).not.toHaveBeenCalled()
    expect(screen.getByText(/valid latitude and longitude/i)).toBeTruthy()
  })
})

// ── SpotDialog: the wrong frequency on the cluster ────────────────────────────────────────
describe('SpotDialog frequency', () => {
  beforeEach(() => vi.clearAllMocks())

  const openSpot = () =>
    render(
      <SpotDialog
        open
        onClose={() => {}}
        initialCall="SV1ABC"
        freqMhz={14.074}
        defaultComment=""
      />,
    )

  const freqInput = () =>
    screen.getByText('Frequency (MHz)').parentElement?.querySelector(
      'input',
    ) as HTMLInputElement

  it('spots the frequency a comma-decimal operator typed', async () => {
    openSpot()
    fireEvent.change(freqInput(), { target: { value: '14,074' } })
    fireEvent.click(screen.getByRole('button', { name: /^spot/i }))
    await act(async () => {
      await Promise.resolve()
    })
    // parseFloat('14,074') is 14 — a spot on 14 MHz, outside the band, for everyone on the
    // cluster to see.
    expect(vi.mocked(postSpot).mock.calls[0]?.[0]).toBe(14.074)
  })

  it('POSITIVE CONTROL: a dot-decimal frequency still spots', async () => {
    openSpot()
    fireEvent.change(freqInput(), { target: { value: '14.074' } })
    fireEvent.click(screen.getByRole('button', { name: /^spot/i }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(vi.mocked(postSpot).mock.calls[0]?.[0]).toBe(14.074)
  })

  it('will not post a spot it could not parse', async () => {
    openSpot()
    fireEvent.change(freqInput(), { target: { value: '14.0.74' } })
    fireEvent.click(screen.getByRole('button', { name: /^spot/i }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(postSpot).not.toHaveBeenCalled()
  })
})

// ── MemoriesView: a locale-aware `type="number"` plus an unguarded `Number(v)` ────────────
describe('MemoriesView channel frequencies', () => {
  beforeEach(() => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 145.5, mode: 'FM', name: 'Repeater' }))
  })

  const mount = () =>
    render(<MemoriesView dialMhz={146.52} dialMode="FM" onRecall={() => {}} myGrid="EN52" />)

  /** Open the row's editor (the ✎ button) and return the labelled input inside it. */
  const editField = (container: HTMLElement, label: string) => {
    fireEvent.click(container.querySelector('.mv-row-edit') as HTMLElement)
    const field = Array.from(container.querySelectorAll('.mv-field')).find(
      (el) => el.querySelector('span')?.textContent === label,
    ) as HTMLElement
    return within(field).getByRole('textbox') as HTMLInputElement
  }

  it('stores the RX frequency a comma-decimal operator typed', () => {
    const { container } = mount()
    const rx = editField(container, 'RX MHz')
    fireEvent.change(rx, { target: { value: '14,074' } })
    fireEvent.blur(rx)
    expect(memoriesStore.get().memories[0].rxMhz).toBe(14.074)
  })

  it('POSITIVE CONTROL: a dot-decimal RX frequency still commits', () => {
    const { container } = mount()
    const rx = editField(container, 'RX MHz')
    fireEvent.change(rx, { target: { value: '14.074' } })
    fireEvent.blur(rx)
    expect(memoriesStore.get().memories[0].rxMhz).toBe(14.074)
  })

  it('REFUSES an unparseable frequency instead of storing 0', () => {
    // `Number('')` is 0 and `Number('abc')` is NaN — both were committed unguarded, so a
    // rejected edit silently moved the channel to 0 MHz (or to NaN, which renders blank).
    const { container } = mount()
    const rx = editField(container, 'RX MHz')
    fireEvent.change(rx, { target: { value: 'not a number' } })
    fireEvent.blur(rx)
    expect(memoriesStore.get().memories[0].rxMhz).toBe(145.5)
  })

  // ★ ROOT CAUSE. The guard the tests above cannot express, because jsdom does NOT implement
  // the UA's locale-aware sanitisation and a real Greek/German Windows does. `type="number"`
  // never hands JavaScript the typed text: on a comma-decimal locale `.` is the GROUP
  // separator, so `14.074` reaches `.value` as `"14074"` — valid, plausible, and 14 GHz off
  // frequency. Nothing downstream can recover from that, so the field itself has to be text.
  //
  // The decimal fields are CONDITIONALLY RENDERED, so this needs two configurations — and the
  // `expect(present)` line in each is what stops the real assertion passing vacuously on a
  // field that simply was not on screen.
  describe.each([
    {
      what: 'offset + CTCSS',
      mem: { offsetDir: 'minus' as const, offsetMhz: 0.6, toneMode: 'tsql' as const, ctcssEncHz: 88.5 },
      decimals: ['RX MHz', 'Offset MHz', 'CTCSS Hz'],
    },
    {
      what: 'odd split + DTCS',
      mem: { offsetDir: 'split' as const, txMhz: 145.9, toneMode: 'dtcs' as const, dtcsCode: 23 },
      decimals: ['RX MHz', 'TX MHz'],
    },
  ])('no decimal field is a `type="number"` input ($what)', ({ mem, decimals }) => {
    it('is text, so the operator’s literal keystrokes reach the parser', () => {
      memoriesStore.set(
        addMemory(emptyBank(), {
          rxMhz: 145.5,
          mode: 'FM',
          name: 'Repeater',
          // The offset/tone block is gated on the channel KIND — without this the fields
          // under test are not rendered and the check has nothing to look at.
          kind: 'repeater',
          ...mem,
        }),
      )
      const { container } = mount()
      fireEvent.click(container.querySelector('.mv-row-edit') as HTMLElement)
      const typeOf = (label: string) => {
        const field = Array.from(container.querySelectorAll('.mv-field')).find(
          (el) => el.querySelector('span')?.textContent === label,
        )
        return field ? (field.querySelector('input')?.getAttribute('type') ?? 'text') : null
      }
      for (const label of decimals) {
        // Rendered at all? Without this the assertion below is vacuous.
        expect(typeOf(label), `${label} is not on screen — this check proves nothing.`).not.toBeNull()
        expect(typeOf(label), `${label} must not be a type="number" input`).toBe('text')
      }
    })
  })

  it('POSITIVE CONTROL: the integer-valued fields ARE still number inputs', () => {
    // Deliberate, and it is what proves the check above can tell the two apart: a DTCS code
    // has no decimal separator, so it carries no locale hazard and keeps its spinner.
    memoriesStore.set(
      addMemory(emptyBank(), {
        rxMhz: 145.5,
        mode: 'FM',
        name: 'Repeater',
        kind: 'repeater',
        toneMode: 'dtcs',
        dtcsCode: 23,
      }),
    )
    const { container } = mount()
    fireEvent.click(container.querySelector('.mv-row-edit') as HTMLElement)
    const dtcs = Array.from(container.querySelectorAll('.mv-field')).find(
      (el) => el.querySelector('span')?.textContent === 'DTCS code',
    )
    expect(dtcs?.querySelector('input')?.getAttribute('type')).toBe('number')
  })
})

// ── FrequencyReadout: the site that was already right, now on the shared helper ───────────
describe('FrequencyReadout dial entry (the pre-existing correct site)', () => {
  /** Click the readout to start editing and return its input. */
  const dialInput = (container: HTMLElement) => {
    fireEvent.click(container.querySelector('.readout.editable') as HTMLElement)
    return within(container).getByLabelText('Dial frequency (MHz)') as HTMLInputElement
  }

  it('still commits a comma decimal, and still commits a dot decimal', () => {
    for (const [typed, want] of [
      ['14,074', 14.074],
      ['14.074', 14.074],
    ] as const) {
      const onCommit = vi.fn()
      const { container, unmount } = render(
        <FrequencyReadout dialMhz={7.03} editable onCommit={onCommit} />,
      )
      const input = dialInput(container)
      fireEvent.change(input, { target: { value: typed } })
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(onCommit).toHaveBeenCalledWith(want)
      unmount()
    }
  })

  it('does not commit an unparseable dial entry', () => {
    const onCommit = vi.fn()
    const { container } = render(
      <FrequencyReadout dialMhz={7.03} editable onCommit={onCommit} />,
    )
    const input = dialInput(container)
    fireEvent.change(input, { target: { value: '14.0.74' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onCommit).not.toHaveBeenCalled()
  })
})

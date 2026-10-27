// @vitest-environment jsdom
//
// JS8's waterfall controls, as JS8Call 3.0.3's: the header pill names the decode filter's window,
// the dialog switches it and sets its centre and width (or centres it on the RX offset), and QSY to
// centre asks the station to move the dial, whose refusal is shown as the station words it. A
// scheduled frequency change is announced once, with Cancel. The row menus' "Centre filter at …Hz"
// exists only while the filter is on. Asserted by value on what reaches the station.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { Js8WaterfallControls, js8CentreFilterItem } from './Js8WaterfallControls'
import type { Js8State } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  js8SetFilter: vi.fn(async () => ({})),
  js8SetQsyCenter: vi.fn(async () => ({})),
  js8QsyToCentre: vi.fn(async () => ({})),
  js8HopAnswer: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

type Controls = NonNullable<Js8State['controls']>
const js8 = (over: Partial<Controls> = {}, sending = false): Js8State =>
  ({
    sending,
    controls: {
      filter: { enabled: true, centerHz: 1500, widthHz: 2000 },
      qsyCenterHz: 1500,
      hop: null,
      ...over,
    },
  }) as unknown as Js8State

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

function mount(state: Js8State, props: Partial<Parameters<typeof Js8WaterfallControls>[0]> = {}) {
  const onState = vi.fn()
  const r = render(
    <Js8WaterfallControls
      js8={state}
      rxOffsetHz={1234}
      txOffsetHz={2100}
      active
      onState={onState}
      {...props}
    />,
  )
  return { onState, ...r }
}

describe('the waterfall controls', () => {
  it('no plate where the station sends no controls (the Remote page, an older station)', () => {
    mount({ sending: false } as unknown as Js8State)
    expect(screen.queryByRole('button', { name: /Filter/ })).toBeNull()
    expect(document.body.textContent).toBe('')
  })

  it('the pill names the filter window, or says it is off', () => {
    mount(js8())
    expect(screen.getByRole('button', { name: t('js8.wf.pill.on', { lo: 500, hi: 2500 }) })).toBeTruthy()
    cleanup()
    mount(js8({ filter: { enabled: false, centerHz: 1500, widthHz: 2000 } }))
    expect(screen.getByRole('button', { name: t('js8.wf.pill.off') })).toBeTruthy()
  })

  it('switches the filter, sets its centre, and centres it on the RX offset', async () => {
    const { js8SetFilter } = await import('../api')
    mount(js8())
    fireEvent.click(screen.getByRole('button', { name: t('js8.wf.pill.on', { lo: 500, hi: 2500 }) }))
    fireEvent.click(screen.getByRole('switch', { name: t('js8.wf.filter.label') }))
    expect(js8SetFilter).toHaveBeenLastCalledWith({ enabled: false, centerHz: 1500, widthHz: 2000 })
    const centre = screen.getByLabelText(t('js8.wf.center.label'))
    fireEvent.change(centre, { target: { value: '1800' } })
    fireEvent.keyDown(centre, { key: 'Enter' })
    expect(js8SetFilter).toHaveBeenLastCalledWith({ enabled: true, centerHz: 1800, widthHz: 2000 })
    fireEvent.click(screen.getByRole('button', { name: t('js8.wf.sync.action') }))
    expect(js8SetFilter).toHaveBeenLastCalledWith({ enabled: true, centerHz: 1234, widthHz: 2000 })
  })

  it('QSY to centre asks the station, and its refusal is shown as the station says it', async () => {
    const { js8QsyToCentre } = await import('../api')
    const { pushToast } = await import('../toast')
    mount(js8())
    fireEvent.click(screen.getByRole('button', { name: t('js8.wf.pill.on', { lo: 500, hi: 2500 }) }))
    const why = 'Not moved: QSY to centre waits until nothing is transmitting.'
    vi.mocked(js8QsyToCentre).mockRejectedValueOnce(why)
    fireEvent.click(screen.getByRole('button', { name: t('js8.wf.qsy.action') }))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(why, 'info', 5000))
    expect(js8QsyToCentre).toHaveBeenCalledTimes(1)
  })

  it('QSY to centre is not offered while a frame is on the air', () => {
    mount(js8({}, true))
    fireEvent.click(screen.getByRole('button', { name: t('js8.wf.pill.on', { lo: 500, hi: 2500 }) }))
    expect((screen.getByRole('button', { name: t('js8.wf.qsy.action') }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('a scheduled change is announced once, while JS8 is on screen, with Cancel', async () => {
    const { pushToast } = await import('../toast')
    const { js8HopAnswer } = await import('../api')
    vi.spyOn(Date, 'now').mockReturnValue(1_000_000)
    const hop = { mhz: 7.078, firesAtMs: 1_030_000, description: '', held: false }
    const hidden = mount(js8({ hop }), { active: false })
    expect(pushToast, 'not while JS8 is hidden').not.toHaveBeenCalled()
    hidden.rerender(
      <Js8WaterfallControls js8={js8({ hop })} rxOffsetHz={0} txOffsetHz={0} active onState={hidden.onState} />,
    )
    expect(pushToast).toHaveBeenCalledTimes(1)
    const [text, kind, ttl, opts] = vi.mocked(pushToast).mock.calls[0]
    expect([text, kind, ttl]).toEqual([t('js8.hop.toast', { mhz: '7.078000', secs: 30 }), 'info', 30_000])
    expect(opts?.actionLabel).toBe(t('js8.wf.hop.cancel'))
    hidden.rerender(
      <Js8WaterfallControls js8={js8({ hop })} rxOffsetHz={0} txOffsetHz={0} active onState={hidden.onState} />,
    )
    expect(pushToast, 'once per change').toHaveBeenCalledTimes(1)
    await act(async () => {
      opts?.action?.()
    })
    expect(js8HopAnswer).toHaveBeenCalledWith(false)
    vi.restoreAllMocks()
  })
})

describe('the row menus’ "Centre filter at …Hz"', () => {
  it('exists only while the filter is on, and centres it on the row’s offset', async () => {
    const { js8SetFilter } = await import('../api')
    const onState = vi.fn()
    expect(js8CentreFilterItem({ enabled: false, centerHz: 1500, widthHz: 2000 }, 1210.6, onState)).toBeNull()
    const item = js8CentreFilterItem({ enabled: true, centerHz: 1500, widthHz: 500 }, 1210.6, onState)
    expect(item?.label).toBe(t('js8.wf.centreAt', { hz: 1211 }))
    expect(item?.disabled).toBeUndefined()
    await act(async () => {
      item?.onSelect()
    })
    expect(js8SetFilter).toHaveBeenCalledWith({ enabled: true, centerHz: 1211, widthHz: 500 })
  })
})

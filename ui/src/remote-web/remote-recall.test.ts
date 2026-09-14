// Remote parity batch 1: what a browser asks the station for when it recalls a memory. The same
// plan the desktop recall follows (planRecall), expressed as the closed radio.memoryRecall intent:
// never a Settings write from the browser, never a call or a tier.
import { expect, it } from 'vitest'
import { remoteRecallArgs } from './remote-recall'
import { planRecall, type Memory } from '../features/memories'
import fixture from './__fixtures__/memories.json'

const memory = (id: string) => (fixture.memories as unknown as Memory[]).find(m => m.id === id)!

it('recalls a CW, an SSB net and a digital memory at their exact dial and section', () => {
  expect(remoteRecallArgs(memory('m-cw'))).toEqual({
    view: 'cw', args: { section: 'cw', dialMhz: 14.06, band: '20m', sideband: null, fm: null },
  })
  expect(remoteRecallArgs(memory('m-net'))).toEqual({
    view: 'phone', args: { section: 'phone', dialMhz: 7.2, band: '40m', sideband: 'LSB', fm: null },
  })
  const ft8 = { ...memory('m-cw'), id: 'm-ft8', mode: 'FT8', rxMhz: 14.074 } as Memory
  expect(remoteRecallArgs(ft8)).toEqual({
    view: 'operate', args: { section: 'digital', dialMhz: 14.074, band: '20m', sideband: null, fm: null },
  })
})

it('carries an FM machine’s shift, offset and tone exactly as the desktop recall writes them', () => {
  const plan = planRecall(memory('m-repeat')).settingsPatch!
  expect(remoteRecallArgs(memory('m-repeat'))).toEqual({
    view: 'phone',
    args: { section: 'phone', dialMhz: 146.94, band: '2m', sideband: null,
      fm: { shift: plan.rptrShift, offsetHz: plan.rptrOffsetOverrideHz, toneHz: plan.ctcssToneHz } },
  })
  expect(plan).toMatchObject({ rptrShift: 'minus', rptrOffsetOverrideHz: 600000, ctcssToneHz: 103.5 })
  // An odd split derives its shift from the transmit frequency, as the desktop does.
  const odd = { ...memory('m-repeat'), offsetDir: 'split', offsetMhz: undefined, txMhz: 147.54 } as Memory
  expect(remoteRecallArgs(odd)?.args.fm).toEqual({ shift: 'plus', offsetHz: 600000, toneHz: 103.5 })
})

it('recalls a mode the phone policy cannot command at its dial only, and refuses a dial with no band', () => {
  // P25 is neither FM nor a sideband: the desktop tunes the dial and asks the operator to set the mode.
  expect(remoteRecallArgs(memory('m-split'))).toEqual({
    view: 'phone', args: { section: 'phone', dialMhz: 145.1, band: '2m', sideband: null, fm: null },
  })
  // FM voice starts at 29 MHz: below it no FM machine travels, the band's own sideband applies.
  const low = { ...memory('m-repeat'), rxMhz: 28.5 } as Memory
  expect(remoteRecallArgs(low)?.args).toEqual({ section: 'phone', dialMhz: 28.5, band: '10m', sideband: null, fm: null })
  expect(remoteRecallArgs({ ...memory('m-cw'), rxMhz: 5.0 } as Memory)).toBeNull()
})

import { expect, it } from 'vitest'
import { shareStructure } from './stable-share'

const sample = () => ({ mycall: 'TEST', radio: { dialMhz: 7.074, band: '40m', readings: { cat: { age: 10 } } }, stations: [{ call: 'W1AW', snr: -10 }, { call: 'K1ABC', snr: -3 }] })

it('an equal sample is the previous object itself', () => {
  const previous = sample()
  expect(shareStructure(previous, sample())).toBe(previous)
})

it('a changed value rebuilds only its own branch; every unchanged sibling keeps its identity', () => {
  const previous = sample(), next = sample()
  next.radio.dialMhz = 7.075
  const shared = shareStructure(previous, next)
  expect(shared).toEqual(next)
  // Positive control: the changed path is new at every level, so its readers do re-render.
  expect(shared).not.toBe(previous)
  expect(shared.radio).not.toBe(previous.radio)
  // Unchanged branches are the old objects.
  expect(shared.radio.readings).toBe(previous.radio.readings)
  expect(shared.stations).toBe(previous.stations)
})

it('arrays share unchanged items, and a length or key change is a change', () => {
  const previous = sample(), grown = sample()
  grown.stations.push({ call: 'N0CALL', snr: 1 })
  const shared = shareStructure(previous, grown)
  expect(shared.stations).not.toBe(previous.stations)
  expect(shared.stations[0]).toBe(previous.stations[0])
  expect(shared.stations[2]).toEqual({ call: 'N0CALL', snr: 1 })
  const fewer = sample() as Partial<ReturnType<typeof sample>>
  delete fewer.stations
  expect(shareStructure(previous, fewer)).not.toBe(previous)
  expect(shareStructure(previous, { ...sample(), extra: null })).not.toBe(previous)
})

it('anything that is not plain JSON is returned as it came', () => {
  const bytes = new Uint8Array([1, 2, 3]), previous = { image: new Uint8Array([1, 2, 3]) }
  const next = { image: bytes }
  const shared = shareStructure(previous, next)
  expect(shared.image).toBe(bytes)
  expect(shared).not.toBe(previous)
})

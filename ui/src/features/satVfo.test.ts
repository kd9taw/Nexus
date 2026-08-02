// The uplink-mapping LABELS. The write rule itself — choosing is confirming,
// for the radio in play; a mapping change retires other radios' consents —
// lives backend-side (`Engine::confirm_sat_uplink`, invoked through the
// `confirmSatUplink` api verb) and is pinned there: the consent pair is
// engine-owned live state, so the UI carries only the words. What is pinned
// here is that those words say what the mappings DO — a mislabeled option in
// this list is how an operator transmits on their own downlink.
import { describe, it, expect } from 'vitest'
import { satVfoLabel, satVfoPair } from './satVfo'

describe('the uplink-mapping labels', () => {
  it('an unset mapping is named for what it does, not as a dead switch', () => {
    // "Off" used to mean "Doppler writes nothing to the radio". It now means
    // no UPLINK mapping — the downlink is corrected either way — and the label
    // has to say so or it reads as a station with Doppler disabled.
    expect(satVfoLabel('off')).toBe('Not set — downlink only')
  })

  it('names the VFO pair for a sentence without the select’s example', () => {
    expect(satVfoPair('main-down-sub-up')).toBe('Main = downlink, Sub = uplink')
  })

  it('every label names the leg placement — both legs or the single leg', () => {
    // A label naming only one leg is how an operator ends up transmitting on
    // the downlink.
    expect(satVfoLabel('uplink-only')).toBe('Uplink only (transmit)')
    expect(satVfoLabel('downlink-only')).toBe('Downlink only (receive)')
    expect(satVfoLabel('a-up-b-down')).toBe('VFO A = uplink, VFO B = downlink')
  })
})

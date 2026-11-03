// The toast for a contact another program logged, and the "via" the contact card shows.
import { describe, it, expect } from 'vitest'
import { intakeToast, programLabel, viaOf, VIA_FIELD, FROM_A_NETWORK_DEVICE, FROM_THIS_COMPUTER } from './logIntake'

describe('a contact another program logged', () => {
  it('is toasted with its call and the program it came from', () => {
    expect(intakeToast('K1ABC', 'WSJT-X')).toBe('Logged K1ABC from WSJT-X')
    expect(intakeToast('W1ABC', '192.168.1.40')).toBe('Logged W1ABC from 192.168.1.40')
  })

  it('names its program from the via field, and a contact logged in Nexus names none', () => {
    expect(viaOf([['SFI', '120'], [VIA_FIELD, 'N1MM']])).toBe('N1MM')
    expect(viaOf([['SFI', '120']])).toBeNull()
    expect(viaOf([[VIA_FIELD, '  ']])).toBeNull()
    expect(viaOf(undefined)).toBeNull()
  })
})

describe('a contact from a device read from afar', () => {
  it('names a device on the network or this computer, never an address', () => {
    expect(programLabel(FROM_A_NETWORK_DEVICE)).toBe('a device on your network')
    expect(programLabel(FROM_THIS_COMPUTER)).toBe('this computer')
    expect(intakeToast('W1ABC', FROM_A_NETWORK_DEVICE)).toBe('Logged W1ABC from a device on your network')
    expect(programLabel('WSJT-X')).toBe('WSJT-X')
  })
})

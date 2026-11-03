// The toast for a contact another program logged, and the "via" the contact card shows.
import { describe, it, expect } from 'vitest'
import { intakeToast, viaOf, VIA_FIELD } from './logIntake'

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

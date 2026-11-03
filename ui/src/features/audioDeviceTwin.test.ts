// #425: the one offered device a stale saved name most likely is now, and the whitespace a reader
// cannot see, shown.
import { describe, expect, it } from 'vitest'
import { renumberedTwin, showHiddenSpaces } from './audioDeviceTwin'

describe('renumberedTwin', () => {
  it("offers the one device that differs only by Windows' number or the spacing", () => {
    // The reporter's two cases: the old name against the renumbered device, and the hand edit
    // with one space where the device has two.
    expect(
      renumberedTwin('Speakers (USB AUDIO CODEC)', ['Speakers (Realtek)', 'Speakers (2- USB AUDIO  CODEC)']),
    ).toBe('Speakers (2- USB AUDIO  CODEC)')
    expect(renumberedTwin('Line (2- USB AUDIO CODEC)', ['Line (2- USB AUDIO  CODEC)'])).toBe(
      'Line (2- USB AUDIO  CODEC)',
    )
    expect(renumberedTwin('2- USB Audio CODEC ', ['USB Audio CODEC'])).toBe('USB Audio CODEC')
  })

  it('offers nothing when it cannot be sure', () => {
    // Two candidates: which radio's codec is which is not for the picker to guess.
    expect(
      renumberedTwin('Speakers (USB AUDIO CODEC)', [
        'Speakers (2- USB AUDIO CODEC)',
        'Speakers (3- USB AUDIO CODEC)',
      ]),
    ).toBeNull()
    expect(renumberedTwin('Speakers (USB AUDIO CODEC)', ['Speakers (Realtek)'])).toBeNull()
    // A second codec's " #2" never points at the first codec.
    expect(renumberedTwin('Speakers (USB AUDIO CODEC) #2', ['Speakers (USB AUDIO CODEC)'])).toBeNull()
    // Offered as saved, or nothing saved: nothing to offer.
    expect(renumberedTwin('Speakers (Realtek)', ['Speakers (Realtek)'])).toBeNull()
    expect(renumberedTwin('', ['Speakers (Realtek)'])).toBeNull()
  })
})

describe('showHiddenSpaces', () => {
  it('marks doubled, end and odd whitespace and leaves single spaces alone', () => {
    expect(showHiddenSpaces('Line (2- USB AUDIO  CODEC)')).toBe('Line (2- USB AUDIO␣␣CODEC)')
    expect(showHiddenSpaces('USB Audio CODEC ')).toBe('USB Audio CODEC␣')
    expect(showHiddenSpaces(' USB\tCODEC')).toBe('␣USB␣CODEC')
    expect(showHiddenSpaces('plughw:CARD=GONE,DEV=0')).toBe('plughw:CARD=GONE,DEV=0')
    expect(showHiddenSpaces('Line (2- USB AUDIO CODEC)')).toBe('Line (2- USB AUDIO CODEC)')
  })
})

// A MEMORY REOPENS ON THE MODE SCREEN IT WAS SAVED FROM.
//
// The report: 144.8875 FM saved for SSTV recalled into Phone, because the only thing recall knew
// was the mode word, and FM means Phone. A memory now carries the screen it was saved from, and
// recall opens that screen. What these pin:
//   - a memory with no screen plans exactly as it always did, field for field;
//   - a screen the memory's mode cannot be worked on, or that is switched off, is not honoured:
//     the memory plans as if it had none, so a recall never strands the operator;
//   - the file stays bank version 2 with one more optional field, so an older build reads every
//     row (it ignores the field), and this build drops a screen it does not know and keeps the row;
//   - saving an equivalent channel from a screen moves the existing row to that screen, since the
//     save is the operator's latest word on where it belongs.
import { describe, expect, it } from 'vitest'
import {
  addMemoryDeduped,
  coerceBank,
  coerceMemory,
  emptyBank,
  MEMORY_SCREENS,
  memoryModeFromRig,
  nextModeScreen,
  planRecall,
  saveFavoriteFromDial,
  screenFits,
  type Memory,
  type MemoryKind,
  type MemoryScreen,
} from './memories'

function mem(over: Partial<Memory> & { rxMhz: number; mode: string }): Memory {
  const m = coerceMemory({ id: 'x', ...over })
  if (!m) throw new Error('bad memory')
  return m
}

const REPORTED = { rxMhz: 144.8875, mode: 'FM' }

describe('a memory with no recorded screen plans exactly as it always did', () => {
  const rows: Array<[string, Partial<Memory> & { rxMhz: number; mode: string }, string]> = [
    ['FM repeater', { rxMhz: 146.94, mode: 'FM', offsetDir: 'minus', offsetMhz: 0.6, toneMode: 'tone', ctcssEncHz: 88.5 }, 'phone'],
    ['USB net', { rxMhz: 14.3, mode: 'USB' }, 'phone'],
    ['CW', { rxMhz: 14.03, mode: 'CW' }, 'cw'],
    ['FT8', { rxMhz: 14.074, mode: 'FT8' }, 'operate'],
    ['RTTY', { rxMhz: 14.08, mode: 'RTTY' }, 'operate'],
    ['JS8', { rxMhz: 14.078, mode: 'JS8' }, 'operate'],
    ['kind digital on USB', { rxMhz: 7.1, mode: 'USB', kind: 'digital' as MemoryKind }, 'operate'],
  ]
  it.each(rows)('%s', (_n, over, view) => {
    expect(planRecall(mem(over)).view).toBe(view)
  })

  it('keeps the phone plumbing of an FM memory', () => {
    expect(planRecall(mem({ rxMhz: 146.94, mode: 'FM', offsetDir: 'minus', offsetMhz: 0.6, toneMode: 'tone', ctcssEncHz: 88.5 }))).toEqual({
      view: 'phone',
      freqMhz: 146.94,
      mode: 'FM',
      settingsPatch: { phoneMode: 'fm', rptrShift: 'minus', rptrOffsetOverrideHz: 600000, ctcssToneHz: 88.5 },
    })
  })
})

describe('a memory with a recorded screen opens it', () => {
  it('the reported case: an FM memory saved from SSTV opens SSTV, not Phone', () => {
    expect(planRecall(mem({ ...REPORTED, screen: 'sstv' })).view).toBe('sstv')
  })

  it('an SSTV FM memory carries its own shift and tone, and leaves the station-wide phone mode alone', () => {
    // The screen holds FM on the channel by itself (`sstv_tune`). Writing phone_mode here would
    // silently turn every later 6 m Phone dial into FM.
    const plan = planRecall(
      mem({ rxMhz: 145.5, mode: 'FM', offsetDir: 'minus', offsetMhz: 0.6, toneMode: 'tone', ctcssEncHz: 100, screen: 'sstv' }),
    )
    expect(plan.settingsPatch).toEqual({ rptrShift: 'minus', rptrOffsetOverrideHz: 600000, ctcssToneHz: 100 })
  })

  it('an SSTV sideband memory writes no settings at all', () => {
    expect(planRecall(mem({ rxMhz: 14.23, mode: 'USB', screen: 'sstv' })).settingsPatch).toBeNull()
  })

  it.each([
    ['rtty', 'RTTY'],
    ['psk', 'PSK31'],
    ['jtty', 'JTTY'],
    ['js8', 'JS8'],
    ['operate', 'FT8'],
    // Q65 is not in the digital mode list, and with no screen it plans as Phone. Saved from the FT
    // screen, the FT screen is where it goes.
    ['operate', 'Q65'],
  ] as Array<[MemoryScreen, string]>)('%s with mode %s, writing no settings', (screen, mode) => {
    const plan = planRecall(mem({ rxMhz: 14.08, mode, screen }))
    expect(plan.view).toBe(screen)
    expect(plan.settingsPatch).toBeNull()
  })

  it('APRS on 2 m FM opens APRS and writes no repeater settings (its tune is simplex)', () => {
    const plan = planRecall(mem({ rxMhz: 144.39, mode: 'FM', toneMode: 'tone', ctcssEncHz: 100, screen: 'aprs' }))
    expect(plan.view).toBe('aprs')
    expect(plan.settingsPatch).toBeNull()
  })

  it('Phone stands over a digital kind when that is where it was saved', () => {
    const plan = planRecall(mem({ rxMhz: 7.1, mode: 'USB', kind: 'digital', screen: 'phone' }))
    expect(plan.view).toBe('phone')
    expect(plan.settingsPatch).toEqual({ phoneMode: 'ssb' })
  })
})

describe('a screen that cannot take the memory is not honoured', () => {
  // The mode can be edited after the save, and a stale screen must not win over it.
  it.each([
    ['sstv', 14.03, 'CW', 'cw'],
    ['phone', 14.03, 'CW', 'cw'],
    ['operate', 14.03, 'CW', 'cw'],
    ['sstv', 14.074, 'FT8', 'operate'],
    ['phone', 14.074, 'FT8', 'operate'],
    ['cw', 14.3, 'USB', 'phone'],
    ['rtty', 14.3, 'USB', 'phone'],
    ['operate', 146.52, 'FM', 'phone'],
    ['psk', 7.1, 'LSB', 'phone'],
    ['js8', 7.1, 'AM', 'phone'],
    // APRS's tune is a 2 m FM verb, so anything else falls back.
    ['aprs', 14.3, 'USB', 'phone'],
    ['aprs', 144.2, 'USB', 'phone'],
    ['aprs', 29.6, 'FM', 'phone'],
    ['aprs', 148.1, 'FM', 'phone'],
    ['aprs', 143.9, 'FM', 'phone'],
  ] as Array<[MemoryScreen, number, string, string]>)('%s for %s %s plans as %s', (screen, rxMhz, mode, view) => {
    const m = mem({ rxMhz, mode, screen })
    expect(screenFits(m, screen)).toBe(false)
    expect(planRecall(m)).toEqual(planRecall({ ...m, screen: undefined }))
    expect(planRecall(m).view).toBe(view)
  })

  it('the 2 m edges are APRS, and NFM is FM', () => {
    expect(screenFits(mem({ rxMhz: 144, mode: 'FM' }), 'aprs')).toBe(true)
    expect(screenFits(mem({ rxMhz: 148, mode: 'NFM' }), 'aprs')).toBe(true)
  })

  it('a screen switched off falls back to where the memory would go without one', () => {
    const m = mem({ ...REPORTED, screen: 'sstv' })
    expect(planRecall(m, (s) => s !== 'sstv')).toEqual(planRecall({ ...m, screen: undefined }))
    // A different section being off changes nothing.
    expect(planRecall(m, (s) => s !== 'rtty').view).toBe('sstv')
  })
})

describe('the file loads safely in both directions', () => {
  it('a row from an older build, with no screen, loads with none', () => {
    const bank = coerceBank({ version: 2, memories: [{ id: 'a', rxMhz: 146.52, mode: 'FM' }], groups: [] })
    expect(bank.memories[0]).not.toHaveProperty('screen')
  })

  it('a screen this build does not know is dropped and the row is kept', () => {
    for (const screen of ['hologram', 'settings', 'logbook', 7, null, {}]) {
      const bank = coerceBank({ version: 2, memories: [{ id: 'a', rxMhz: 146.52, mode: 'FM', screen }], groups: [] })
      expect(bank.memories).toHaveLength(1)
      expect(bank.memories[0]).not.toHaveProperty('screen')
    }
  })

  it('every screen survives a save and a load', () => {
    for (const screen of MEMORY_SCREENS) {
      const stored = JSON.parse(JSON.stringify({ ...emptyBank(), memories: [mem({ rxMhz: 14.2, mode: 'USB', screen })] }))
      expect(coerceBank(stored).memories[0].screen).toBe(screen)
    }
  })

  it('stays bank version 2, a plain string in one more optional field', () => {
    const stored = JSON.parse(JSON.stringify({ ...emptyBank(), memories: [mem({ ...REPORTED, screen: 'sstv' })] }))
    expect(stored.version).toBe(2)
    expect(stored.memories[0].screen).toBe('sstv')
    expect(stored.memories[0].rxMhz).toBe(144.8875)
    expect(stored.memories[0].mode).toBe('FM')
  })
})

describe('saving records the screen', () => {
  it('a new row takes the screen it was saved from', () => {
    const { bank, added } = addMemoryDeduped(emptyBank(), { ...REPORTED, screen: 'sstv' })
    expect(added).toBe(true)
    expect(bank.memories[0].screen).toBe('sstv')
  })

  it('an equivalent row saved from a screen moves to it, and adds nothing', () => {
    const first = addMemoryDeduped(emptyBank(), REPORTED).bank
    const { bank, added, moved } = addMemoryDeduped(first, { ...REPORTED, screen: 'sstv' })
    expect(added).toBe(false)
    expect(moved).toBe(true)
    expect(bank.memories).toHaveLength(1)
    expect(bank.memories[0].screen).toBe('sstv')
  })

  it('a save from the same screen is not a move', () => {
    const first = addMemoryDeduped(emptyBank(), { ...REPORTED, screen: 'sstv' }).bank
    const res = addMemoryDeduped(first, { ...REPORTED, screen: 'sstv' })
    expect(res.moved).toBe(false)
    expect(res.bank).toBe(first)
  })

  it('a save from no screen (Program, a pack, an import) never takes the screen away', () => {
    const first = addMemoryDeduped(emptyBank(), { ...REPORTED, screen: 'sstv' }).bank
    const res = addMemoryDeduped(first, REPORTED)
    expect(res.moved).toBe(false)
    expect(res.bank.memories[0].screen).toBe('sstv')
  })

  it('the move leaves a pack row the pack’s, as a star does', () => {
    const first = addMemoryDeduped(emptyBank(), { ...REPORTED, source: 'curated' }).bank
    const { bank } = addMemoryDeduped(first, { ...REPORTED, screen: 'sstv' })
    expect(bank.memories[0].source).toBe('curated')
  })

  it('the strip’s ＋ records it on a new row, on a row it stars, and on a row already starred', () => {
    const added = saveFavoriteFromDial(emptyBank(), { ...REPORTED, screen: 'phone' })
    expect(added.result).toBe('added')
    expect(added.bank.memories[0].screen).toBe('phone')

    const plain = addMemoryDeduped(emptyBank(), REPORTED).bank
    const starred = saveFavoriteFromDial(plain, { ...REPORTED, screen: 'phone' })
    expect(starred.result).toBe('starred')
    expect(starred.bank.memories[0].screen).toBe('phone')
    expect(starred.bank.memories[0].favorite).toBe(true)

    const exists = saveFavoriteFromDial(starred.bank, { ...REPORTED, screen: 'cw' })
    expect(exists.result).toBe('exists')
    expect(exists.bank.memories[0].screen).toBe('cw')
  })
})

describe('which mode screen a save from the Memories section came from', () => {
  it('a mode screen becomes the one', () => {
    for (const s of MEMORY_SCREENS) expect(nextModeScreen(null, s)).toBe(s)
    expect(nextModeScreen('phone', 'sstv')).toBe('sstv')
  })

  it('a glance at something else keeps it', () => {
    for (const v of ['logbook', 'memories', 'settings', 'spots', 'needed', 'connect'] as const) {
      expect(nextModeScreen('sstv', v)).toBe('sstv')
    }
  })

  it('Tempo and Satellites own the dial, so a save after them names no screen', () => {
    expect(nextModeScreen('sstv', 'chat')).toBeNull()
    expect(nextModeScreen('sstv', 'sats')).toBeNull()
  })
})

describe('the mode an SSTV or APRS save is stamped with', () => {
  // Neither screen is a section, so the rig's word is the only one, and with the data submode held
  // that word is not the channel's. A memory stamped PKTFM would plan an SSB tune.
  it.each([
    ['FM', 'FM'],
    ['PKTFM', 'FM'],
    ['DATA-FM', 'FM'],
    ['FM-D', 'FM'],
    ['NFM', 'FM'],
    ['USB', 'USB'],
    ['PKTUSB', 'USB'],
    ['DATA-U', 'USB'],
    ['D-U', 'USB'],
    ['LSB', 'LSB'],
    ['PKTLSB', 'LSB'],
    ['DATA-L', 'LSB'],
    [' usb ', 'USB'],
    ['AM', 'AM'],
  ])('%s is %s', (rig, want) => {
    expect(memoryModeFromRig(rig)).toBe(want)
  })
})

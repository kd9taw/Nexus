// When a decoder screen counts as LEFT (decoderScreens.ts): on entering another section, never on
// a glance. Sequences, because the leave can come several screens after the decoder screen.
import { describe, it, expect } from 'vitest'
import { enterView, type DecoderScreen } from './decoderScreens'

/** Walk `views` from nothing held; every screen left on the way, in order. */
function walk(views: string[]): { left: DecoderScreen[]; held: DecoderScreen[] } {
  let held: DecoderScreen[] = []
  const left: DecoderScreen[] = []
  for (const v of views) {
    const r = enterView(held, v)
    left.push(...r.left)
    held = r.held
  }
  return { left, held }
}

describe('decoder screens left', () => {
  it('the tour: all five, then FT8, leaves every one', () => {
    const r = walk(['operate', 'rtty', 'psk', 'jtty', 'sstv', 'aprs', 'operate'])
    expect(r.left.sort()).toEqual(['aprs', 'jtty', 'psk', 'rtty', 'sstv'])
    expect(r.held).toEqual([])
  })

  it('each decoder screen leaves the one before it', () => {
    expect(walk(['rtty', 'psk']).left).toEqual(['rtty'])
    expect(walk(['psk', 'jtty']).left).toEqual(['psk'])
    expect(walk(['sstv', 'aprs']).left).toEqual(['sstv'])
    expect(walk(['aprs', 'phone']).left).toEqual(['aprs'])
    expect(walk(['jtty', 'cw']).left).toEqual(['jtty'])
    expect(walk(['rtty', 'chat']).left).toEqual(['rtty'])
    expect(walk(['psk', 'js8']).left).toEqual(['psk'])
  })

  it('a glance leaves nothing; the next section does', () => {
    for (const glance of ['connect', 'logbook', 'settings', 'sats', 'stats', 'fieldDay']) {
      const r = walk(['sstv', glance])
      expect(r.left, glance).toEqual([])
      expect(r.held, glance).toEqual(['sstv'])
      expect(walk(['sstv', glance, 'operate']).left, glance).toEqual(['sstv'])
    }
  })

  it('coming back to a screen leaves nothing and keeps it held', () => {
    const r = walk(['rtty', 'logbook', 'rtty'])
    expect(r.left).toEqual([])
    expect(r.held).toEqual(['rtty'])
  })

  it('a decoder screen reached through a glance still leaves the one before', () => {
    expect(walk(['rtty', 'logbook', 'psk']).left).toEqual(['rtty'])
    expect(walk(['rtty', 'logbook', 'psk']).held).toEqual(['psk'])
  })

  it('screens that start no decoder hold nothing', () => {
    expect(walk(['operate', 'phone', 'cw', 'logbook']).held).toEqual([])
    expect(walk(['operate', 'phone', 'cw', 'logbook']).left).toEqual([])
  })

  it('a screen the engine still holds is asked about again at the next section', () => {
    // App puts a kept screen back on the list; the next section leaves it again.
    expect(enterView(['sstv'], 'phone').left).toEqual(['sstv'])
    expect(enterView(['sstv', 'rtty'], 'rtty')).toEqual({ left: ['sstv'], held: ['rtty'] })
  })
})

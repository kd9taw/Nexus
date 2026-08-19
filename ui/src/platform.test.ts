// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'

// IS_MAC / MOD_LABEL are computed at module load, so each case stubs the UA and
// re-imports a fresh copy — the same way the app sees exactly one platform per run.
async function loadWithUA(ua: string) {
  vi.resetModules()
  vi.stubGlobal('navigator', { userAgent: ua })
  return import('./platform')
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('platform chord labels', () => {
  it('mac UA branches to ⌘ (Ctrl there is the OS right-click / Spaces chord)', async () => {
    const p = await loadWithUA(
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
    )
    expect(p.IS_MAC).toBe(true)
    expect(p.MOD_LABEL).toBe('⌘')
    expect(p.modChord(1)).toBe('⌘1')
    expect(p.modChord('9')).toBe('⌘9')
  })

  it('everywhere else stays Ctrl', async () => {
    const p = await loadWithUA(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko)',
    )
    expect(p.IS_MAC).toBe(false)
    expect(p.MOD_LABEL).toBe('Ctrl')
    expect(p.modChord(1)).toBe('Ctrl+1')
  })
})

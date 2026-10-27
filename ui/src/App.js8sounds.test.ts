// JS8Call's sounds from whichever screen is showing: App's WIRING of the player, pinned at the source
// level the App.js8host.test.ts way (App cannot be mounted in jsdom). What must be true of it: it is
// mounted once, beside the JS8 host and not inside it, on the same feature toggle; it is handed
// nothing about which screen shows (no `effectiveView`, no `active`), so no screen decides whether a
// sound plays; and it is handed the engine's mode and the sound settings. Its behaviour is Js8SoundPlayer.test.tsx's.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const app = readFileSync(fileURLToPath(new URL('./App.tsx', import.meta.url)), 'utf8')

describe('the JS8 sound player in App', () => {
  it('is mounted once, beside the JS8 host, and told nothing about the screen shown', () => {
    const at = app.indexOf('<Js8SoundPlayer')
    expect(at, 'App mounts the player').toBeGreaterThan(0)
    expect(app.indexOf('<Js8SoundPlayer', at + 1), 'once').toBe(-1)
    const host = app.indexOf('className="js8-host"')
    expect(host, 'CONTROL: the host is found').toBeGreaterThan(0)
    expect(at, 'after the host block closes, not inside it').toBeGreaterThan(app.indexOf('</div>', host))
    const guard = app.slice(Math.max(0, at - 80), at)
    expect(guard).toMatch(/\{isViewEnabled\('js8'\) && \(\s*$/)
    const props = app.slice(at, app.indexOf('/>', at))
    expect(props).not.toMatch(/effectiveView|active=/)
    expect(props).toContain('tier={snap?.link.tier}')
    expect(props).toContain('sounds={settings?.js8Sounds ?? []}')
    expect(props).toContain('files={settings?.js8SoundFiles ?? {}}')
    // The new-message notice's Show opens the JS8 screen and asks it for that station's Inbox.
    expect(props).toMatch(/onShowMessage=\{\(call\) => \{\s*handleView\('js8'\)\s*setJs8InboxAsk\(/)
    expect(app).toContain('inboxRequest={js8InboxAsk}')
  })
})

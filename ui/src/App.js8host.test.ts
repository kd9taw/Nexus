// The JS8 keep-alive host — WIRING pinned at the source level, the host-hidden.test.ts way,
// because App cannot be mounted in jsdom (it blocks on the snapshot: "Connecting to Nexus…").
// What must be true of the block: it is gated on the feature toggle (a disabled — and JS8 ships
// defaultOff — section mounts nothing), it is hidden by `effectiveView` (never unmounted, so the
// activity stream keeps its scroll while the operator is elsewhere), and it passes the cockpit
// exactly the props the other cockpit hosts pass — `active` (the display poll + the js8_enter
// edge), `onSetTxEnabled` (the header pill is the ONLY TX latch in this view, the TopBar cluster
// being hidden by hideDigitalChrome), `onSetFrequency` (the JS8 band picker), and its OWN panel
// layout (JS8_PANELS) — never another cockpit's, which would share ⊞ state across modes.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const app = readFileSync(fileURLToPath(new URL('./App.tsx', import.meta.url)), 'utf8')

/** The `.js8-host` JSX block: from its class attribute to the host's own closing `</div>`
 *  (the cockpit inside is self-closing, so the first `</div>` after the class is the host's). */
function js8HostBlock(): { start: number; text: string } {
  const start = app.indexOf('className="js8-host"')
  expect(start, 'App.tsx has no .js8-host').toBeGreaterThan(0)
  return { start, text: app.slice(start, app.indexOf('</div>', start)) }
}

describe('the JS8 keep-alive host', () => {
  it('is gated on the feature toggle and hidden by effectiveView, like every other host', () => {
    const { start, text } = js8HostBlock()
    const before = app.slice(Math.max(0, start - 80), start)
    expect(before).toMatch(/isViewEnabled\('js8'\)\s*&&\s*\(\s*<div\s*$/)
    expect(text).toContain(`hidden={effectiveView !== 'js8'}`)
    expect(text).toContain('<Js8Cockpit')
  })

  it('passes the cockpit its own panel layout and the App handlers', () => {
    const { text } = js8HostBlock()
    for (const prop of [
      "active={effectiveView === 'js8'}",
      'panels={js8Panels}',
      'onSetTxEnabled={handleSetTxEnabled}',
      'onSetFrequency={handleSetFrequency}',
      'onOpenLogbook={openLogbookFor}',
      'onSnap={setSnap}',
      'snap={snap}',
      'theme={theme}',
      'wheelSensitivity={settings?.wheelTuneSensitivity ?? 1}',
    ]) {
      expect(text, `the .js8-host block is missing ${prop}`).toContain(prop)
    }
    expect(app).toContain('const js8Panels = usePanelLayout(JS8_PANELS)')
  })

  it('control: the same parse sees the PSK host the same way (the pattern is real)', () => {
    const psk = app.indexOf('className="psk-host"')
    expect(psk).toBeGreaterThan(0)
    expect(app.slice(psk, app.indexOf('</div>', psk))).toContain('panels={pskPanels}')
  })
})

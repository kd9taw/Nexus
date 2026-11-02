// @vitest-environment jsdom
//
// THE COMPACT CHROME (the 1080p pass, 2026-11-01), RENDERED: the real cockpits, the real top bar and
// the header's ⋯ at the size classes that fold them (features/compactChrome), against the one rule the
// pass may never bend — EVERY TRANSMIT AND STOP CONTROL STAYS WHERE IT IS: PTT, Lock, Send, Stop TX,
// Tune, the TX latch, the Esc/Stop macro and every macro key in the dock or the TX strip, at every size,
// with no id in any pane vocabulary and never behind a fold. Only read-only meters, a status line, the
// tier pills, the now-bar and occasional controls (memories, rotator, split, REC) move.
//
// The cascade half (what the folds' classes resolve to, the short-window tier's values at both sides of
// its step) is computed in cockpit-panes.test.ts, cockpit-shells.test.ts and responsive-vocab.test.ts;
// geometry (one row at 1920×1040 and 1707×920) is the browser probe's. This file holds the DOM: where
// each control renders, and that the keyboard reaches everything a fold put behind a button.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { TopBar } from './TopBar'
import { HeaderOverflow } from './HeaderOverflow'
import { t } from '../i18n'
import type { RadioStatus, Tier } from '../types'
import { cw, jtty, panelsWith, phone, psk, rtty, settle, stopsOnScreen, type Case } from './stop-line.testkit'

// The api answers the stop-line sweeps share (stop-line.api.testkit.ts says why).
// …plus the version the top bar prints beside its name (the shared answers have none for it).
vi.mock('../api', async (importOriginal) => ({
  ...(await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
  appVersion: async () => '1.0.0',
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children only; the header, the TX strip and the dock are real.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./panes/BoxBody', () => ({ BoxBody: () => <div data-testid="box-body-stub" /> }))

vi.setConfig({ testTimeout: 20_000 })

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete document.documentElement.dataset.viewport
})

const at = (vp: 'sm' | 'md' | 'lg' | 'xl') => {
  document.documentElement.dataset.viewport = vp
}
const SIZES = ['sm', 'md', 'lg', 'xl'] as const
const WIDE = new Set(['md', 'lg', 'xl'])
const FOLDS = new Set(['md', 'lg'])

/** Where a transmit control may stand: the dock or the TX strip, and nowhere a fold could reach. */
function assertTxHome(el: Element, what: string) {
  expect(el.closest('.cockpit-txdock, .cockpit-txstrip'), `${what} is in the dock or the TX strip`).not.toBeNull()
  expect(el.closest('.ch-overflow-panel'), `${what} is never behind the header's ⋯`).toBeNull()
  expect(el.closest('.cockpit-panes, .pane-frame, .cockpit-header'), `${what} is in no pane and not in the header`).toBeNull()
}

async function mount(c: Case<any>) {
  c.render(panelsWith<string>([]))
  await settle()
}

describe('the compact dock: every transmit and stop control stays in the dock or the TX strip, at every size', () => {
  for (const c of [phone, cw, rtty, psk, jtty]) {
    it.each(SIZES)(`${c.cockpit} at %s: the stop controls, and the senders, are where they were`, async (vp) => {
      at(vp)
      await mount(c)
      const stops = stopsOnScreen(c.stopControls)
      for (const [label] of c.stopControls) {
        const found = stops.get(label)!
        expect(found.length, `${c.cockpit} ${vp}: ${label} on screen`).toBeGreaterThan(0)
        for (const b of found) assertTxHome(b, `${c.cockpit} ${vp}: ${label}`)
      }
      const dock = document.querySelector('.cockpit-txdock')!
      expect(dock, `${c.cockpit}: a dock`).not.toBeNull()
      if (c === phone) {
        assertTxHome(screen.getByRole('checkbox', { name: 'Lock' }), 'Lock')
      } else if (c === cw) {
        const keys = within(screen.getByRole('group', { name: t('cw.macros.aria') })).getAllByRole('button')
        expect(keys.length, 'CW: eight F-keys').toBeGreaterThanOrEqual(8)
        for (const k of keys) assertTxHome(k, `CW ${vp}: ${k.textContent}`)
        assertTxHome(screen.getByRole('button', { name: /^send$/i }), 'CW: Send')
      } else {
        const group = screen.getByRole('group', { name: t(`${c.view}.macros.aria` as never) })
        assertTxHome(group, `${c.cockpit} ${vp}: the macro keys`)
        expect(group.classList.contains('dock-fkeys'), `${c.cockpit}: the keys stand on the 70em floor`).toBe(true)
      }
      // Nothing the ⋯ holds is a transmit or stop control (the list above found every one of them
      // outside it); and only Phone and CW fold their header at all.
      const overflow = document.querySelector('.ch-overflow-panel')
      expect(overflow != null, `${c.cockpit} ${vp}: header ⋯`).toBe((c === phone || c === cw) && FOLDS.has(vp))
    })
  }

  it.each(SIZES)('Phone and CW at %s: the read-only meters stand in the strip from md up, in the dock below', async (vp) => {
    for (const c of [phone, cw]) {
      at(vp)
      await mount(c)
      const inStrip = document.querySelector('.cockpit-txstrip .cockpit-txstrip-meters .ph-txmeters')
      const inDock = document.querySelector('.cockpit-txdock .ph-txmeters')
      expect(inStrip != null, `${c.cockpit} ${vp}: meters in the strip`).toBe(WIDE.has(vp))
      expect(inDock != null, `${c.cockpit} ${vp}: meters in the dock`).toBe(!WIDE.has(vp))
      if (inStrip) expect(inStrip.classList.contains('strip'), 'the two-row strip variant').toBe(true)
      cleanup()
    }
  })

  it('in the strip the meters keep their four slots before the first over (Phone sets COMP, PROC and TX bandwidth there)', async () => {
    at('lg')
    await mount(phone)
    const meters = document.querySelector('.cockpit-txstrip .ph-txmeters')!
    expect([...meters.querySelectorAll('.ph-txmeter-label')].map((l) => l.textContent)).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
    expect(meters.querySelector('.ph-txmeters-hint'), 'no hint line: it would be a third row').toBeNull()
  })

  it.each(SIZES)('Phone at %s: the transmit contract joins the PTT row from md up, after PTT and Lock', async (vp) => {
    at(vp)
    await mount(phone)
    const contract = document.querySelector('.ph-txcontract')!
    expect(contract, 'the contract is drawn at every size').not.toBeNull()
    const row = document.querySelector('.cockpit-txdock .ph-ptt-row')!
    if (WIDE.has(vp)) {
      expect(contract.parentElement).toBe(row)
      expect(contract.classList.contains('dock-status-inrow')).toBe(true)
      const kids = [...row.children]
      const ptt = row.querySelector('.ph-ptt')!
      const lock = row.querySelector('.ph-lock')!
      expect(kids.indexOf(ptt)).toBeLessThan(kids.indexOf(lock))
      expect(kids.indexOf(lock), 'nothing the contract says can move PTT or Lock').toBeLessThan(kids.indexOf(contract))
    } else {
      expect(contract.parentElement?.classList.contains('cockpit-txdock'), 'its own line, as before').toBe(true)
      expect(contract.classList.contains('dock-status-inrow')).toBe(false)
      const kids = [...contract.parentElement!.children]
      expect(kids.indexOf(contract), 'above the PTT row').toBeLessThan(kids.indexOf(row))
    }
  })
})

describe('the CW and Phone headers fold their occasional controls behind ⋯ at md and lg', () => {
  const REC = /^record qso audio$/i
  for (const c of [cw, phone]) {
    it.each(SIZES)(`${c.cockpit} at %s`, async (vp) => {
      at(vp)
      await mount(c)
      const more = screen.queryByRole('button', { name: t('cockpit.header.more.label') })
      if (!FOLDS.has(vp)) {
        expect(more, `${vp}: no ⋯`).toBeNull()
        const rec = screen.getByRole('button', { name: REC })
        expect(rec.closest('.cockpit-header'), 'REC inline in the header').not.toBeNull()
        return
      }
      expect(more).not.toBeNull()
      expect(more!.closest('.cockpit-header'), 'the ⋯ is in the header').not.toBeNull()
      expect(more!.getAttribute('aria-expanded')).toBe('false')
      const group = document.getElementById(more!.getAttribute('aria-controls')!)!
      expect(group.hidden, 'closed: the group is not drawn and not in the tab order').toBe(true)
      expect(screen.queryByRole('button', { name: REC }), 'REC is behind the ⋯ while closed').toBeNull()
      // Every control the fold took is in the group (mounted while closed, so it keeps its state).
      expect(within(group).getByRole('button', { name: REC, hidden: true })).toBeTruthy()
      // Split too (the fixture's CAT is up, so CW draws it as well as Phone).
      expect(group.querySelector('.ph-split'), 'split is in the group').not.toBeNull()
      expect(document.querySelectorAll('.cockpit-header .ph-split'), 'and nowhere else in the header').toHaveLength(1)
      fireEvent.click(more!)
      expect(more!.getAttribute('aria-expanded')).toBe('true')
      expect(group.hidden).toBe(false)
      expect(group.classList.contains('ch-overflow-open')).toBe(true)
      expect(screen.getByRole('button', { name: REC }), 'open: REC is one click away').toBeTruthy()
    })
  }
})

describe('HeaderOverflow: the keyboard reaches everything it holds, and Esc still reaches the window', () => {
  function host(badges?: ReactNode) {
    return render(
      <div>
        <button type="button">before</button>
        <HeaderOverflow label="More controls" title="More" badges={badges}>
          <button type="button">First</button>
          <input aria-label="Second" />
          <button type="button" disabled>
            Off
          </button>
        </HeaderOverflow>
        <button type="button">after</button>
      </div>,
    )
  }
  const trigger = () => screen.getByRole('button', { name: 'More controls' })

  it('Enter opens it and puts the keyboard on the first control; Escape closes it and returns to the button', async () => {
    host()
    trigger().focus()
    fireEvent.keyDown(trigger(), { key: 'Enter' })
    fireEvent.click(trigger())
    await waitFor(() => expect(document.activeElement?.textContent).toBe('First'))
    expect(trigger().getAttribute('aria-expanded')).toBe('true')
    const seen: string[] = []
    const onWindow = (e: KeyboardEvent) => seen.push(e.key)
    window.addEventListener('keydown', onWindow)
    try {
      fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    } finally {
      window.removeEventListener('keydown', onWindow)
    }
    expect(trigger().getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(trigger())
    expect(seen, 'Esc is not swallowed: CW’s window handler stops a transmission on it').toEqual(['Escape'])
  })

  it('ArrowDown opens it too; a pointer click opens it without moving the keyboard', async () => {
    host()
    fireEvent.keyDown(trigger(), { key: 'ArrowDown' })
    await waitFor(() => expect(document.activeElement?.textContent).toBe('First'))
    fireEvent.click(trigger())
    expect(trigger().getAttribute('aria-expanded')).toBe('false')
    trigger().focus()
    fireEvent.click(trigger())
    expect(trigger().getAttribute('aria-expanded')).toBe('true')
    expect(document.activeElement).toBe(trigger())
  })

  it('every control inside is reachable by role once open, and none while closed', () => {
    host()
    expect(screen.queryByRole('button', { name: 'First' })).toBeNull()
    expect(screen.queryByRole('textbox', { name: 'Second' })).toBeNull()
    fireEvent.click(trigger())
    expect(screen.getByRole('button', { name: 'First' })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: 'Second' })).toBeTruthy()
    expect(screen.getByRole('group', { name: 'More controls' })).toBeTruthy()
  })

  it('a press outside closes it; a press in a popup one of its controls opened does not', () => {
    host()
    fireEvent.click(trigger())
    const popup = document.createElement('div')
    popup.setAttribute('role', 'menu')
    document.body.appendChild(popup)
    try {
      fireEvent.pointerDown(popup)
      expect(trigger().getAttribute('aria-expanded'), 'a portaled popup is inside, for this purpose').toBe('true')
      fireEvent.pointerDown(screen.getByRole('button', { name: 'after' }))
      expect(trigger().getAttribute('aria-expanded')).toBe('false')
    } finally {
      popup.remove()
    }
  })

  it('shows what is on behind it on the button', () => {
    host(<span className="ch-overflow-badge">SPLIT</span>)
    expect(trigger().textContent).toContain('SPLIT')
    expect(trigger().getAttribute('aria-label'), 'its name stays the label').toBe('More controls')
  })
})

describe('the top bar at md and lg: one menu button for the eleven tiers, the TX cluster untouched', () => {
  const radio = {
    dialMhz: 14.074,
    band: '20m',
    sideband: 'USB',
    rigMode: 'USB',
    rigConfirmed: true,
    nextSlotMs: 0,
    txEven: true,
    txCycleAuto: true,
    txEnabled: false,
    txAllowed: true,
    transmitting: false,
    tuning: false,
    qsoRecording: false,
    catOk: true,
    dtSec: 0,
    operatingMode: 'digital',
  } as unknown as RadioStatus
  const noop = () => {}
  const TIERS = ['Fast', 'Deep', 'FT4', 'FT8', 'FT2', 'WSPR', 'Q65', 'MSK144', 'JT65', 'FST4', 'FST4W']

  function bar(screenName: 'fieldDay' | 'operate', over: Record<string, unknown> = {}) {
    const ft = screenName === 'operate'
    return render(
      <div className="app">
        <TopBar
          mycall="N0CALL"
          mygrid="EN52xa"
          radio={radio}
          link={{ tier: 'FT8', dtSec: 0 } as never}
          bandPlan={[]}
          onSetFrequency={noop}
          onSetTxEnabled={noop}
          onSetTune={noop}
          onHaltTx={noop}
          onSetTxEven={noop}
          onSetTxCycleAuto={noop}
          onSetHoldTxFreq={noop}
          tier="FT8"
          onTierChange={noop}
          onOpenGuide={noop}
          field={false}
          onFieldChange={noop}
          fdActive={screenName === 'fieldDay'}
          hideTxControls={ft}
          hideFrequencyControl={ft}
          hideDigitalChrome={false}
          ftScreen={ft}
          {...over}
        />
      </div>,
    )
  }
  const pills = (c: HTMLElement) => [...c.querySelectorAll('.tier-toggle:not(.tx-period) > .tier-btn[aria-pressed]')]

  it.each(['fieldDay', 'operate'] as const)('%s: at md and lg the pills are one button named by the mode in use; at xl the pills', (name) => {
    for (const vp of ['md', 'lg', 'xl'] as const) {
      at(vp)
      const { container } = bar(name)
      const folded = container.querySelector('.tier-menu-folded')
      if (FOLDS.has(vp)) {
        expect(pills(container), `${name} ${vp}: no pills`).toEqual([])
        expect(folded, `${name} ${vp}: the folded button`).not.toBeNull()
        expect(folded!.textContent).toBe('FT8')
        expect(folded!.getAttribute('aria-haspopup')).toBe('menu')
      } else {
        expect(pills(container).map((p) => p.textContent?.replace(/^(Tempo|BCN|RX)/, '')), `${name} ${vp}`).toEqual(TIERS)
        expect(folded, `${name} ${vp}: no folded button`).toBeNull()
      }
      // The Tx cycle stays three buttons at every size, and the bar's TX cluster stays on Field Day.
      expect(container.querySelectorAll('.tx-period > .tier-btn')).toHaveLength(3)
      if (name === 'fieldDay') {
        const cluster = container.querySelector('.op-controls')!
        expect([...cluster.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['TX Off', 'Tune', 'Stop TX', 'Hold Tx'])
      }
      cleanup()
    }
  })

  it('the keyboard opens it, reaches all eleven in the pills’ order, picks one and comes back to the button', async () => {
    at('lg')
    const picked: Tier[] = []
    const { container } = bar('fieldDay', { onTierChange: (tier: Tier) => picked.push(tier) })
    const button = container.querySelector('.tier-menu-folded') as HTMLButtonElement
    button.focus()
    fireEvent.keyDown(button, { key: 'Enter' })
    const menu = await screen.findByRole('menu')
    const items = within(menu).getAllByRole('menuitem')
    expect(items.map((i) => i.textContent?.replace(/^(Tempo|BCN|RX)/, ''))).toEqual(TIERS)
    for (const i of items) expect(i.getAttribute('title'), `${i.textContent}: keeps its pill's tooltip`).toBeTruthy()
    await waitFor(() => expect(document.activeElement).toBe(items[0]))
    fireEvent.keyDown(document.activeElement!, { key: 'End' })
    await waitFor(() => expect(document.activeElement).toBe(items[10]))
    fireEvent.keyDown(document.activeElement!, { key: 'Enter' })
    await waitFor(() => expect(picked).toEqual(['FST4W']))
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    expect(document.activeElement).toBe(button)
  })

  it('Escape closes it without picking anything', async () => {
    at('md')
    const picked: Tier[] = []
    const { container } = bar('operate', { onTierChange: (tier: Tier) => picked.push(tier) })
    const button = container.querySelector('.tier-menu-folded') as HTMLButtonElement
    fireEvent.keyDown(button, { key: 'ArrowDown' })
    await screen.findByRole('menu')
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    expect(picked).toEqual([])
    expect(document.activeElement).toBe(button)
  })

  it('the now-bar rides as the bar’s last group, on the operating screens and the others alike', () => {
    for (const name of ['operate', 'fieldDay'] as const) {
      at('lg')
      const { container } = bar(name, { nowBar: <div className="now-bar now-bar-inbar" /> })
      const header = container.querySelector('header.topbar')!
      expect(header.lastElementChild!.classList.contains('topbar-now'), name).toBe(true)
      expect(header.querySelectorAll('.topbar-now'), name).toHaveLength(1)
      cleanup()
    }
    at('lg')
    const none = bar('fieldDay')
    expect(none.container.querySelector('.topbar-now'), 'no slot without a now-bar').toBeNull()
  })
})

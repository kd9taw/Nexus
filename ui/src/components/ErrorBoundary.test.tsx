// @vitest-environment jsdom
// Pins the structural half of the 0.24.6 black-screen fix.
//
// The field facts this test encodes: a render throw in one Connect pane took down the
// ENTIRE main-window React root — the nav rail with it — leaving a black window that
// came back every launch, while the Needed POP-OUT (its own root) kept driving CAT
// fine. So: (1) a crash must not reach the siblings, (2) the operator must be able to
// navigate out of it, and (3) it must be caught on the FIRST mount too, because the
// app restores its last view at launch and lands straight back in the crash.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { useState } from 'react'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { ErrorBoundary } from './ErrorBoundary'

afterEach(cleanup)

/** React logs every caught error itself; silence the noise and let us assert ours. */
let errorSpy: ReturnType<typeof vi.spyOn>
/** React DEV re-throws a caught error through a synthetic DOM event so browser devtools
 * still see it; jsdom then dumps the whole stack to stderr on every one of these tests.
 * Swallow it at the window — the boundary's own console.error is what we assert. */
const muteJsdom = (e: ErrorEvent) => e.preventDefault()
beforeEach(() => {
  errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  window.addEventListener('error', muteJsdom)
})
afterEach(() => {
  window.removeEventListener('error', muteJsdom)
  errorSpy.mockRestore()
})

const BOOM = 'Rendered more hooks than during the previous render.'
function Boom(): JSX.Element {
  throw new Error(BOOM)
}

/** The shell shape: rail + the boundary-wrapped view, as App mounts it. */
function Shell({ view }: { view: string }) {
  return (
    <div className="shell">
      <nav>RAIL</nav>
      <ErrorBoundary label={view} resetKey={view}>
        {view === 'connect' ? <Boom /> : <p>{view} view</p>}
      </ErrorBoundary>
    </div>
  )
}

describe('ErrorBoundary', () => {
  it('catches a first-mount throw and leaves the siblings alive', () => {
    // The restart-loop case: the persisted view is restored INTO the crash, so the
    // throw happens on the very first render — no click involved.
    const { container } = render(<Shell view="connect" />)
    expect(screen.getByText('RAIL')).toBeTruthy() // the rail survived → not a black screen
    expect(container.innerHTML).not.toBe('')
    expect(screen.getByRole('alert')).toBeTruthy()
    expect(screen.getByText(/connect hit an error/i)).toBeTruthy()
  })

  it('shows the real error text, selectable, for a bug report', () => {
    render(<Shell view="connect" />)
    const detail = screen.getByText(new RegExp(BOOM.replace(/[.]/g, '\\.')))
    expect(detail.className).toContain('view-crash-detail')
  })

  it('never swallows silently — console.error carries the crash', () => {
    render(<Shell view="connect" />)
    const calls = errorSpy.mock.calls as unknown[][]
    const ours = calls.filter((c) => String(c[0]).includes('[nexus] connect crashed'))
    expect(ours.length).toBeGreaterThan(0)
    expect(String(ours[0][1])).toContain(BOOM)
  })

  it('recovers when the operator navigates away (resetKey change)', () => {
    const { rerender } = render(<Shell view="connect" />)
    expect(screen.getByRole('alert')).toBeTruthy()
    rerender(<Shell view="operate" />)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByText('operate view')).toBeTruthy()
  })

  it('the escape action fires its handler and retries the children', () => {
    // Also covers the awkward case the action lands on the SAME view (no resetKey
    // change): the button clears the error itself, so it always does something.
    function Flaky() {
      const [tries, setTries] = useState(0)
      return (
        <ErrorBoundary
          label="Connect"
          resetKey="connect"
          action={{ label: 'Back to Operate', onClick: () => setTries((n) => n + 1) }}
        >
          {tries === 0 ? <Boom /> : <p>healed</p>}
        </ErrorBoundary>
      )
    }
    render(<Flaky />)
    fireEvent.click(screen.getByRole('button', { name: 'Back to Operate' }))
    expect(screen.getByText('healed')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('adds no wrapper element while healthy (the shell flex layout is untouched)', () => {
    // `.shell` is a flex row and `.operate-host` is display:contents — an extra <div>
    // around the children would silently change both.
    const { container } = render(
      <ErrorBoundary label="Operate">
        <main className="layout">ok</main>
      </ErrorBoundary>,
    )
    expect(container.firstElementChild?.className).toBe('layout')
  })
})

// ── Wiring ──────────────────────────────────────────────────────────────────────
// A boundary that exists but is mounted nowhere is what this tree shipped with before
// this change, and no behavior test can see that. These read the real sources and
// compute CONTAINMENT (which tags fall between the boundary's open and close), not
// mere presence — a `grep ErrorBoundary` passes on dead code.
const src = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')

/** Source between `<ErrorBoundary` and its matching `</ErrorBoundary>`, nth occurrence. */
function boundaryBody(source: string, nth = 0): string {
  let from = -1
  for (let i = 0; i <= nth; i++) from = source.indexOf('<ErrorBoundary', from + 1)
  expect(from, `no ErrorBoundary #${nth + 1} mounted`).toBeGreaterThan(-1)
  const end = source.indexOf('</ErrorBoundary>', from)
  expect(end, 'unclosed ErrorBoundary').toBeGreaterThan(from)
  return source.slice(from, end)
}

describe('the boundary is actually mounted', () => {
  it('App wraps the workspace and the keep-alive hosts, but not the nav rail', () => {
    const app = src('../App.tsx')
    const body = boundaryBody(app)
    expect(body).toContain('{workspace}')
    expect(body).toContain('className="operate-host"')
    // The rail must stay OUTSIDE — it is the operator's only way out of a crash.
    expect(body).not.toContain('<ModeNav')
    // Inside `.shell`: everything between the shell's opening tag and the boundary
    // is the rail, nothing else.
    const shell = app.indexOf('<div className="shell">')
    expect(shell).toBeGreaterThan(-1)
    expect(app.indexOf('<ErrorBoundary')).toBeGreaterThan(shell)
  })

  it('main.tsx wraps BOTH React roots (the pop-out gets nothing from App\'s boundary)', () => {
    const main = src('../main.tsx')
    const bodies = [boundaryBody(main, 0), boundaryBody(main, 1)].join('\n')
    expect(bodies).toContain('<DetachedPanel')
    expect(bodies).toContain('<App />')
  })
})

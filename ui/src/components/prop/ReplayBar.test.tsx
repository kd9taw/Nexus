// @vitest-environment jsdom
//
// The replay bar must never read as now: REPLAY, the replayed UTC time and how long ago, the GOES
// reading it drew from with that reading's own time (or that there is none), the footprint always
// "estimated from GOES", and the line that says everything else is live. The button that starts it
// is disabled, saying why, without a held trace.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { ReplayBar, ReplayButton } from './ReplayBar'
import { t } from '../../i18n'
import { wxStampLabel } from '../../propViz'

afterEach(cleanup)

const NOW = Date.UTC(2026, 9, 31, 18, 0)
const AT = NOW - (2 * 3600 + 10 * 60) * 1000
const WIN = { loMs: NOW - 6 * 3600_000, hiMs: NOW - 300_000 }
const series: [number, number][] = [
  [WIN.loMs / 1000, 1e-6],
  [AT / 1000, 5e-5],
  [WIN.hiMs / 1000, 1e-6],
]

function bar(over: Partial<Parameters<typeof ReplayBar>[0]> = {}) {
  return render(
    <ReplayBar
      atMs={AT}
      win={WIN}
      nowMs={NOW}
      series={series}
      flux={{ flux: 5e-5, at: AT / 1000 }}
      footDrawn={true}
      onScrub={() => {}}
      onBack={() => {}}
      {...over}
    />,
  )
}

describe('the replay bar', () => {
  it('reads REPLAY with the replayed time and how long ago', () => {
    const r = bar()
    const head = r.container.querySelector('.replay-chip-head')!.textContent
    expect(head).toBe(t('map.replay.head', { time: wxStampLabel(AT / 1000), ago: '2h ago' }))
    expect(head).toContain('REPLAY')
    expect(r.container.querySelector('[role="status"]')).not.toBeNull()
    expect(r.container.textContent).toContain(t('map.replay.live'))
  })

  it('names the reading it drew from, with its own time, and calls the footprint an estimate', () => {
    const r = bar()
    expect(r.container.textContent).toContain(t('map.replay.flux', { cls: 'M5.0', time: '15:50Z' }))
    expect(r.container.querySelector('[data-source]')!.getAttribute('data-source')).toBe('goes')
    expect(r.container.textContent).toContain(t('map.replay.est'))
    expect(r.container.textContent).not.toContain('D-RAP')
  })

  it('says there is no reading, and claims no footprint, when none is held for that time', () => {
    const r = bar({ flux: null, footDrawn: false })
    expect(r.container.textContent).toContain(t('map.replay.noFlux'))
    expect(r.container.textContent).not.toContain(t('map.replay.est'))
  })

  it('bounds the slider to the window and marks the replayed time on the trace', () => {
    const r = bar()
    const s = r.container.querySelector('.replay-slider') as HTMLInputElement
    expect([Number(s.min), Number(s.max), Number(s.step)]).toEqual([WIN.loMs, WIN.hiMs, 300_000])
    expect(s.getAttribute('aria-valuetext')).toBe(wxStampLabel(AT / 1000))
    expect(r.container.querySelector('.flare-spark-cursor')).not.toBeNull()
  })

  it('puts the way back to now right under the head', () => {
    const r = bar()
    const kids = [...r.container.querySelector('.replay-chip')!.children]
    expect(kids[0].className).toBe('replay-chip-head')
    expect(kids[1].classList.contains('replay-back')).toBe(true)
  })

  it('breaks the trace where a bucket is missing', () => {
    const lo = WIN.loMs / 1000
    const held: [number, number][] = [
      [lo, 1e-6],
      [lo + 300, 1e-6],
      [lo + 900, 1e-6], // the bucket at lo + 600 is missing
      [lo + 1200, 1e-6],
    ]
    const d = bar({ series: held }).container.querySelector('.flare-spark-line')!.getAttribute('d')!
    expect(d.match(/M/g)!.length, d).toBe(2)
    cleanup()
    const whole: [number, number][] = [...held.slice(0, 2), [lo + 600, 1e-6], ...held.slice(2)]
    const d2 = bar({ series: whole }).container.querySelector('.flare-spark-line')!.getAttribute('d')!
    expect(d2.match(/M/g)!.length, 'CONTROL: an unbroken trace is one line').toBe(1)
  })

  it('goes back to now', () => {
    const back = vi.fn()
    const r = bar({ onBack: back })
    fireEvent.click(r.container.querySelector('.replay-back')!)
    expect(back).toHaveBeenCalledTimes(1)
  })
})

describe('the replay button', () => {
  it('is disabled, saying why, without a held trace', () => {
    const r = render(<ReplayButton on={false} win={null} startMs={null} onStart={() => {}} onStop={() => {}} />)
    const b = r.container.querySelector('button')!
    expect(b.disabled).toBe(true)
    expect(b.title).toBe(t('map.replay.start.none'))
  })

  it('starts at the given time, and ends a running replay', () => {
    const start = vi.fn()
    const stop = vi.fn()
    const r = render(<ReplayButton on={false} win={WIN} startMs={AT} onStart={start} onStop={stop} />)
    fireEvent.click(r.container.querySelector('button')!)
    expect(start).toHaveBeenCalledWith(AT)
    cleanup()
    const r2 = render(<ReplayButton on={true} win={WIN} startMs={AT} onStart={start} onStop={stop} />)
    expect(r2.container.querySelector('button')!.textContent).toBe(t('map.replay.stop'))
    fireEvent.click(r2.container.querySelector('button')!)
    expect(stop).toHaveBeenCalledTimes(1)
  })
})

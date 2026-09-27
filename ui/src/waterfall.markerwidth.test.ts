// THE FT WATERFALL'S RX/TX MARKERS SHOW THE SIGNAL'S WIDTH, AS WSJT-X'S WIDE GRAPH DOES (2026-09-27).
//
// Operator: "in wsjtx, when working in the waterfall, it has a width of the signal instead of a
// single line like today. I would like to implement the width lines". WSJT-X's Wide Graph brackets
// each marker from its frequency (the signal's LOWEST tone) up to its top tone. The widths below are
// WSJT-X's own arithmetic, read in `CPlotter::DrawOverlay`, `widgets/plotter.cpp` at v3.0.2 (sha256
// 4ce2aaa9…, identical on master; 3.2.0-rc1 and 2.7.0 draw the same widths). The line each figure
// comes from is named beside it, so a row can be checked against the source, not against this file.
import { describe, it, expect } from 'vitest'
import { markerWidthHz } from './waterfall'

describe('markerWidthHz: the width WSJT-X draws above each marker', () => {
  it('FT8 is 7 tone spacings of 12000/1920 Hz: 43.75 Hz, lowest tone to highest (plotter.cpp:512)', () => {
    expect(markerWidthHz('FT8', { periodS: 15 })).toBeCloseTo((7 * 12000) / 1920, 9)
    expect(markerWidthHz('FT8', { periodS: 15 })).toBeCloseTo(43.75, 9)
  })

  it('FT4 is 3 tone spacings of 12000/576 Hz: 62.5 Hz (plotter.cpp:511)', () => {
    expect(markerWidthHz('FT4', { periodS: 7.5 })).toBeCloseTo((3 * 12000) / 576, 9)
    expect(markerWidthHz('FT4', { periodS: 7.5 })).toBeCloseTo(62.5, 9)
  })

  it("FST4 and FST4W are 3 spacings of 12000/nsps, with the plotter's own nsps per period (plotter.cpp:513-524)", () => {
    const plotterNsps: Record<number, number> = { 15: 800, 30: 1680, 60: 4000, 120: 8400, 300: 21504, 900: 66560, 1800: 134400 }
    for (const tier of ['FST4', 'FST4W'] as const) {
      for (const [period, nsps] of Object.entries(plotterNsps)) {
        expect(markerWidthHz(tier, { periodS: Number(period) }), `${tier}-${period}`).toBeCloseTo((3 * 12000) / nsps, 9)
      }
    }
  })

  it('Q65 is 65 spacings of 2^submode × 12000/nsps (plotter.cpp:562-571)', () => {
    const nsps: Record<number, number> = { 15: 1800, 30: 3600, 60: 7200, 120: 16000, 300: 41472 }
    for (const [period, n] of Object.entries(nsps)) {
      for (let sub = 0; sub <= 4; sub++) {
        const want = 65 * 2 ** sub * (12000 / n)
        expect(markerWidthHz('Q65', { periodS: Number(period), q65Submode: sub }), `Q65-${period}${'ABCDE'[sub]}`).toBeCloseTo(want, 6)
      }
    }
    // The one operators run most on 6 m: Q65-60A is 108 Hz wide, not a line.
    expect(markerWidthHz('Q65', { periodS: 60, q65Submode: 0 })).toBeCloseTo(108.333, 3)
  })

  it('JT65 is 65 spacings of 11025/4096 Hz, doubled for B and again for C (plotter.cpp:572-576)', () => {
    const a = (65 * 11025) / 4096
    expect(markerWidthHz('JT65', { periodS: 60, jt65Submode: 0 })).toBeCloseTo(a, 9)
    expect(markerWidthHz('JT65', { periodS: 60, jt65Submode: 1 })).toBeCloseTo(2 * a, 9)
    expect(markerWidthHz('JT65', { periodS: 60, jt65Submode: 2 })).toBeCloseTo(4 * a, 9)
  })

  it('stays a single line where WSJT-X gives no width for the signal Nexus sends', () => {
    // TempoFast / TempoDeep are Nexus's own modes and FT2 is Decodium's: WSJT-X draws none of them.
    // MSK144: WSJT-X hides the Wide Graph and shows the Fast Graph (mainwindow.cpp:11490-11491).
    // WSPR: WSJT-X centres its TX bracket on the TX frequency (plotter.cpp:697-700) because it
    // transmits WSPR centred there (mainwindow.cpp:12815); Nexus's beacon puts its lowest tone on
    // the offset, so that bracket would sit 2.2 Hz below Nexus's own signal.
    // JS8 is not a WSJT-X mode.
    for (const tier of ['TempoFast', 'TempoDeep', 'FT2', 'MSK144', 'WSPR', 'JS8'] as const) {
      expect(markerWidthHz(tier, { periodS: 15, q65Submode: 0, jt65Submode: 0 }), tier).toBeNull()
    }
    expect(markerWidthHz(null)).toBeNull()
    expect(markerWidthHz(undefined)).toBeNull()
  })

  it('never guesses a period or submode it was not given', () => {
    expect(markerWidthHz('FST4', { periodS: 45 }), 'FST4 has no 45 s period').toBeNull()
    expect(markerWidthHz('FST4', {}), 'FST4 without a period').toBeNull()
    expect(markerWidthHz('Q65', { periodS: 60 }), 'Q65 without a submode (settings not loaded)').toBeNull()
    expect(markerWidthHz('Q65', { periodS: 60, q65Submode: 5 }), 'Q65 has no submode F').toBeNull()
    expect(markerWidthHz('Q65', { periodS: 90, q65Submode: 0 }), 'Q65 has no 90 s period').toBeNull()
    expect(markerWidthHz('JT65', { periodS: 60 }), 'JT65 without a submode').toBeNull()
    expect(markerWidthHz('JT65', { periodS: 60, jt65Submode: 3 }), 'JT65 has no submode D').toBeNull()
  })
})

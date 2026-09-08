// scripts/js8-sanitise-logs.test.mjs — gates for the JS8 log sanitiser.
//
// WHY these tests exist: the fixture that comes out of the sanitiser is the ONLY oracle for
// the frame codec's i3 bit order, type bits and real JSC indices (spec §Verification), and it
// is cut from a file under the operator's Windows profile. Two things must therefore be
// provable without opening the real log: (1) the parse is byte-faithful (trailing spaces in
// the rendered text are wire truth — "KD2UWR: @HB HEARTBEAT FN30 " ends in a space), and
// (2) the leak check has a positive control (feedback-negative-results-need-a-positive-control:
// "grep found nothing" proves nothing unless the same grep finds a planted leak).
//
// Run: node --test scripts/js8-sanitise-logs.test.mjs
import test from 'node:test'
import assert from 'node:assert/strict'
import { parseAllTxt, parseDirectedTxt, renderFrames, renderDirected, leaks } from './js8-sanitise-logs.mjs'

// Two periods, CRLF, a negative snr glued to the time, a Transmitting line and a band line.
const ALL = [
  '2026-01-22 23:57:38  7.078 MHz  JS8',
  '2026-01-22 23:58:30  5  0.1  773 A  iXNuhbCBNNQl         0    K0OG VA3MMU ',
  '2026-01-22 23:58:30-11  0.1  645 A  2Wu+WEWCuiZG         3    KD2UWR: @HB HEARTBEAT FN30 ',
  '2026-01-23 00:00:00  Transmitting 14.078 MHz  JS8:  N0CALL: @ALLCALL CQ CQ CQ EN52 ',
  '2026-01-23 00:00:00 -1  0.1  943 A  S+xkvHDA67qc         3    NO1ZE: KD2UWR HEARTBEAT SNR +07 ',
  '2026-01-24 04:01:20 -5  0.1 1702 B  UASw6+SGVRE+         4    SIGNAL IS FADING FOR ',
  '2026-01-24 17:14:00-11  0.9 1577 A  +H6JFDi3XTXd         2     WFD 1H MO',
].join('\r\n') + '\r\n'

test('parseAllTxt keeps decode lines only, in order, with relative seconds and byte-exact text', () => {
  const rows = parseAllTxt(ALL)
  assert.equal(rows.length, 5)
  assert.deepEqual(rows[0], { t: 0, letter: 'A', snr: 5, dt: '0.1', freq: 773, sixbit: 'iXNuhbCBNNQl', i3: 0, text: 'K0OG VA3MMU ' })
  assert.equal(rows[1].snr, -11)
  assert.equal(rows[1].text, 'KD2UWR: @HB HEARTBEAT FN30 ')
  assert.equal(rows[2].t, 90)                      // 23:58:30 → 00:00:00 next day
  assert.equal(rows[3].t, 90 + 24 * 3600 + 4 * 3600 + 80)
  assert.equal(rows[4].text, ' WFD 1H MO')         // leading space kept
})

test('renderFrames emits the binding column format with # +N clock lines between periods', () => {
  const out = renderFrames(parseAllTxt(ALL))
  const lines = out.split('\n')
  assert.equal(lines[0].startsWith('# JS8 golden frames'), true)
  const body = lines.filter((l) => !l.startsWith('# JS8') && !l.startsWith('# Columns') && !l.startsWith('# A `'))
  assert.deepEqual(body.slice(0, 5), [
    '# +0',
    'A 5 0.1 773 iXNuhbCBNNQl 0 K0OG VA3MMU ',
    'A -11 0.1 645 2Wu+WEWCuiZG 3 KD2UWR: @HB HEARTBEAT FN30 ',
    '# +90',
    'A -1 0.1 943 S+xkvHDA67qc 3 NO1ZE: KD2UWR HEARTBEAT SNR +07 ',
  ])
  assert.equal(out.endsWith('\n'), true)
  assert.equal(out.includes('\r'), false)
  assert.equal(out.includes('Transmitting'), false)
  assert.equal(out.includes('MHz'), false)
})

// Real multi-speed logs interleave A/B decode timestamps, so a later-logged decode can carry an
// EARLIER timestamp. The clock line is clamped to be non-decreasing (Math.max(0, ...)), because
// the reassembly oracle needs a monotone cadence clock and the Rust reader parses `# +N` as u64.
test('renderFrames clamps a backwards decode-timestamp step to # +0', () => {
  const OUT_OF_ORDER = [
    '2026-01-23 00:10:20  1  0.1  700 B  SOs-kw+F6XqA         3    KS1DMD: @HB HEARTBEAT ',
    '2026-01-23 00:10:15 -7 -0.3 1502 A  SJc86wXmk3qP         3    KE0ZDH: @HB HEARTBEAT ',
  ].join('\r\n') + '\r\n'
  const rows = parseAllTxt(OUT_OF_ORDER)
  assert.equal(rows[1].t, -5)                       // parse keeps the true (negative) delta
  const body = renderFrames(rows).split('\n').filter((l) => !l.startsWith('# JS8') && !l.startsWith('# Columns') && !l.startsWith('# A `'))
  assert.deepEqual(body.slice(0, 4), [
    '# +0',
    'B 1 0.1 700 SOs-kw+F6XqA 3 KS1DMD: @HB HEARTBEAT ',
    '# +0',                                          // clamped, not `# +-5`
    'A -7 -0.3 1502 SJc86wXmk3qP 3 KE0ZDH: @HB HEARTBEAT ',
  ])
})

test('parseDirectedTxt strips the ♢ marker, trailing whitespace and every date/dial column', () => {
  const dir = '2026-01-22 23:59:57\t7.078000\t1206\t-11\tKD2UWR: @HB HEARTBEAT ♢ \r\n' +
              '2026-01-23 00:00:12\t7.078000\t1206\t+06\tN6CYB: KD2UWR HEARTBEAT SNR +07 ♢ \r\n' +
              '2026-01-25 01:00:00\t7.078000\t866\t+09\tKD8NOA: KJ5MIW MSG FRIDAY CONTACT. ♢ \r\n'
  const rows = parseDirectedTxt(dir)
  assert.deepEqual(rows, [
    { freq: 1206, snr: -11, text: 'KD2UWR: @HB HEARTBEAT' },
    { freq: 1206, snr: 6, text: 'N6CYB: KD2UWR HEARTBEAT SNR +07' },
    { freq: 866, snr: 9, text: 'KD8NOA: KJ5MIW MSG FRIDAY CONTACT.' },
  ])
  const out = renderDirected(rows)
  assert.equal(out, '1206\t-11\tKD2UWR: @HB HEARTBEAT\n1206\t6\tN6CYB: KD2UWR HEARTBEAT SNR +07\n866\t9\tKD8NOA: KJ5MIW MSG FRIDAY CONTACT.\n')
})

test('leaks() has a positive control: a planted username or profile path trips it, clean text does not', () => {
  const names = ['zaphod', 'ZAPHOD']
  assert.equal(leaks('A 5 0.1 773 iXNuhbCBNNQl 0 K0OG VA3MMU ', names), null)
  assert.match(leaks('A 5 0.1 773 iXNuhbCBNNQl 0 hello zaphod', names), /zaphod/)
  assert.match(leaks('C:\\Users\\Zaphod\\AppData\\Local\\JS8Call', names), /Users|AppData|zaphod/i)
  assert.match(leaks('/mnt/c/Users/x/AppData/Local/JS8Call/ALL.TXT', names), /AppData/)
})

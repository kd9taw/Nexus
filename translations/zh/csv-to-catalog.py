#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
csv-to-catalog.py — turn a filled Nexus Chinese translation CSV into `ui/src/i18n/zh.ts`.

    python3 csv-to-catalog.py nexus-zh-translation.csv -o ../../ui/src/i18n/zh.ts \\
        --splash-out splash-zh.json
    python3 csv-to-catalog.py nexus-zh-translation.csv             # writes the catalog to stdout

The same script builds the Traditional catalog (`translations/zh-hant/`), which differs only in
the header it writes and the export it names: pass `--variant hant`. The CSV's column names stay
`chinese` in both kits — the guard on this script's side reads the column, not the language.

This is the RETURN PATH: the kit's other half. `verify-zh.py` says the filled CSV is mechanically
sound; this turns it into the TypeScript catalog the application imports.

The rows whose key starts with `splashscreen.` are not catalog keys. They are the few words of
`ui/public/splashscreen.html`, a page that loads before the application and carries its own
strings. They never go into `zh.ts`: `--splash-out` writes them as one JSON object, to be pasted
into that page's `splash-strings` block under `"zh"`.

Run the checker FIRST. This script re-checks only what it must in order to emit a correct file
(plural rows, the splash set, duplicate keys, the column header); it is not a second opinion on
the translation.

Python 3 only. No installation, no dependencies.

Exit code 0 = `zh.ts` written. Exit code 1 = nothing written, and the reason is on stderr.
"""

import argparse
import csv
import json
import sys
from collections import Counter, OrderedDict

COLUMNS = ['priority', 'key', 'english', 'chinese', 'do_not_translate', 'notes']

# The splash page's own strings travel in the CSV under this prefix (see the docstring).
SPLASH_PREFIX = 'splashscreen.'

# CLDR plural categories, in the order `PluralForms` declares them (`ui/src/i18n/types.ts`).
# Only used to validate what the CSV carries — Chinese emits NO plural object at all.
PLURAL_FORMS = ('zero', 'one', 'two', 'few', 'many', 'other')

HEADER = '''\
// CHINESE (SIMPLIFIED) — GENERATED. Do not hand-edit; edit the translation CSV and re-run the
// converter.
//
//     python3 translations/zh/csv-to-catalog.py nexus-zh-translation.csv -o ui/src/i18n/zh.ts
//
// Registered as `zh`. The two guards that keep a shipped catalog honest read the locale list
// straight out of `main.tsx` with `/installCatalog\\(\\s*'([a-z-]+)'/g` (`placeholders.test.ts`,
// `locale-switch.test.ts`), and `initLocale()` matches OS language then region, so a `zh`
// catalog serves a Simplified Chinese install (`zh-CN`, `zh-Hans`, `zh-SG`) alike. The id is
// `zh`, not `zh-CN`: the guards' pattern only matches `[a-z-]+`, and an upper-case `C` would
// drop this catalog out of both of them silently.
//
// PARTIAL BY DESIGN AT RUNTIME. Every key absent here resolves to the English source string
// (`rawMessage` falls through), so a missing translation is a Chinese screen with an English
// sentence on it — never a blank and never a key.
//
// ⚠️ AN EMPTY CSV CELL BECOMES AN ABSENT KEY, NEVER AN EMPTY STRING. `""` would satisfy the
// completeness guard (`placeholders.test.ts` tests key PRESENCE) and then render as English at
// runtime anyway (`index.ts` treats a blank as missing).
//
// ⚠️ NO PLURAL OBJECTS, AND THAT IS NOT AN OMISSION. `Intl.PluralRules('zh')` selects `other`
// for every count — Chinese does not inflect for number — so each plural key here is ONE string
// that already reads correctly with any count ("{{count}} 条通联"). The CSV carries the usual
// `::one/::few/::many/::other` rows because it is generated from the English kit; the converter
// collapses them, taking `other`. The same shape ships in `ja.ts`.
//
// ⚠️ WHAT IS NOT TRANSLATED HERE, AND WHY IT MUST STAY THAT WAY: frequencies and every number
// that is one, signal reports, callsigns, grid squares, band and mode names, Q-codes, RST,
// POTA/SOTA references, ADIF field names, CW macro tokens, award names and product names. A
// decimal comma reaching a dial, a log or a CAT command is an operating fault, not a matter of
// taste — `placeholders.test.ts` fails this file if a digit-comma-digit sequence ever appears in
// it, alongside the checks that every {{placeholder}} and <marker> matches English.
//
// Placeholders may MOVE (Chinese word order differs — that is why they exist); they may never be
// renamed, dropped or added. Markers are names, never HTML: only a marker the call site passed
// in `tags` becomes an element, so nothing here can introduce one.

import type { PartialCatalog } from './index'

export const ZH: PartialCatalog = {
'''

# The Traditional variant of the same kit. Same contract as `ZH` — one string per plural, no
# plural object, absent key falls back to English — and the id is lower-case for the same reason
# `zh` is: the guards' `/installCatalog\(\s*'([a-z-]+)'/` would miss `zh-Hant` entirely.
HEADER_HANT = '''\
// CHINESE (TRADITIONAL, Taiwan wording) — GENERATED. Do not hand-edit; edit the translation CSV
// and re-run the converter.
//
//     python3 translations/zh/csv-to-catalog.py \\
//         translations/zh-hant/nexus-zh-hant-translation.csv -o ui/src/i18n/zh-hant.ts --variant hant
//
// Registered as `zh-hant`, and the id stays lower-case: the guards that read the shipped locale
// list out of `main.tsx` match `[a-z-]+`, so `zh-Hant` would drop this catalog out of them
// silently. `initLocale()` prefers it for a Traditional-script or Taiwan/Hong Kong/Macao system
// and falls back to `zh` for everything else.
//
// PARTIAL BY DESIGN AT RUNTIME, like `zh.ts`: an absent key renders the English source string.
// One string per plural key — `Intl.PluralRules('zh-hant')` selects `other` for every count.

import type { PartialCatalog } from './index'

export const ZH_HANT: PartialCatalog = {
'''


def ts_string(text):
    """A TypeScript double-quoted string literal for `text`.

    JSON string syntax is a subset of TypeScript's, so `json.dumps` handles the two characters
    that actually break a literal — the quote and the backslash — plus control characters and
    embedded newlines. `ensure_ascii=False` keeps the Chinese readable, matching de.ts.
    U+2028/U+2029 are legal in a modern JS string but are escaped anyway: they are invisible in
    an editor and have historically terminated a line.
    """
    out = json.dumps(text, ensure_ascii=False)
    return out.replace('\u2028', '\\u2028').replace('\u2029', '\\u2029')


def load(path):
    with open(path, encoding='utf-8-sig', newline='') as fh:
        reader = csv.DictReader(fh)
        return reader.fieldnames, list(reader)


def build(rows, errors):
    """CSV rows -> (an ordered {key: str} mapping, the splash strings, stats). Appends to `errors`."""
    catalog_rows = [r for r in rows if not r['key'].startswith(SPLASH_PREFIX)]
    splash_rows = [r for r in rows if r['key'].startswith(SPLASH_PREFIX)]

    # Every form the CSV carries for a plural base, filled or not — a half-filled set can only be
    # seen against the rows the kit shipped.
    all_forms = OrderedDict()
    for row in catalog_rows:
        key = row['key']
        if '::' not in key:
            continue
        base, form = key.rsplit('::', 1)
        if form not in PLURAL_FORMS:
            errors.append('%s: "%s" is not a plural category. The kit uses %s.'
                          % (key, form, ', '.join(PLURAL_FORMS)))
            continue
        all_forms.setdefault(base, OrderedDict())[form] = row['chinese'].strip()

    # ── refuse a half-filled plural, and a plural without `other` ────────────────────────
    #
    # Chinese ships one sentence per plural, taken from `other`, so `other` is the row that
    # matters. A base whose `other` is empty but whose `one` is filled would silently vanish
    # from the catalog and fall back to English — which is the honest outcome, but it is a row
    # the translator believed they had done, so it is refused here.
    bad_bases = set()
    for base, forms in all_forms.items():
        filled = [f for f, v in forms.items() if v]
        if not filled:
            continue
        if 'other' not in forms or not forms['other']:
            bad_bases.add(base)
            errors.append('%s: the `other` row is empty. Chinese ships one sentence for every '
                          'count and the converter takes `other`, so this key would fall back to '
                          'English. Fill it (normally the same sentence as `::few`/`::many`).'
                          % base)
            continue
        empty = [f for f in forms if not forms[f]]
        if empty:
            bad_bases.add(base)
            errors.append('%s: half-filled plural. Filled: %s. Empty: %s.\n'
                          '    Chinese reads the same for every count, so all four rows normally '
                          'hold the same sentence — but a blank row is a sentence nobody looked at.'
                          % (base, ', '.join(filled), ', '.join(empty)))

    # ── the splash page: every string or none ────────────────────────────────────────────
    #
    # `splash.test.ts` requires each language the app installs to carry every splash string,
    # none blank. A partial set cannot be shipped, so it is refused here rather than at CI.
    splash = OrderedDict((r['key'][len(SPLASH_PREFIX):], r['chinese'].strip()) for r in splash_rows)
    splash_filled = [k for k, v in splash.items() if v]
    if splash_filled and len(splash_filled) != len(splash):
        errors.append('the splash page strings are half filled. Filled: %s. Empty: %s.\n'
                      '    The splash page needs every one of its strings in each language, or '
                      'none. Fill them all, or clear them all.'
                      % (', '.join(splash_filled),
                         ', '.join(k for k, v in splash.items() if not v)))

    seen = Counter(r['key'] for r in rows)
    for key, n in sorted(seen.items()):
        if n > 1:
            errors.append('%s: appears %d times. The program matches rows by key; a duplicate '
                          'means one of them is silently discarded.' % (key, n))

    # ── the emit pass, in CSV row order ──────────────────────────────────────────────────
    #
    # A plural lands at the position of its FIRST row, so the file's order is the order the
    # translator worked in (the CSV is priority-ordered), and the same CSV always produces the
    # same bytes.
    out = OrderedDict()
    stats = Counter()
    for row in catalog_rows:
        key = row['key']
        zh = row['chinese']
        if zh.strip() == '':
            # ⚠️ SKIPPED ENTIRELY — not emitted as "". See the file header.
            stats['skipped'] += 1
            continue
        if '::' in key:
            base, form = key.rsplit('::', 1)
            if form not in PLURAL_FORMS or base in bad_bases:
                continue
            if base in out:
                continue                      # one string per plural, whichever row comes first
            other = all_forms.get(base, {}).get('other', '')
            if not other:
                continue
            out[base] = other
            stats['plural'] += 1
        else:
            if key in out:
                continue                      # already reported as a duplicate
            out[key] = zh
            stats['plain'] += 1
        stats['filled'] += 1
        try:
            stats['tier%d' % int(row['priority'])] += 1
        except (ValueError, KeyError):
            pass
    return out, (splash if splash_filled else OrderedDict()), stats


def render(entries, variant):
    lines = [HEADER if variant == 'hans' else HEADER_HANT]
    for key, value in entries.items():
        lines.append('  %s: %s,\n' % (ts_string(key), ts_string(value)))
    lines.append('}\n')
    return ''.join(lines)


def main(argv):
    ap = argparse.ArgumentParser(description='Turn a filled Nexus Chinese CSV into ui/src/i18n/zh.ts.')
    ap.add_argument('csv', help='the filled nexus-zh-translation.csv')
    ap.add_argument('-o', '--out', help='write here instead of stdout (e.g. ui/src/i18n/zh.ts)')
    ap.add_argument('--splash-out', help='write the splash page strings here, as JSON')
    ap.add_argument('--variant', choices=('hans', 'hant'), default='hans',
                    help='which Chinese catalog to write: hans -> ZH, hant -> ZH_HANT')
    args = ap.parse_args(argv[1:])

    try:
        header, rows = load(args.csv)
    except UnicodeDecodeError:
        print('PROBLEM: this file is not UTF-8. Re-save it as CSV UTF-8.', file=sys.stderr)
        return 1
    except OSError as exc:
        print('PROBLEM: could not open %s (%s)' % (args.csv, exc), file=sys.stderr)
        return 1

    if header != COLUMNS:
        print('PROBLEM: the columns are not the ones the kit shipped.\n'
              '    expected: %s\n    found:    %s'
              % (', '.join(COLUMNS), ', '.join(header or [])), file=sys.stderr)
        return 1

    errors = []
    entries, splash, stats = build(rows, errors)
    if errors:
        print('REFUSING TO WRITE A CATALOG — %d problem%s:'
              % (len(errors), '' if len(errors) == 1 else 's'), file=sys.stderr)
        for e in errors:
            print('  %s' % e, file=sys.stderr)
        print('\nNothing was written. Fix the CSV (run verify-zh.py) and try again.', file=sys.stderr)
        return 1

    text = render(entries, args.variant)
    if args.out:
        with open(args.out, 'w', encoding='utf-8', newline='\n') as fh:
            fh.write(text)
    else:
        sys.stdout.write(text)
    if splash and args.splash_out:
        with open(args.splash_out, 'w', encoding='utf-8', newline='\n') as fh:
            fh.write(json.dumps(splash, ensure_ascii=False, indent=2) + '\n')

    where = args.out or '(stdout)'
    print('wrote %s' % where, file=sys.stderr)
    print('  rows read:        %d' % len(rows), file=sys.stderr)
    print('  cells filled:     %d' % stats['filled'], file=sys.stderr)
    print('  cells left empty: %d  (absent from the catalog — falls back to English)'
          % stats['skipped'], file=sys.stderr)
    print('  catalog entries:  %d  (%d plain, %d plural collapsed to one string)'
          % (len(entries), stats['plain'], stats['plural']), file=sys.stderr)
    tiers = ', '.join('%d:%d' % (t, stats['tier%d' % t]) for t in range(1, 7) if stats['tier%d' % t])
    if tiers:
        print('  by priority tier: %s' % tiers, file=sys.stderr)
    if splash:
        if args.splash_out:
            print('  splash strings:   %d, written to %s' % (len(splash), args.splash_out),
                  file=sys.stderr)
        else:
            print('  splash strings:   %d filled and NOT written — pass --splash-out FILE'
                  % len(splash), file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))

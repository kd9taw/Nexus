#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
csv-to-catalog.py — turn a filled Nexus Italian translation CSV into `ui/src/i18n/it.ts`.

    python3 csv-to-catalog.py nexus-it-translation.csv -o ../../ui/src/i18n/it.ts \\
        --splash-out splash-it.json
    python3 csv-to-catalog.py nexus-it-translation.csv            # writes the catalog to stdout

This is the RETURN PATH: the kit's other half. `verify-it.py` says the filled CSV is mechanically
sound; this turns it into the TypeScript catalog the application imports.

The rows whose key starts with `splashscreen.` are not catalog keys. They are the few words of
`ui/public/splashscreen.html`, a page that loads before the application and carries its own
strings. They never go into `it.ts`: `--splash-out` writes them as one JSON object, to be pasted
into that page's `splash-strings` block under `"it"`.

Run the checker FIRST. This script re-checks only what it must in order to emit a correct file
(plural pairs, the splash set, duplicate keys, the column header); it is not a second opinion on
the translation.

Python 3 only. No installation, no dependencies.

Exit code 0 = `it.ts` written. Exit code 1 = nothing written, and the reason is on stderr.
"""

import argparse
import csv
import json
import sys
from collections import Counter, OrderedDict

COLUMNS = ['priority', 'key', 'english', 'italian', 'do_not_translate', 'notes']

# The splash page's own strings travel in the CSV under this prefix (see the docstring).
SPLASH_PREFIX = 'splashscreen.'

# CLDR plural categories, in the order `PluralForms` declares them (`ui/src/i18n/types.ts`).
# The emitted object follows THIS order, never the row order in the CSV, so two files with the
# same content but a different row order still produce byte-identical output.
PLURAL_FORMS = ('zero', 'one', 'two', 'few', 'many', 'other')

HEADER = '''\
// ITALIAN — GENERATED. Do not hand-edit; edit the translation CSV and re-run the converter.
//
//     python3 translations/it/csv-to-catalog.py nexus-it-translation.csv -o ui/src/i18n/it.ts
//
// Registered as `it`. The two guards that keep a shipped catalog honest read the locale list
// straight out of `main.tsx` with `/installCatalog\\(\\s*'([a-z-]+)'/g` (`placeholders.test.ts`,
// `locale-switch.test.ts`), and `initLocale()` matches OS language then region, so an `it`
// catalog serves an Italian install (`it-IT`) and a Swiss Italian one (`it-CH`) alike.
//
// PARTIAL BY DESIGN AT RUNTIME. Every key absent here resolves to the English source string
// (`rawMessage` falls through), so a missing translation is an Italian screen with an English
// sentence on it — never a blank and never a key.
//
// ⚠️ AN EMPTY CSV CELL BECOMES AN ABSENT KEY, NEVER AN EMPTY STRING. `""` would satisfy the
// completeness guard (`placeholders.test.ts` tests key PRESENCE) and then render as English at
// runtime anyway (`index.ts` treats a blank as missing) — a catalog that reports 100% complete
// and is largely blank. An omitted key is the honest form of the same fallback.
//
// ⚠️ WHAT IS NOT TRANSLATED HERE, AND WHY IT MUST STAY THAT WAY: frequencies and every number
// that is one, signal reports, callsigns, grid squares, band and mode names, Q-codes, RST,
// POTA/SOTA references, ADIF field names, CW macro tokens, award names and product names. An
// Italian decimal comma reaching a dial, a log or a CAT command is an operating fault, not a
// matter of taste — `placeholders.test.ts` fails this file if a digit-comma-digit sequence ever
// appears in it, alongside the checks that every {{placeholder}} and <marker> matches English.
//
// Placeholders may MOVE (Italian word order differs — that is why they exist); they may never be
// renamed, dropped or added.
//
// PLURALS: `Intl.PluralRules('it')` selects three categories — `one`, `many` and `other`. The
// CSV carries `one` and `other`; `many` fires only on an exact million, and `selectPluralForm`
// falls back to `other` for any category a catalog omits. Italian selects `other` for a count of
// ZERO, as English does.

import type { PartialCatalog } from './index'

export const IT: PartialCatalog = {
'''


def ts_string(text):
    """A TypeScript double-quoted string literal for `text`.

    JSON string syntax is a subset of TypeScript's, so `json.dumps` handles the two characters
    that actually break a literal — the quote and the backslash — plus control characters and
    embedded newlines. `ensure_ascii=False` keeps the accents readable, matching de.ts.
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
    """CSV rows -> (an ordered {key: str | {form: str}} mapping, the splash strings, stats).
    Appends to `errors`."""
    catalog_rows = [r for r in rows if not r['key'].startswith(SPLASH_PREFIX)]
    splash_rows = [r for r in rows if r['key'].startswith(SPLASH_PREFIX)]

    # Every form the CSV carries for a plural base, filled or not — a half-filled pair can only
    # be seen against the pair the kit shipped.
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
        all_forms.setdefault(base, OrderedDict())[form] = row['italian'].strip()

    # ── refuse a half-filled plural, loudly, in BOTH directions ─────────────────────────
    #
    # `one` filled and `other` empty is the case verify-it.py warns about. The reverse is no
    # better and is worse than leaving both blank: `selectPluralForm` resolves a missing category
    # to `other`, so a catalog holding only `other` renders the PLURAL sentence for a count of 1,
    # where an absent key would have fallen back to correct English.
    bad_bases = set()
    for base, forms in all_forms.items():
        filled = [f for f, v in forms.items() if v]
        if not filled:
            continue
        empty = [f for f in forms if not forms[f]]
        if empty:
            bad_bases.add(base)
            errors.append(
                '%s: half-filled plural. Filled: %s. Empty: %s.\n'
                '    Both forms are needed. Leaving one blank does not fall back to English — '
                'the runtime resolves a missing form to `other`, so the count reads wrong for '
                'the other case. Fill both rows, or clear both.'
                % (base, ', '.join(filled), ', '.join(empty)))
        elif 'other' not in forms:
            bad_bases.add(base)
            errors.append(
                '%s: plural has no `other` row. `other` is the required form and the fallback '
                'for every category this locale can select; a plural object without it is not a '
                'valid catalog entry.' % base)

    # ── the splash page: every string or none ───────────────────────────────────────────
    #
    # `splash.test.ts` requires each language the app installs to carry every splash string,
    # none blank. A partial set cannot be shipped, so it is refused here rather than at CI.
    splash = OrderedDict((r['key'][len(SPLASH_PREFIX):], r['italian'].strip()) for r in splash_rows)
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

    # ── the emit pass, in CSV row order ────────────────────────────────────────────────
    #
    # A plural lands at the position of its FIRST row, so the file's order is the order the
    # translator worked in (the CSV is priority-ordered), and the same CSV always produces the
    # same bytes.
    out = OrderedDict()
    stats = Counter()
    for row in catalog_rows:
        key = row['key']
        it = row['italian']
        if it.strip() == '':
            # ⚠️ SKIPPED ENTIRELY — not emitted as "". See the file header: an absent key falls
            # back to English by design; an empty string would pass the completeness guard and
            # then render as English anyway, which is the same screen reached by a lie.
            stats['skipped'] += 1
            continue
        if '::' in key:
            base, form = key.rsplit('::', 1)
            if form not in PLURAL_FORMS or base in bad_bases:
                continue
            if base not in out:
                out[base] = OrderedDict()
                stats['plural'] += 1
            out[base][form] = it
        else:
            if key in out:
                continue                      # already reported as a duplicate
            out[key] = it
            stats['plain'] += 1
        stats['filled'] += 1
        try:
            stats['tier%d' % int(row['priority'])] += 1
        except (ValueError, KeyError):
            pass
    return out, (splash if splash_filled else OrderedDict()), stats


def render(entries):
    lines = [HEADER]
    for key, value in entries.items():
        if isinstance(value, str):
            lines.append('  %s: %s,\n' % (ts_string(key), ts_string(value)))
        else:
            forms = ', '.join('%s: %s' % (ts_string(f), ts_string(value[f]))
                              for f in PLURAL_FORMS if f in value)
            lines.append('  %s: { %s },\n' % (ts_string(key), forms))
    lines.append('}\n')
    return ''.join(lines)


def main(argv):
    ap = argparse.ArgumentParser(description='Turn a filled Nexus Italian CSV into ui/src/i18n/it.ts.')
    ap.add_argument('csv', help='the filled nexus-it-translation.csv')
    ap.add_argument('-o', '--out', help='write here instead of stdout (e.g. ui/src/i18n/it.ts)')
    ap.add_argument('--splash-out', help='write the splash page strings here, as JSON')
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
        print('\nNothing was written. Fix the CSV (run verify-it.py) and try again.',
              file=sys.stderr)
        return 1

    text = render(entries)
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
    print('  catalog entries:  %d  (%d plain, %d plural)'
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

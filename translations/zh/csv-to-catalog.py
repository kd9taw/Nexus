#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
csv-to-catalog.py — turn a filled Nexus Chinese translation CSV into `ui/src/i18n/zh.ts` (Simplified)
or `ui/src/i18n/zh-hant.ts` (Traditional, Taiwan wording).

    python3 translations/zh/csv-to-catalog.py translations/zh/nexus-zh-translation.csv \\
        -o ui/src/i18n/zh.ts --splash-out translations/zh/splash-zh.json
    python3 translations/zh/csv-to-catalog.py translations/zh-hant/nexus-zh-hant-translation.csv \\
        --variant hant -o ui/src/i18n/zh-hant.ts --splash-out translations/zh-hant/splash-zh-hant.json

One script serves both kits: the two CSVs share a layout (the translation column is `chinese` in
each), and only the header and the exported name differ.

This is the RETURN PATH: it turns the CSV into the TypeScript catalog the application imports.
It is adapted from `translations/ru/csv-to-catalog.py`; the one rule that differs is plurals.
Chinese has no grammatical plural (`Intl.PluralRules('zh')` selects `other` for every count), so
each plural becomes ONE plain string taken from the `::other` row, as `ja.ts` does — never the
forms concatenated, which is the bug the fr/es catalogs once shipped.

The rows whose key starts with `splashscreen.` are not catalog keys. They are the words of
`ui/public/splashscreen.html`, a page that loads before the application and carries its own
strings. `--splash-out` writes them as one JSON object for that page's `splash-strings` block.

A CSV returned against an older Nexus is normal: strings change between the kit going out and
coming back. So every row is checked against `ui/src/i18n/en.ts` as the application has it now
(read through Node's type stripping, as `translations/it/make-kit.py` does), and a row whose key
is gone, or whose English has changed since it was translated, is LEFT OUT: that key renders the
current English until the kit is refreshed, never a translation of words the screen no longer
says. A plural travels whole: if any of its forms' English changed, none of it is used.

Needs Python 3 and Node 22.6 or later (the one already used to build ui/). No other dependency.

Exit code 0 = catalog written. Exit code 1 = nothing written, and the reason is on stderr.
"""

import argparse
import csv
import json
import subprocess
import sys
from collections import Counter, OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

COLUMNS = ['priority', 'key', 'english', 'chinese', 'do_not_translate', 'notes']

# The splash page's own strings travel in the CSV under this prefix (see the docstring).
SPLASH_PREFIX = 'splashscreen.'

# CLDR plural categories, in the order `PluralForms` declares them (`ui/src/i18n/types.ts`).
# The emitted object follows THIS order, never the row order in the CSV, so two files with the
# same content but a different row order still produce byte-identical output.
PLURAL_FORMS = ('zero', 'one', 'two', 'few', 'many', 'other')

COMMON = '''\
//
// PARTIAL BY DESIGN AT RUNTIME. Every key absent here resolves to the English source string
// (`rawMessage` falls through), so a missing translation is an English sentence on a Chinese
// screen — never a blank and never a key. An empty CSV cell becomes an ABSENT key, never "".
//
// ⚠️ NOT TRANSLATED, AND IT MUST STAY THAT WAY: frequencies and every number that is one, signal
// reports, callsigns, grid squares, band and mode names, Q-codes, RST, POTA/SOTA references,
// ADIF field names, CW macro tokens, award names and product names. `placeholders.test.ts`
// fails a digit-comma-digit sequence the English did not have, and checks every {{placeholder}}
// and <marker> against English.
//
// PLURALS: Chinese has no grammatical plural (`Intl.PluralRules('zh')` selects `other` for every
// count), so a key that is a plural object in en.ts is a plain string here, taken from the CSV's
// `::other` row — as in ja.ts.
//
// Placeholders may MOVE (word order differs — that is why they exist); they may never be
// renamed, dropped or added.

import type { PartialCatalog } from './index'

'''

HEADERS = {
    'hans': ('''\
// CHINESE (SIMPLIFIED) — GENERATED. Do not hand-edit; edit the translation CSV and re-run:
//
//     python3 translations/zh/csv-to-catalog.py translations/zh/nexus-zh-translation.csv \\
//         -o ui/src/i18n/zh.ts
//
// Registered as `zh`. `initLocale()` serves it to a Simplified-script or mainland/Singapore
// system (`zh-CN`, `zh-SG`, `zh-Hans`) and to a bare `zh`.
''', 'ZH'),
    'hant': ('''\
// CHINESE (TRADITIONAL, Taiwan wording) — GENERATED. Do not hand-edit; edit the translation CSV
// and re-run:
//
//     python3 translations/zh/csv-to-catalog.py translations/zh-hant/nexus-zh-hant-translation.csv \\
//         --variant hant -o ui/src/i18n/zh-hant.ts
//
// Registered as `zh-hant` (lower-case: the guards read locale ids out of `main.tsx` with
// `[a-z-]+`). `initLocale()` serves it to a Traditional-script system (`zh-Hant`, `zh-TW`,
// `zh-HK`, `zh-MO`).
''', 'ZH_HANT'),
}


def header(variant):
    top, name = HEADERS[variant]
    return top + COMMON + 'export const %s: PartialCatalog = {\n' % name


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


def load_catalog(path):
    """en.ts as the application sees it: `EN`."""
    script = ('import(%s).then((m) => process.stdout.write(JSON.stringify(m.EN)))'
              % json.dumps(Path(path).resolve().as_uri()))
    last = ''
    for cmd in (['node', '--experimental-strip-types', '--no-warnings', '-e', script],
                ['node', '--no-warnings', '-e', script]):     # Node that strips types by default
        try:
            run = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
        except OSError as exc:
            raise SystemExit('PROBLEM: could not run node (%s). Node 22.6 or later is needed.' % exc)
        if run.returncode == 0:
            return json.loads(run.stdout)
        last = run.stderr.strip()
    raise SystemExit('PROBLEM: node could not load %s:\n%s' % (path, last))


def drop_stale(rows, catalog):
    """Blank every translated row whose key is gone from `catalog` or whose English changed.

    Returns a Counter of what was dropped. Blanking (not deleting) keeps the row census intact
    for the checks in `build`; a blank cell is an absent key, i.e. the current English."""
    def english_now(key):
        base, _, form = key.partition('::')
        value = catalog.get(base)
        if value is None:
            return None
        if not form:
            return value if isinstance(value, str) else None
        if isinstance(value, str):
            return None
        # A form English does not use (Chinese kits carry few/many too) has nothing to compare.
        return value.get(form, value.get('other'))

    stale_bases = set()
    for r in rows:
        key = r['key']
        if key.startswith(SPLASH_PREFIX) or not r['chinese'].strip():
            continue
        base, _, form = key.partition('::')
        if form and form not in ('one', 'other') and isinstance(catalog.get(base), dict) \
                and form not in catalog[base]:
            continue
        if english_now(key) != r['english']:
            stale_bases.add(base)
    dropped = Counter()
    for r in rows:
        key = r['key']
        if key.startswith(SPLASH_PREFIX) or not r['chinese'].strip():
            continue
        base = key.partition('::')[0]
        if base in stale_bases:
            dropped['gone' if base not in catalog else 'changed'] += 1
            r['chinese'] = ''
    return dropped, stale_bases


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
        all_forms.setdefault(base, OrderedDict())[form] = row['chinese'].strip()

    # ── refuse a half-filled plural, loudly, in BOTH directions ─────────────────────────
    #
    # `one` filled and `other` empty is the case a translator most often leaves. The reverse is no
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

    # ── the emit pass, in CSV row order ────────────────────────────────────────────────
    #
    # A plural lands at the position of its FIRST row, so the file's order is the order the
    # translator worked in (the CSV is priority-ordered), and the same CSV always produces the
    # same bytes.
    out = OrderedDict()
    stats = Counter()
    for row in catalog_rows:
        key = row['key']
        it = row['chinese']
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
            # One plain string per plural, from `::other` (see the docstring). It lands at the
            # position of the plural's first row; the other forms' rows add nothing.
            if base not in out:
                out[base] = all_forms[base]['other']
                stats['plural'] += 1
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


def render(entries, variant):
    lines = [header(variant)]
    for key, value in entries.items():
        lines.append('  %s: %s,\n' % (ts_string(key), ts_string(value)))
    lines.append('}\n')
    return ''.join(lines)


def main(argv):
    ap = argparse.ArgumentParser(description='Turn a filled Nexus Chinese CSV into ui/src/i18n/zh.ts or zh-hant.ts.')
    ap.add_argument('csv', help='the filled nexus-zh-translation.csv or nexus-zh-hant-translation.csv')
    ap.add_argument('-o', '--out', help='write here instead of stdout (e.g. ui/src/i18n/zh.ts)')
    ap.add_argument('--variant', choices=sorted(HEADERS), default='hans',
                    help='hans writes `ZH` (Simplified); hant writes `ZH_HANT` (Traditional)')
    ap.add_argument('--splash-out', help='write the splash page strings here, as JSON')
    ap.add_argument('--catalog', default=str(ROOT / 'ui/src/i18n/en.ts'),
                    help='the English catalog rows are checked against (default: ui/src/i18n/en.ts)')
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

    catalog = load_catalog(args.catalog)
    dropped, stale_bases = drop_stale(rows, catalog)
    errors = []
    entries, splash, stats = build(rows, errors)
    if errors:
        print('REFUSING TO WRITE A CATALOG — %d problem%s:'
              % (len(errors), '' if len(errors) == 1 else 's'), file=sys.stderr)
        for e in errors:
            print('  %s' % e, file=sys.stderr)
        print('\nNothing was written. Fix the CSV and try again.',
              file=sys.stderr)
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
    if dropped:
        print('  left out, English changed since translated: %d keys'
              % sum(1 for b in stale_bases if b in catalog), file=sys.stderr)
        print('  left out, key no longer in English:        %d keys'
              % sum(1 for b in stale_bases if b not in catalog), file=sys.stderr)
    print('  catalog entries:  %d  (%d plain, %d plural as one string)'
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

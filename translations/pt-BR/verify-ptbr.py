#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
verify-ptbr.py — check a filled-in Nexus pt-BR translation CSV.

    python3 verify-ptbr.py nexus-ptbr-translation.csv

It checks the MECHANICAL things only: that every row is still there, that the English and
do-not-translate columns were not edited, and that nothing which must survive a translation
byte-for-byte got renamed, dropped, added, or re-punctuated. It says nothing about whether the
Portuguese is good — that is your job and it cannot be checked by a program.

Python 3 only. No installation, no dependencies.

Exit code 0 = nothing broken (warnings may still be printed). Exit code 1 = something to fix.
"""

import csv
import hashlib
import re
import sys
from collections import Counter, defaultdict

# ── What the kit shipped, so a missing row can be detected without a second file ───────────
EXPECTED_ROWS = 5044
EXPECTED_KEYS_SHA = '6f951a3e68e34099c15f05316bab03bc36462111893ce5daacd4b1acf5ec1037'
EXPECTED_ENGLISH_SHA = '4aaff5af36e7140a96e4c92a63fc0eb93ede25b2e2963b51435bb4df2c7983f7'
EXPECTED_DNT_SHA = 'bead37d97049e6ed0152c0aa72cef3bff434ca9b2107db65b383163f15ca9abd'
EXPECTED_TIERS = {1: 542, 2: 313, 3: 834, 4: 333, 5: 2269, 6: 753}
COLUMNS = ['priority', 'key', 'english', 'portuguese', 'do_not_translate', 'notes']

RE_PLACEHOLDER = re.compile(r'\{\{(\w+)\}\}')
RE_SINGLE_BRACE = re.compile(r'(?<!\{)\{([a-z]\w*)\}(?!\})')
RE_MARKER = re.compile(r'<(/?)([a-zA-Z][a-zA-Z0-9]*)>')
RE_CWTOKEN = re.compile(r'(?<!\{)\{([A-Z][A-Z0-9_]*)\}(?!\})')
RE_DECIMAL_COMMA = re.compile(r'\d,\d')

MAX_EXAMPLES = 8


class Report:
    """Collects problems by kind so the output is a short list, not 4000 lines."""

    def __init__(self):
        self.errors = defaultdict(list)
        self.warnings = defaultdict(list)

    def error(self, kind, key, detail):
        self.errors[kind].append((key, detail))

    def warn(self, kind, key, detail):
        self.warnings[kind].append((key, detail))

    def n_errors(self):
        return sum(len(v) for v in self.errors.values())

    def n_warnings(self):
        return sum(len(v) for v in self.warnings.values())

    def render(self):
        for label, bucket in (('PROBLEM', self.errors), ('WORTH A LOOK', self.warnings)):
            for kind, items in bucket.items():
                print()
                print('%s: %s  (%d row%s)' % (label, kind, len(items), '' if len(items) == 1 else 's'))
                for key, detail in items[:MAX_EXAMPLES]:
                    print('    %s' % key)
                    for line in detail.splitlines():
                        print('        %s' % line)
                if len(items) > MAX_EXAMPLES:
                    print('    ... and %d more like it' % (len(items) - MAX_EXAMPLES))


def sha(text):
    return hashlib.sha256(text.encode('utf-8')).hexdigest()


def markers(text):
    """Counter of marker tokens, e.g. {'<b>': 2, '</b>': 2}."""
    return Counter('<%s%s>' % (slash, name) for slash, name in RE_MARKER.findall(text))


def is_all_invariant(english, required):
    """True when the English is nothing but tokens, numbers and punctuation.

    Such a row is legitimately identical in Portuguese ("FT8", "{{call}}", "599 · 20m"),
    so leaving it unchanged is not a sign of an untranslated row.
    """
    stripped = english
    for token in required:
        stripped = stripped.replace(token, ' ')
    stripped = RE_PLACEHOLDER.sub(' ', stripped)
    stripped = RE_MARKER.sub(' ', stripped)
    stripped = RE_CWTOKEN.sub(' ', stripped)
    return not re.search(r'[A-Za-z]{2}', stripped)


def check_row(row, rep):
    key = row['key']
    english = row['english']
    pt = row['portuguese']
    dnt = [t.strip() for t in row['do_not_translate'].split('|') if t.strip()]

    if pt == '':
        return False                      # not translated yet: nothing to check, nothing wrong
    if pt.strip() == '':
        rep.error('a cell holds only spaces, which the program treats as untranslated',
                  key, 'Either write the Portuguese or clear the cell completely.')
        return False

    # 1. {{placeholders}} — same names, same number of each. Order may change.
    want, got = Counter(RE_PLACEHOLDER.findall(english)), Counter(RE_PLACEHOLDER.findall(pt))
    if want != got:
        missing = sorted((want - got).elements())
        extra = sorted((got - want).elements())
        bits = []
        if missing:
            bits.append('missing or renamed: ' + ', '.join('{{%s}}' % m for m in missing))
        if extra:
            bits.append('not in the English: ' + ', '.join('{{%s}}' % m for m in extra))
        rep.error('a {{slot}} was renamed, dropped or added', key,
                  'English:    %s\nPortuguese: %s\n%s' % (english, pt, '; '.join(bits)))

    # 1b. single braces where double braces were meant — renders as literal text
    singles = [s for s in RE_SINGLE_BRACE.findall(pt) if s in set(RE_PLACEHOLDER.findall(english))]
    if singles:
        rep.error('a slot was written with single braces instead of double', key,
                  'Portuguese: %s\nWrite {{%s}}, not {%s}.' % (pt, singles[0], singles[0]))

    # 2. <markers> — same tags, same count, and each one closed.
    mw, mg = markers(english), markers(pt)
    if mw != mg:
        missing = sorted((mw - mg).elements())
        extra = sorted((mg - mw).elements())
        bits = []
        if missing:
            bits.append('missing: ' + ' '.join(missing))
        if extra:
            bits.append('invented or duplicated: ' + ' '.join(extra))
        rep.error('a <marker> was changed', key,
                  'English:    %s\nPortuguese: %s\n%s\nOnly the marker names the English uses will '
                  'do anything; anything else is printed on screen as literal text.'
                  % (english, pt, '; '.join(bits)))
    stack = []
    for slash, name in RE_MARKER.findall(pt):
        if slash:
            if not stack or stack[-1] != name:
                rep.error('a <marker> is out of order or was never opened', key,
                          'Portuguese: %s\n</%s> closes a marker that is not open at that point. '
                          'Each <%s> is closed by the matching </%s>, in order.' % (pt, name, name, name))
                stack = []
                break
            stack.pop()
        else:
            stack.append(name)
    if stack:
        rep.error('a <marker> was left unclosed', key,
                  'Portuguese: %s\nEvery <%s> needs a matching </%s>.' % (pt, stack[0], stack[0]))

    # 3. {MACRO} tokens — single braces, all capitals. Literal, always.
    cw, cg = Counter(RE_CWTOKEN.findall(english)), Counter(RE_CWTOKEN.findall(pt))
    if cw != cg:
        rep.error('a CW macro token was changed', key,
                  'English:    %s\nPortuguese: %s\nThese are matched literally by the macro '
                  'expander. Copy them exactly.' % (english, pt))

    # 4. Ham vocabulary and other terms of art listed in do_not_translate.
    plain = [t for t in dnt if not (t.startswith('{') or t.startswith('<'))]
    absent = [t for t in plain if t not in pt]
    if absent:
        rep.error('a term that must be copied through is not in the Portuguese', key,
                  'English:    %s\nPortuguese: %s\nNot found: %s\nIf you believe one of these '
                  'really is said in Portuguese in Brazil, say so and I will change the list.'
                  % (english, pt, ', '.join(absent)))

    # 5. A decimal comma the English did not have. This is the operating fault.
    if RE_DECIMAL_COMMA.search(pt) and not RE_DECIMAL_COMMA.search(english):
        rep.error('a decimal comma appeared in a number', key,
                  'English:    %s\nPortuguese: %s\nAn operator reads these off the screen and '
                  'dials them into a radio. Technical numbers keep the dot.' % (english, pt))

    # 6. Copied English sitting in the Portuguese column.
    if pt.strip() == english.strip() and not is_all_invariant(english, plain):
        rep.warn('the Portuguese is identical to the English', key,
                 'Text: %s\nFine if that really is the Portuguese; otherwise this row is not done.'
                 % english)

    return True


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    path = argv[1]

    try:
        with open(path, encoding='utf-8-sig', newline='') as fh:
            reader = csv.DictReader(fh)
            header = reader.fieldnames
            rows = list(reader)
    except UnicodeDecodeError:
        print('PROBLEM: this file is not UTF-8. Re-save it as CSV UTF-8 and try again.')
        return 1
    except OSError as exc:
        print('PROBLEM: could not open %s (%s)' % (path, exc))
        return 1

    print('Nexus pt-BR translation check')
    print('file: %s' % path)

    if header != COLUMNS:
        print()
        print('PROBLEM: the columns are not the ones the kit shipped.')
        print('    expected: %s' % ', '.join(COLUMNS))
        print('    found:    %s' % ', '.join(header or []))
        print('Do not add, remove or reorder columns. Fix that and run this again.')
        return 1

    rep = Report()

    # ── the census: is every row still here, and untouched where it must be ──────────────
    if len(rows) != EXPECTED_ROWS:
        rep.error('the number of rows changed', '(whole file)',
                  'The kit shipped %d rows; this file has %d. %d row%s went missing — most likely '
                  'a sort, a filter left on when saving, or a deleted line.'
                  % (EXPECTED_ROWS, len(rows), abs(EXPECTED_ROWS - len(rows)),
                     '' if abs(EXPECTED_ROWS - len(rows)) == 1 else 's')
                  if len(rows) < EXPECTED_ROWS else
                  'The kit shipped %d rows; this file has %d. There are extra rows.'
                  % (EXPECTED_ROWS, len(rows)))

    dupes = [k for k, n in Counter(r['key'] for r in rows).items() if n > 1]
    if dupes:
        rep.error('the same key appears more than once', '(whole file)',
                  'Duplicated: %s' % ', '.join(sorted(dupes)[:10]))

    ordered = sorted(rows, key=lambda r: r['key'])
    keys_ok = sha('\n'.join(sorted(r['key'] for r in rows))) == EXPECTED_KEYS_SHA
    if not keys_ok:
        rep.error('the key column is not the one the kit shipped', '(whole file)',
                  'Some keys were changed, removed or added. The program matches rows by key, so '
                  'a changed key is a lost string. Start again from the original CSV and paste '
                  'your Portuguese column across.\n(The english and do_not_translate columns are '
                  'not checked while rows are missing — fix the rows first.)')
    if keys_ok and sha('\n'.join(r['key'] + '\t' + r['english'] for r in ordered)) != EXPECTED_ENGLISH_SHA:
        rep.error('the english column was edited', '(whole file)',
                  'The English text must stay exactly as shipped — it is what the Portuguese is '
                  'matched against. If some English is wrong, tell me rather than fixing it here.')
    if keys_ok and sha('\n'.join(r['key'] + '\t' + r['do_not_translate'] for r in ordered)) != EXPECTED_DNT_SHA:
        rep.error('the do_not_translate column was edited', '(whole file)',
                  'Leave that column alone. If a term on it should be translated after all, say '
                  'so and I will change the kit.')

    # ── per row ─────────────────────────────────────────────────────────────────────────
    filled = 0
    per_tier = Counter()
    for row in rows:
        if check_row(row, rep):
            filled += 1
            try:
                per_tier[int(row['priority'])] += 1
            except (ValueError, KeyError):
                pass

    # ── plural pairs travel together ────────────────────────────────────────────────────
    plural = defaultdict(dict)
    for row in rows:
        if '::' in row['key']:
            base, form = row['key'].rsplit('::', 1)
            plural[base][form] = row['portuguese']
    for base, forms in plural.items():
        done = [f for f, v in forms.items() if v.strip()]
        if done and len(done) != len(forms):
            rep.warn('only one half of a plural pair is filled in', base,
                     'Filled: %s. Still empty: %s. Both forms are needed, or the count will read '
                     'wrong for one of them.'
                     % (', '.join(sorted(done)),
                        ', '.join(sorted(f for f in forms if f not in done))))
        elif len(done) == len(forms) and len(set(forms.values())) == 1 and len(forms) > 1:
            rep.warn('both plural forms are the same sentence', base,
                     'Text: %s\nThat is correct in some languages. In Portuguese the singular and '
                     'plural usually differ — worth a second look.' % list(forms.values())[0])

    # A translator who pastes the English column across would otherwise get warnings only.
    copied = len(rep.warnings.get('the Portuguese is identical to the English', ()))
    if filled >= 20 and copied * 5 > filled:
        rep.error('most of the Portuguese column is still English', '(whole file)',
                  '%d of the %d filled rows are word-for-word the English. That is what a copied '
                  'column looks like, not a translation. If it was deliberate, ignore this; if the '
                  'column got pasted by accident, start again from the original CSV.'
                  % (copied, filled))

    rep.render()

    # ── summary ─────────────────────────────────────────────────────────────────────────
    print()
    print('-' * 72)
    print('Rows in file:      %d   (the kit shipped %d)' % (len(rows), EXPECTED_ROWS))
    pct = (100.0 * filled / len(rows)) if rows else 0.0
    print('Rows translated:   %d   (%.1f%%)' % (filled, pct))
    for tier in sorted(EXPECTED_TIERS):
        total = EXPECTED_TIERS[tier]
        got = per_tier.get(tier, 0)
        bar = '#' * int(round(20.0 * got / total)) if total else ''
        print('    tier %d: %5d of %5d  %-20s %5.1f%%'
              % (tier, got, total, bar, 100.0 * got / total if total else 0.0))
    print('-' * 72)

    if rep.n_errors():
        print()
        print('%d problem%s to fix. Nothing here is about your Portuguese — every one is a token, '
              'a marker or a row that moved.' % (rep.n_errors(), '' if rep.n_errors() == 1 else 's'))
        if rep.n_warnings():
            print('%d more thing%s worth a look.'
                  % (rep.n_warnings(), '' if rep.n_warnings() == 1 else 's'))
        return 1

    if rep.n_warnings():
        print()
        print('No problems. %d thing%s worth a look above — none of them will break anything.'
              % (rep.n_warnings(), '' if rep.n_warnings() == 1 else 's'))
        return 0

    print()
    print('No problems found. Every slot, marker and technical term survived. Thank you — send it '
          'to kd9taw@protonmail.com whenever you are ready.')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))

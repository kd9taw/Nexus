#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
verify-ru.py — check a filled-in Nexus Russian translation CSV.

    python3 verify-ru.py nexus-ru-translation.csv

It checks the MECHANICAL things only: that every row is still there, that the English and
do-not-translate columns were not edited, and that nothing which must survive a translation
byte-for-byte got renamed, dropped, added, or re-punctuated. It says nothing about whether the
Russian is good — that is your job and it cannot be checked by a program.

Python 3 only. No installation, no dependencies.

Exit code 0 = nothing broken (warnings may still be printed). Exit code 1 = something to fix.
"""

import csv
import hashlib
import re
import sys
from collections import Counter, defaultdict

sys.stdout.reconfigure(encoding='utf-8')

# ── What the kit shipped, so a missing row can be detected without a second file ───────────
# make-kit.py rewrites the lines between the two markers every time it builds the CSV, from the
# file it just wrote. Do not edit them by hand: a pin that disagrees with the CSV beside it
# reports a translator's untouched file as damaged.
# >>> make-kit pins
EXPECTED_ROWS = 10136
EXPECTED_KEYS_SHA = '01782eda20f1a14c1383e96e00e2f1d80076dec4e8aae17ccc8a3011c82a2a8c'
EXPECTED_ENGLISH_SHA = 'a292845ef65149ec2e0a897a5cdd2f62cb355d2960c53373fcf988da4797981f'
EXPECTED_DNT_SHA = 'dcce04bd0c1664975d833cacba7277c4a4c3fd1ddc26ca3c50a2effc079705d4'
EXPECTED_TIERS = {1: 983, 2: 349, 3: 1897, 4: 560, 5: 4013, 6: 2334}
# <<< make-kit pins
COLUMNS = ['priority', 'key', 'english', 'russian', 'do_not_translate', 'notes']

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


def pins(rows):
    """What a CSV's untranslatable columns add up to. make-kit.py writes these as the EXPECTED_*
    constants above, and this file compares a returned CSV against them — one formula, used by
    both, so the two cannot drift apart."""
    ordered = sorted(rows, key=lambda r: r['key'])
    tiers = Counter()
    for r in rows:
        try:
            tiers[int(r['priority'])] += 1
        except (ValueError, KeyError):
            pass
    return {
        'rows': len(rows),
        'keys': sha('\n'.join(sorted(r['key'] for r in rows))),
        'english': sha('\n'.join(r['key'] + '\t' + r['english'] for r in ordered)),
        'dnt': sha('\n'.join(r['key'] + '\t' + r['do_not_translate'] for r in ordered)),
        'tiers': dict(sorted(tiers.items())),
    }


def markers(text):
    """Counter of marker tokens, e.g. {'<b>': 2, '</b>': 2}."""
    return Counter('<%s%s>' % (slash, name) for slash, name in RE_MARKER.findall(text))


def is_all_invariant(english, required):
    """True when the English is nothing but tokens, numbers and punctuation.

    Such a row is legitimately identical in Russian ("FT8", "{{call}}", "599 · 20m"),
    so leaving it unchanged is not a sign of an untranslated row.
    """
    stripped = english
    for token in required:
        stripped = stripped.replace(token, ' ')
    stripped = RE_PLACEHOLDER.sub(' ', stripped)
    stripped = RE_MARKER.sub(' ', stripped)
    stripped = RE_CWTOKEN.sub(' ', stripped)
    return not re.search(r'[A-Za-z]{2}', stripped)


def check_row(row, rep, one_needs_count=False):
    key = row['key']
    english = row['english']
    it = row['russian']
    dnt = [t.strip() for t in row['do_not_translate'].split('|') if t.strip()]

    if it == '':
        return False                      # not translated yet: nothing to check, nothing wrong
    if it.strip() == '':
        rep.error('a cell holds only spaces, which the program treats as untranslated',
                  key, 'Either write the Russian or clear the cell completely.')
        return False

    # 1. {{placeholders}} — same names, same number of each. Order may change.
    want, got = Counter(RE_PLACEHOLDER.findall(english)), Counter(RE_PLACEHOLDER.findall(it))
    # Russian `::one` also fires on 21, 31, 101…, so where the English plural shows the count at
    # all, the Russian `::one` row must show it too, even when the English `::one` writes "One".
    if one_needs_count and 'count' not in want:
        want['count'] = 1
    if want != got:
        missing = sorted((want - got).elements())
        extra = sorted((got - want).elements())
        bits = []
        if missing:
            bits.append('missing or renamed: ' + ', '.join('{{%s}}' % m for m in missing))
        if extra:
            bits.append('not in the English: ' + ', '.join('{{%s}}' % m for m in extra))
        rep.error('a {{slot}} was renamed, dropped or added', key,
                  'English: %s\nRussian: %s\n%s' % (english, it, '; '.join(bits)))

    # 1b. single braces where double braces were meant — renders as literal text
    singles = [s for s in RE_SINGLE_BRACE.findall(it) if s in set(RE_PLACEHOLDER.findall(english))]
    if singles:
        rep.error('a slot was written with single braces instead of double', key,
                  'Russian: %s\nWrite {{%s}}, not {%s}.' % (it, singles[0], singles[0]))

    # 2. <markers> — same tags, same count, and each one closed.
    mw, mg = markers(english), markers(it)
    if mw != mg:
        missing = sorted((mw - mg).elements())
        extra = sorted((mg - mw).elements())
        bits = []
        if missing:
            bits.append('missing: ' + ' '.join(missing))
        if extra:
            bits.append('invented or duplicated: ' + ' '.join(extra))
        rep.error('a <marker> was changed', key,
                  'English: %s\nRussian: %s\n%s\nOnly the marker names the English uses will '
                  'do anything; anything else is printed on screen as literal text.'
                  % (english, it, '; '.join(bits)))
    stack = []
    for slash, name in RE_MARKER.findall(it):
        if slash:
            if not stack or stack[-1] != name:
                rep.error('a <marker> is out of order or was never opened', key,
                          'Russian: %s\n</%s> closes a marker that is not open at that point. '
                          'Each <%s> is closed by the matching </%s>, in order.' % (it, name, name, name))
                stack = []
                break
            stack.pop()
        else:
            stack.append(name)
    if stack:
        rep.error('a <marker> was left unclosed', key,
                  'Russian: %s\nEvery <%s> needs a matching </%s>.' % (it, stack[0], stack[0]))

    # 3. {MACRO} tokens — single braces, all capitals. Literal, always.
    cw, cg = Counter(RE_CWTOKEN.findall(english)), Counter(RE_CWTOKEN.findall(it))
    if cw != cg:
        rep.error('a CW macro token was changed', key,
                  'English: %s\nRussian: %s\nThese are matched literally by the macro '
                  'expander. Copy them exactly.' % (english, it))

    # 4. Ham vocabulary and other terms of art listed in do_not_translate.
    plain = [t for t in dnt if not (t.startswith('{') or t.startswith('<'))]
    absent = [t for t in plain if t not in it]
    if absent:
        rep.error('a term that must be copied through is not in the Russian', key,
                  'English: %s\nRussian: %s\nNot found: %s\nIf you believe one of these '
                  'really is said in Russian on the air, say so and I will change the list.'
                  % (english, it, ', '.join(absent)))

    # 5. A decimal comma the English did not have. This is the operating fault.
    if RE_DECIMAL_COMMA.search(it) and not RE_DECIMAL_COMMA.search(english):
        rep.error('a decimal comma appeared in a number', key,
                  'English: %s\nRussian: %s\nAn operator reads these off the screen and '
                  'dials them into a radio. Technical numbers keep the dot.' % (english, it))

    # 6. Copied English sitting in the Russian column.
    if it.strip() == english.strip() and not is_all_invariant(english, plain):
        rep.warn('the Russian is identical to the English', key,
                 'Text: %s\nFine if that really is the Russian; otherwise this row is not done.'
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

    print('Nexus Russian translation check')
    print('file: %s' % path)

    if header != COLUMNS:
        print()
        print('PROBLEM: the columns are not the ones the kit shipped.')
        print('    expected: %s' % ', '.join(COLUMNS))
        print('    found:    %s' % ', '.join(header or []))
        print('Do not add, remove or reorder columns. Fix that and run this again.')
        return 1

    rep = Report()
    got = pins(rows)

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

    keys_ok = got['keys'] == EXPECTED_KEYS_SHA
    if not keys_ok:
        rep.error('the key column is not the one the kit shipped', '(whole file)',
                  'Some keys were changed, removed or added. The program matches rows by key, so '
                  'a changed key is a lost string. Start again from the original CSV and paste '
                  'your Russian column across.\n(The english and do_not_translate columns are '
                  'not checked while rows are missing — fix the rows first.)')
    if keys_ok and got['english'] != EXPECTED_ENGLISH_SHA:
        rep.error('the english column was edited', '(whole file)',
                  'The English text must stay exactly as shipped — it is what the Russian is '
                  'matched against. If some English is wrong, tell me rather than fixing it here.')
    if keys_ok and got['dnt'] != EXPECTED_DNT_SHA:
        rep.error('the do_not_translate column was edited', '(whole file)',
                  'Leave that column alone. If a term on it should be translated after all, say '
                  'so and I will change the kit.')

    # ── per row ─────────────────────────────────────────────────────────────────────────
    filled = 0
    per_tier = Counter()
    # Plural bases whose English `other` form shows {{count}}: their Russian `::one` must too.
    count_bases = {r['key'].rsplit('::', 1)[0] for r in rows
                   if r['key'].endswith('::other') and '{{count}}' in r['english']}
    for row in rows:
        one_needs_count = (row['key'].endswith('::one')
                           and row['key'].rsplit('::', 1)[0] in count_bases)
        if check_row(row, rep, one_needs_count):
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
            plural[base][form] = row['russian']
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
                     'Text: %s\nThat is correct in some languages. In Russian the singular and '
                     'plural usually differ — worth a second look.' % list(forms.values())[0])

    # A translator who pastes the English column across would otherwise get warnings only.
    copied = len(rep.warnings.get('the Russian is identical to the English', ()))
    if filled >= 20 and copied * 5 > filled:
        rep.error('most of the Russian column is still English', '(whole file)',
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
        n = per_tier.get(tier, 0)
        bar = '#' * int(round(20.0 * n / total)) if total else ''
        print('    tier %d: %5d of %5d  %-20s %5.1f%%'
              % (tier, n, total, bar, 100.0 * n / total if total else 0.0))
    print('-' * 72)

    if rep.n_errors():
        print()
        print('%d problem%s to fix. Nothing here is about your Russian — every one is a token, '
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
          'to contact@hamradiotools.io whenever you are ready.')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))

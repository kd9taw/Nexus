// Tests for scripts/check-macho-bundle.mjs, the check release.yml and ci.yml run on every Mach-O
// a macOS build ships.
//
// The fixtures are real Mach-O headers and load commands, written here byte by byte in the layout
// <mach-o/loader.h> defines, so the check reads them exactly as it reads a shipped file. Every
// refusal is paired with the clean file it was planted in, which passes: a check that refused
// everything would fail the controls, and one that refused nothing would fail the plants.
//
// Run: node --test scripts/check-macho-bundle.test.mjs

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { readMachO, checkSlices, parseVersion, plistMinimum } from './check-macho-bundle.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'check-macho-bundle.mjs');
const CPU = { arm64: 0x0100000c, x86_64: 0x01000007 };
const LIBSYSTEM = '/usr/lib/libSystem.B.dylib';
const ver = (s) => {
  const [a, b, c] = parseVersion(s);
  return (a << 16) | (b << 8) | c;
};

// One load command that carries a string (dylib_command / rpath_command / dylinker_command).
function strCmd(cmd, s, headerLen) {
  const body = Buffer.from(`${s}\0`, 'utf8');
  const size = Math.ceil((headerLen + body.length) / 8) * 8;
  const b = Buffer.alloc(size);
  b.writeUInt32LE(cmd, 0);
  b.writeUInt32LE(size, 4);
  b.writeUInt32LE(headerLen, 8); // lc_str offset
  body.copy(b, headerLen);
  return b;
}
const dylib = (s, cmd = 0x0c) => strCmd(cmd, s, 24); // name, timestamp, current, compat
const rpath = (s) => strCmd(0x8000001c, s, 12);
function buildVersion(minos, platform = 1) {
  const b = Buffer.alloc(24);
  b.writeUInt32LE(0x32, 0);
  b.writeUInt32LE(24, 4);
  b.writeUInt32LE(platform, 8);
  b.writeUInt32LE(ver(minos), 12);
  b.writeUInt32LE(ver('15.0'), 16); // sdk
  b.writeUInt32LE(0, 20); // ntools
  return b;
}
function versionMin(minos) {
  const b = Buffer.alloc(16);
  b.writeUInt32LE(0x24, 0);
  b.writeUInt32LE(16, 4);
  b.writeUInt32LE(ver(minos), 8);
  b.writeUInt32LE(ver('15.0'), 12);
  return b;
}

/** A thin 64-bit little-endian Mach-O with the given load commands. */
function macho({ arch = 'arm64', minos = '12.0', loads = [LIBSYSTEM, '@rpath/libhamlib.4.dylib'], rpaths = ['@loader_path'], id = null, weak = [], legacyMin = false, extra = [], dylibType = false } = {}) {
  const cmds = [
    ...(id ? [dylib(id, 0x0d)] : []),
    ...(minos === null ? [] : [legacyMin ? versionMin(minos) : buildVersion(minos)]),
    ...loads.map((l) => dylib(l)),
    ...weak.map((l) => dylib(l, 0x80000018)),
    ...rpaths.map(rpath),
    ...extra,
  ];
  const body = Buffer.concat(cmds);
  const h = Buffer.alloc(32);
  h.writeUInt32LE(0xfeedfacf, 0);
  h.writeUInt32LE(CPU[arch], 4);
  h.writeUInt32LE(0, 8);
  h.writeUInt32LE(dylibType ? 6 : 2, 12);
  h.writeUInt32LE(cmds.length, 16);
  h.writeUInt32LE(body.length, 20);
  return Buffer.concat([h, body, Buffer.alloc(64)]);
}

/** A universal file holding the given thin images. */
function fat(images) {
  const align = 4096;
  const head = Buffer.alloc(8 + images.length * 20);
  head.writeUInt32BE(0xcafebabe, 0);
  head.writeUInt32BE(images.length, 4);
  const parts = [];
  let at = align;
  images.forEach((img, i) => {
    const o = 8 + i * 20;
    head.writeUInt32BE(img.readUInt32LE(4), o); // cputype
    head.writeUInt32BE(0, o + 4);
    head.writeUInt32BE(at, o + 8);
    head.writeUInt32BE(img.length, o + 12);
    head.writeUInt32BE(12, o + 16);
    parts.push({ at, img });
    at = Math.ceil((at + img.length) / align) * align;
  });
  const out = Buffer.alloc(at);
  head.copy(out, 0);
  for (const p of parts) p.img.copy(out, p.at);
  return out;
}

const PLIST = (v) => `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>Nexus</string>
${v === null ? '' : `  <key>LSMinimumSystemVersion</key>\n  <string>${v}</string>\n`}</dict>
</plist>
`;

/** A scratch .app laid out as the bundler lays Nexus out. Overrides replace or add files. */
function app(over = {}, { arch = 'arm64', plist = '12.0' } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'macho-check-'));
  const a = path.join(root, 'Nexus.app');
  const hl = 'Contents/Resources/resources/hamlib';
  const files = {
    'Contents/MacOS/Nexus': macho({ arch, loads: [LIBSYSTEM, '/usr/lib/libc++.1.dylib', '/System/Library/Frameworks/WebKit.framework/Versions/A/WebKit'], rpaths: [] }),
    [`${hl}/rigctld`]: macho({ arch }),
    [`${hl}/rigctl`]: macho({ arch }),
    [`${hl}/libhamlib.4.dylib`]: macho({ arch, id: '@rpath/libhamlib.4.dylib', loads: [LIBSYSTEM, '@rpath/libusb-1.0.0.dylib'], rpaths: [], dylibType: true }),
    [`${hl}/libusb-1.0.0.dylib`]: macho({ arch, id: '@rpath/libusb-1.0.0.dylib', loads: [LIBSYSTEM, '/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit'], rpaths: [], dylibType: true }),
    [`${hl}/COPYING.txt`]: Buffer.from('GNU GENERAL PUBLIC LICENSE\n'),
    'Contents/Resources/resources/deepcw/model.onnx': Buffer.from([0x08, 0x07, 0x12, 0x00]),
    ...(plist === undefined ? {} : { 'Contents/Info.plist': Buffer.from(PLIST(plist)) }),
    ...over,
  };
  for (const [rel, data] of Object.entries(files)) {
    if (data === null) continue;
    fs.mkdirSync(path.dirname(path.join(a, rel)), { recursive: true });
    fs.writeFileSync(path.join(a, rel), data);
  }
  return a;
}

function run(args) {
  const r = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
}
const full = (a, arch = 'arm64', extra = []) => run(['--floor', '12.0', '--arch', arch, '--plist', path.join(a, 'Contents/Info.plist'), '--require', 'Contents/MacOS/Nexus', ...extra, a]);
const HL = 'Contents/Resources/resources/hamlib';

test('a clean Apple Silicon bundle passes, and every Mach-O in it was read', () => {
  const r = full(app());
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /5 file\(s\), arm64/);
});

test('a clean Intel bundle passes as x86_64 and fails as arm64', () => {
  const a = app({}, { arch: 'x86_64' });
  assert.equal(full(a, 'x86_64').code, 0);
  const wrong = full(a, 'arm64');
  assert.equal(wrong.code, 1);
  assert.match(wrong.out, /architectures x86_64, expected arm64/);
});

test('a library loaded from /opt/local (MacPorts) fails, and names the file and the path', () => {
  const r = full(app({ [`${HL}/rigctl`]: macho({ loads: [LIBSYSTEM, '@rpath/libhamlib.4.dylib', '/opt/local/lib/libreadline.8.dylib'] }) }));
  assert.equal(r.code, 1);
  assert.match(r.out, /hamlib\/rigctl: loads \/opt\/local\/lib\/libreadline\.8\.dylib/);
  assert.match(r.out, /under \/opt\/local/);
});

test('a library loaded from /opt/homebrew fails (the 1.10.0 libusb class), weak loads included', () => {
  const strong = full(app({ [`${HL}/libhamlib.4.dylib`]: macho({ id: '@rpath/libhamlib.4.dylib', loads: [LIBSYSTEM, '/opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib'], rpaths: [], dylibType: true }) }));
  assert.equal(strong.code, 1);
  assert.match(strong.out, /libhamlib\.4\.dylib: loads \/opt\/homebrew\/opt\/libusb/);
  const weak = full(app({ [`${HL}/rigctld`]: macho({ weak: ['/usr/local/lib/libfoo.dylib'] }) }));
  assert.equal(weak.code, 1);
  assert.match(weak.out, /rigctld: loads \/usr\/local\/lib\/libfoo\.dylib/);
});

test('a run path or an install name under a build-machine prefix fails', () => {
  const rp = full(app({ [`${HL}/rigctld`]: macho({ rpaths: ['@loader_path', '/opt/homebrew/lib'] }) }));
  assert.equal(rp.code, 1);
  assert.match(rp.out, /searches run path \/opt\/homebrew\/lib/);
  const id = full(app({ [`${HL}/libusb-1.0.0.dylib`]: macho({ id: '/usr/local/lib/libusb-1.0.0.dylib', loads: [LIBSYSTEM], rpaths: [], dylibType: true }) }));
  assert.equal(id.code, 1);
  assert.match(id.out, /names \/usr\/local\/lib\/libusb-1\.0\.0\.dylib, under \/usr\/local/);
});

test('an absolute path outside the system, such as the build prefix, fails too', () => {
  const r = full(app({ [`${HL}/rigctld`]: macho({ loads: [LIBSYSTEM, '/private/var/folders/xy/T/tmp.1/prefix/lib/libusb-1.0.0.dylib'] }) }));
  assert.equal(r.code, 1);
  assert.match(r.out, /neither bundled .* nor a system library/);
});

test('a Mach-O built for macOS 14 fails against a minimum of 12 (the 1.21.0 Hamlib class)', () => {
  const r = full(app({ [`${HL}/libusb-1.0.0.dylib`]: macho({ minos: '14.0', id: '@rpath/libusb-1.0.0.dylib', loads: [LIBSYSTEM], rpaths: [], dylibType: true }) }));
  assert.equal(r.code, 1);
  assert.match(r.out, /libusb-1\.0\.0\.dylib: needs macOS 14\.0, newer than the app's minimum 12\.0/);
});

test('the minimum is a ceiling, not an exact match: 11.0 and the older LC_VERSION_MIN_MACOSX pass', () => {
  assert.equal(full(app({ 'Contents/MacOS/Nexus': macho({ minos: '11.0', rpaths: [] }) })).code, 0);
  assert.equal(full(app({ [`${HL}/rigctl`]: macho({ minos: '10.13', legacyMin: true }) })).code, 0);
  const newer = full(app({ [`${HL}/rigctl`]: macho({ minos: '12.1', legacyMin: true }) }));
  assert.equal(newer.code, 1);
  assert.match(newer.out, /needs macOS 12\.1/);
});

test('positive controls: no minimum recorded, no libSystem, or a non-macOS platform each fail', () => {
  const nomin = full(app({ [`${HL}/rigctld`]: macho({ minos: null }) }));
  assert.equal(nomin.code, 1);
  assert.match(nomin.out, /records no minimum macOS/);
  const nosys = full(app({ [`${HL}/rigctld`]: macho({ loads: ['@rpath/libhamlib.4.dylib'] }) }));
  assert.equal(nosys.code, 1);
  assert.match(nosys.out, /does not load \/usr\/lib\/libSystem\.B\.dylib/);
  const ios = macho();
  ios.writeUInt32LE(2, 32 + 8); // the build_version_command is first: platform iOS
  const r = full(app({ [`${HL}/rigctld`]: ios }));
  assert.equal(r.code, 1);
  assert.match(r.out, /platform 2, not macOS/);
});

test('a folder with no Mach-O in it fails rather than passing on nothing read', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'macho-empty-'));
  fs.writeFileSync(path.join(root, 'COPYING.txt'), 'text');
  const r = run(['--floor', '12.0', '--arch', 'arm64', root]);
  assert.equal(r.code, 1);
  assert.match(r.out, /no Mach-O file found/);
});

test('a required file that is missing, or is not a Mach-O, fails', () => {
  const r = full(app({ 'Contents/MacOS/Nexus': null }));
  assert.equal(r.code, 1);
  assert.match(r.out, /Contents\/MacOS\/Nexus: required, and not among the Mach-O files read/);
  const text = full(app({ 'Contents/MacOS/Nexus': Buffer.from('#!/bin/sh\nexit 0\n') }));
  assert.equal(text.code, 1);
  assert.match(text.out, /required/);
});

test('Info.plist must set LSMinimumSystemVersion to exactly the minimum', () => {
  const def = full(app({}, { plist: '10.13' }));
  assert.equal(def.code, 1);
  assert.match(def.out, /LSMinimumSystemVersion is 10\.13, not the app's minimum 12\.0/);
  const none = full(app({}, { plist: null }));
  assert.equal(none.code, 1);
  assert.match(none.out, /sets no LSMinimumSystemVersion/);
  const bin = full(app({ 'Contents/Info.plist': Buffer.from('bplist00\x00\x01') }));
  assert.equal(bin.code, 1);
  assert.match(bin.out, /binary plist/);
  assert.equal(plistMinimum(PLIST('12.0')), '12.0');
});

test('a truncated or malformed Mach-O fails instead of being skipped', () => {
  const good = macho();
  const cut = good.subarray(0, 40);
  const r = full(app({ [`${HL}/rigctld`]: cut }));
  assert.equal(r.code, 1);
  assert.match(r.out, /rigctld: unreadable Mach-O/);
  const bad = Buffer.from(good);
  bad.writeUInt32LE(4, 32 + 4); // first command's size below the 8-byte minimum
  const r2 = full(app({ [`${HL}/rigctld`]: bad }));
  assert.equal(r2.code, 1);
  assert.match(r2.out, /bad size/);
});

test('a universal file must hold exactly the expected slices, and each slice is checked', () => {
  const both = fat([macho({ arch: 'arm64' }), macho({ arch: 'x86_64' })]);
  assert.deepEqual(readMachO(both).map((s) => s.arch), ['arm64', 'x86_64']);
  assert.equal(full(app({ [`${HL}/rigctld`]: both }), 'arm64', ['--arch', 'x86_64']).code, 1, 'other files are thin arm64 only');
  const one = full(app({ [`${HL}/rigctld`]: both }), 'arm64');
  assert.equal(one.code, 1);
  assert.match(one.out, /architectures arm64\+x86_64, expected arm64/);
  const badSlice = fat([macho({ arch: 'arm64' }), macho({ arch: 'x86_64', minos: '15.0' })]);
  const v = checkSlices(readMachO(badSlice), { floor: [12, 0, 0], arches: ['arm64', 'x86_64'] });
  assert.deepEqual(v, ["[x86_64] needs macOS 15.0, newer than the app's minimum 12.0"]);
});

test('a Java class file (same magic as a universal file) is not mistaken for one', () => {
  const cls = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x41, 0, 0, 0, 0]);
  assert.equal(readMachO(cls), null);
  assert.equal(full(app({ 'Contents/Resources/x.class': cls })).code, 0);
});

test('symbolic links are not followed, so a link out of the bundle reads nothing', () => {
  const a = app();
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'macho-outside-'));
  fs.writeFileSync(path.join(outside, 'bad'), macho({ minos: '15.0' }));
  fs.symlinkSync(path.join(outside, 'bad'), path.join(a, HL, 'linked'));
  assert.equal(full(a).code, 0);
});

test('usage errors exit 2, never 0', () => {
  const a = app();
  assert.equal(run(['--arch', 'arm64', a]).code, 2);
  assert.equal(run(['--floor', 'twelve', '--arch', 'arm64', a]).code, 2);
  assert.equal(run(['--floor', '12.0', a]).code, 2);
  assert.equal(run(['--floor', '12.0', '--arch', 'ppc', a]).code, 2);
  assert.equal(run(['--floor', '12.0', '--arch', 'arm64']).code, 2);
  assert.equal(run(['--floor', '12.0', '--arch', 'arm64', '--bogus', a]).code, 2);
});

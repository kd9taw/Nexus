#!/usr/bin/env node
// Check every Mach-O in a macOS build against what an operator's Mac will have.
//
// WHY. A Mach-O names the libraries it loads by path and records the oldest macOS it runs on
// (its `minos`). Both are decided on the build machine, and both pass every test there:
//   - A path under /opt/homebrew, /usr/local or /opt/local resolves on a runner that has
//     Homebrew or MacPorts, and is missing on a Mac that does not. dyld then stops the program
//     before main. That is how the 1.10.0 CAT regression shipped (libhamlib named Homebrew's
//     libusb), and how a community Intel build ended up needing MacPorts' readline.
//   - A minos above the macOS the app promises means dyld refuses the file on an older Mac. The
//     bundled Hamlib tools of 1.21.0 were built for 14.0 because the build never set a
//     deployment target, while the app said 10.13 and the README said 12.
// The runner can test neither, because it has the paths and the newest macOS. So this reads the
// load commands straight out of the files, the same bytes dyld reads, and needs no Apple tool:
// it runs on Linux too, which is where its own tests run.
//
// WHAT IT CHECKS, for every Mach-O under the paths given (files, or folders searched whole;
// symbolic links are not followed) and every architecture slice in it:
//   1. The slice set is exactly the one expected (--arch arm64 | x86_64, repeatable for a
//      universal file).
//   2. The slice records a minimum macOS (LC_BUILD_VERSION, or LC_VERSION_MIN_MACOSX on an older
//      file), for macOS, no newer than --floor.
//   3. Every library it loads (LC_LOAD_DYLIB, LC_LOAD_WEAK_DYLIB, LC_REEXPORT_DYLIB,
//      LC_LAZY_LOAD_DYLIB, LC_LOAD_UPWARD_DYLIB) is @rpath/, @loader_path/, @executable_path/,
//      or a system library under /usr/lib/ or /System/.
//   4. Every run path (LC_RPATH) is @loader_path or @executable_path relative, or a system one.
//   5. No load command at all, its own install name (LC_ID_DYLIB) included, names /opt/homebrew,
//      /usr/local or /opt/local.
//   6. POSITIVE CONTROLS, so a reader that reads nothing cannot pass: each slice loads
//      /usr/lib/libSystem.B.dylib (every real program and library does), each slice has a
//      minimum macOS (rule 2), at least one Mach-O is found, and each --require path is one of
//      the Mach-Os read.
// And with --plist, the app's Info.plist sets LSMinimumSystemVersion to exactly --floor, which is
// what Finder and the App Store read before the app is ever started.
//
// WHAT IT CANNOT CHECK: objects linked in from a static archive (libgfortran.a and the like)
// leave no minos of their own in the linked file; the linker warns about them at build time, and
// release.yml reports those warnings. Whether dyld on a real older Mac then agrees is a bench
// question.
//
// Usage:
//   node scripts/check-macho-bundle.mjs --floor 12.0 --arch arm64 [--plist <Info.plist>]
//        [--require <path relative to the first path>]... <file-or-folder>...
// Exit 0 clean, 1 on any finding, 2 on a usage error.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const FORBIDDEN = ['/opt/homebrew', '/usr/local', '/opt/local'];
const SYSTEM = /^\/usr\/lib\/|^\/System\//;
const LOAD_OK = /^@rpath\/|^@loader_path\/|^@executable_path\/|^\/usr\/lib\/|^\/System\//;
const RPATH_OK = /^@loader_path(?:\/|$)|^@executable_path(?:\/|$)|^\/usr\/lib(?:\/|$)|^\/System\//;
const LIBSYSTEM = '/usr/lib/libSystem.B.dylib';

export const CPU = { 0x01000007: 'x86_64', 0x0100000c: 'arm64', 7: 'i386', 12: 'arm' };

const LC_REQ_DYLD = 0x80000000;
const LC = {
  LOAD_DYLIB: 0x0c,
  ID_DYLIB: 0x0d,
  // `>>> 0`: JavaScript's `|` yields a signed 32-bit number, and the file's are unsigned.
  LOAD_WEAK_DYLIB: (0x18 | LC_REQ_DYLD) >>> 0,
  REEXPORT_DYLIB: (0x1f | LC_REQ_DYLD) >>> 0,
  LAZY_LOAD_DYLIB: 0x20,
  LOAD_UPWARD_DYLIB: (0x23 | LC_REQ_DYLD) >>> 0,
  RPATH: (0x1c | LC_REQ_DYLD) >>> 0,
  DYLD_ENVIRONMENT: 0x27,
  VERSION_MIN_MACOSX: 0x24,
  BUILD_VERSION: 0x32,
};
const LOADS = new Set([LC.LOAD_DYLIB, LC.LOAD_WEAK_DYLIB, LC.REEXPORT_DYLIB, LC.LAZY_LOAD_DYLIB, LC.LOAD_UPWARD_DYLIB]);
const PLATFORM_MACOS = 1;

const MH_MAGIC = 0xfeedface;
const MH_MAGIC_64 = 0xfeedfacf;
const MH_CIGAM = 0xcefaedfe;
const MH_CIGAM_64 = 0xcffaedfe;
const FAT_MAGIC = 0xcafebabe;
const FAT_MAGIC_64 = 0xcafebabf;

/** "12.0" or "12.0.1" → [12, 0, 0] / [12, 0, 1]; null when it is not a version. */
export function parseVersion(s) {
  const m = /^(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(String(s).trim());
  return m ? [Number(m[1]), Number(m[2] ?? 0), Number(m[3] ?? 0)] : null;
}
/** The xxxx.yy.zz nibble encoding Mach-O uses for versions. */
export function decodeVersion(v) {
  return [v >>> 16, (v >>> 8) & 0xff, v & 0xff];
}
export const formatVersion = (v) => `${v[0]}.${v[1]}${v[2] ? `.${v[2]}` : ''}`;
const cmpVersion = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

/** The first four bytes say whether a file is Mach-O at all, and how to read it. */
function kind(buf) {
  if (buf.length < 8) return null;
  const be = buf.readUInt32BE(0);
  if (be === FAT_MAGIC || be === FAT_MAGIC_64) {
    // Java class files share 0xcafebabe; their next field is a version, far above any real
    // count of architectures. file(1) draws the line the same way.
    const n = buf.readUInt32BE(4);
    return n > 0 && n < 32 ? { fat: true, wide: be === FAT_MAGIC_64 } : null;
  }
  if ([MH_MAGIC, MH_MAGIC_64, MH_CIGAM, MH_CIGAM_64].includes(be)) return { fat: false };
  return null;
}

/** Read one thin Mach-O image at `off` in `buf`. Throws on anything malformed. */
export function readSlice(buf, off = 0, size = buf.length - off) {
  if (off + 28 > buf.length) throw new Error('truncated header');
  const magicBE = buf.readUInt32BE(off);
  const le = magicBE === MH_CIGAM || magicBE === MH_CIGAM_64;
  const wide = magicBE === MH_MAGIC_64 || magicBE === MH_CIGAM_64;
  if (![MH_MAGIC, MH_MAGIC_64, MH_CIGAM, MH_CIGAM_64].includes(magicBE)) throw new Error('not a Mach-O slice');
  const u32 = (at) => (le ? buf.readUInt32LE(at) : buf.readUInt32BE(at));
  const cputype = u32(off + 4);
  const ncmds = u32(off + 16);
  const sizeofcmds = u32(off + 20);
  const hdr = wide ? 32 : 28;
  const end = off + hdr + sizeofcmds;
  if (end > off + size || end > buf.length) throw new Error('load commands run past the end of the file');
  const out = { arch: CPU[cputype] ?? `cputype 0x${cputype.toString(16)}`, loads: [], ids: [], rpaths: [], env: [], minos: null, platform: null };
  const str = (cmdOff, cmdSize) => {
    const at = u32(cmdOff + 8);
    if (at < 12 || at >= cmdSize) throw new Error('a load command names a string outside itself');
    const s = buf.subarray(cmdOff + at, cmdOff + cmdSize);
    const nul = s.indexOf(0);
    return s.subarray(0, nul < 0 ? s.length : nul).toString('utf8');
  };
  let p = off + hdr;
  for (let i = 0; i < ncmds; i++) {
    if (p + 8 > end) throw new Error(`load command ${i} runs past the load command area`);
    const cmd = u32(p);
    const cmdsize = u32(p + 4);
    if (cmdsize < 8 || p + cmdsize > end) throw new Error(`load command ${i} has a bad size (${cmdsize})`);
    if (LOADS.has(cmd)) out.loads.push(str(p, cmdsize));
    else if (cmd === LC.ID_DYLIB) out.ids.push(str(p, cmdsize));
    else if (cmd === LC.RPATH) out.rpaths.push(str(p, cmdsize));
    else if (cmd === LC.DYLD_ENVIRONMENT) out.env.push(str(p, cmdsize));
    else if (cmd === LC.BUILD_VERSION) {
      out.platform = u32(p + 8);
      out.minos = decodeVersion(u32(p + 12));
    } else if (cmd === LC.VERSION_MIN_MACOSX && out.minos === null) {
      out.platform = PLATFORM_MACOS;
      out.minos = decodeVersion(u32(p + 8));
    }
    p += cmdsize;
  }
  return out;
}

/** Every slice of a Mach-O file, thin or universal. null when the file is not Mach-O. */
export function readMachO(buf) {
  const k = kind(buf);
  if (!k) return null;
  if (!k.fat) return [readSlice(buf, 0)];
  const n = buf.readUInt32BE(4);
  const slices = [];
  for (let i = 0; i < n; i++) {
    const at = 8 + i * (k.wide ? 32 : 20);
    if (at + (k.wide ? 32 : 20) > buf.length) throw new Error('truncated universal header');
    const offset = k.wide ? Number(buf.readBigUInt64BE(at + 8)) : buf.readUInt32BE(at + 8);
    const size = k.wide ? Number(buf.readBigUInt64BE(at + 16)) : buf.readUInt32BE(at + 12);
    if (offset + size > buf.length) throw new Error(`slice ${i} runs past the end of the file`);
    slices.push(readSlice(buf, offset, size));
  }
  return slices;
}

/** The findings for one file's slices; empty when it is clean. */
export function checkSlices(slices, { floor, arches }) {
  const v = [];
  const got = slices.map((s) => s.arch).sort();
  const want = [...arches].sort();
  if (got.join(',') !== want.join(',')) v.push(`architectures ${got.join('+')}, expected ${want.join('+')}`);
  for (const s of slices) {
    const at = slices.length > 1 ? `[${s.arch}] ` : '';
    if (s.minos === null) v.push(`${at}records no minimum macOS (no LC_BUILD_VERSION or LC_VERSION_MIN_MACOSX)`);
    else if (s.platform !== PLATFORM_MACOS) v.push(`${at}is built for platform ${s.platform}, not macOS`);
    else if (cmpVersion(s.minos, floor) > 0) v.push(`${at}needs macOS ${formatVersion(s.minos)}, newer than the app's minimum ${formatVersion(floor)}`);
    if (!s.loads.includes(LIBSYSTEM)) v.push(`${at}does not load ${LIBSYSTEM}; nothing was read from it, or it is not a real program`);
    for (const l of s.loads) if (!LOAD_OK.test(l)) v.push(`${at}loads ${l}, which is neither bundled (@rpath/@loader_path/@executable_path) nor a system library`);
    for (const r of s.rpaths) if (!RPATH_OK.test(r)) v.push(`${at}searches run path ${r}, outside the bundle and the system`);
    for (const x of [...s.loads, ...s.ids, ...s.rpaths, ...s.env]) {
      for (const bad of FORBIDDEN) {
        if (x === bad || x.startsWith(`${bad}/`)) v.push(`${at}names ${x}, under ${bad}, which exists only on the build machine`);
      }
    }
  }
  return [...new Set(v)];
}

/** LSMinimumSystemVersion out of an XML Info.plist; throws on a binary one. */
export function plistMinimum(text) {
  if (text.startsWith('bplist')) throw new Error('a binary plist; convert it with `plutil -convert xml1` first');
  const m = /<key>\s*LSMinimumSystemVersion\s*<\/key>\s*<string>\s*([^<]*?)\s*<\/string>/.exec(text);
  return m ? m[1] : null;
}

function* walk(p) {
  const st = fs.lstatSync(p);
  if (st.isSymbolicLink()) return;
  if (st.isDirectory()) {
    for (const e of fs.readdirSync(p).sort()) yield* walk(path.join(p, e));
  } else if (st.isFile()) {
    yield p;
  }
}

/** Check paths; returns { findings, count, files }. */
export function checkPaths(paths, { floor, arches, plist = null, require = [] }) {
  const findings = [];
  const files = [];
  for (const root of paths) {
    if (!fs.existsSync(root)) {
      findings.push(`${root}: does not exist`);
      continue;
    }
    for (const f of walk(root)) {
      let slices;
      try {
        // The header and load commands are at the front, but a universal file's slices are not;
        // the files here are tens of MB at most, so read each whole.
        slices = readMachO(fs.readFileSync(f));
      } catch (e) {
        findings.push(`${f}: unreadable Mach-O (${e.message})`);
        continue;
      }
      if (!slices) continue;
      files.push(f);
      for (const x of checkSlices(slices, { floor, arches })) findings.push(`${f}: ${x}`);
    }
  }
  if (!files.length) findings.push(`no Mach-O file found under ${paths.join(', ')}; nothing was checked`);
  for (const r of require) {
    const want = path.resolve(paths[0] ?? '.', r);
    if (!files.some((f) => path.resolve(f) === want)) findings.push(`${want}: required, and not among the Mach-O files read`);
  }
  if (plist) {
    try {
      const got = plistMinimum(fs.readFileSync(plist, 'utf8'));
      if (got === null) findings.push(`${plist}: sets no LSMinimumSystemVersion (the bundler's default would apply)`);
      else if (!parseVersion(got) || cmpVersion(parseVersion(got), floor) !== 0) {
        findings.push(`${plist}: LSMinimumSystemVersion is ${got}, not the app's minimum ${formatVersion(floor)}`);
      }
    } catch (e) {
      findings.push(`${plist}: ${e.message}`);
    }
  }
  return { findings, count: files.length, files };
}

function main(argv) {
  const usage = (why) => {
    console.error(`check-macho-bundle: ${why}\nusage: check-macho-bundle.mjs --floor <x.y> --arch <arm64|x86_64>... [--plist <Info.plist>] [--require <rel>]... <path>...`);
    return 2;
  };
  let floor = null;
  const arches = [];
  const require = [];
  let plist = null;
  const paths = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const val = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    try {
      if (a === '--floor') floor = parseVersion(val());
      else if (a === '--arch') arches.push(val());
      else if (a === '--plist') plist = val();
      else if (a === '--require') require.push(val());
      else if (a.startsWith('--')) return usage(`unknown option ${a}`);
      else paths.push(a);
    } catch (e) {
      return usage(e.message);
    }
  }
  if (!floor) return usage('--floor is required and must be a version like 12.0');
  if (!arches.length) return usage('--arch is required');
  for (const a of arches) if (!Object.values(CPU).includes(a)) return usage(`unknown --arch ${a}`);
  if (!paths.length) return usage('no path to check');
  const { findings, count } = checkPaths(paths, { floor, arches, plist, require });
  if (findings.length) {
    console.error(`Mach-O check FAILED (${count} file(s) read, minimum macOS ${formatVersion(floor)}, ${arches.join('+')}):`);
    for (const f of findings) console.error(`  ${f}`);
    return 1;
  }
  console.log(`Mach-O check passed: ${count} file(s), ${arches.join('+')}, minimum macOS ${formatVersion(floor)} or lower, every load bundled or system${plist ? ', LSMinimumSystemVersion matches' : ''}`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}

#!/usr/bin/env node
// verify-updater-sigs.mjs — prove each file's updater signature against the public key every
// shipped client trusts, before the release goes out.
//
//   usage: node scripts/verify-updater-sigs.mjs --version <ver> --not-before <unix time> <tauri.conf.json> <file>...
//
// WHY. sign-updaters signs the Windows and Linux installers and the macOS bundler signs its
// updater tarball, each in another job; publish then takes those bytes back out of the artifact
// store. Until this ran, nothing in publish tied what it ships to what was signed: an installer
// replaced in the store after signing would pass every gate, be summed into SHA256SUMS and be
// published, and only self-updating clients would ever refuse it. A fresh download from the
// release page had nothing protecting it.
//
// WHAT IT CHECKS, per file, against `plugins.updater.pubkey` (the key compiled into every client):
//   - <file>.sig exists beside it and is a minisign signature by that key's id;
//   - the signature verifies over the file (prehashed "ED": BLAKE2b-512 of the file, which is what
//     tauri-cli and tauri's bundler write; legacy "Ed" over the raw file is accepted as minisign
//     itself does);
//   - the global signature verifies over the signature and its trusted comment, so the comment
//     (timestamp, file, version) cannot have been edited;
//   - and then what that verified comment SAYS. A signature checked only for being valid can be
//     replayed: an older installer swapped in with its own old .sig is signed by the right key over
//     exactly its bytes. So the comment's `file:` must be this file's name, its `version:` this
//     release's, and its `timestamp:` no earlier than --not-before (this run, less a margin), so a
//     signature from an earlier run of the same version cannot be replayed under the same name.
//
// THE COMMENT, as tauri writes it: `timestamp:<unix>\tfile:<name>\tversion:<ver>`. sign-updaters
// (tauri-cli `signer sign --app-version`) names the file it signed, which is the shipped name. The
// macOS bundler signs the tarball as `Nexus.app.tar.gz`, before the macos job renames it
// Nexus_<ver>_<arch>.app.tar.gz, so a `.app.tar.gz` may also carry that one name. For those two, the
// version and the timestamp are what tie the signature to this release, not the file name.
// No secret: it reads only the public key. Node's own crypto, so nothing is installed for it.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const fail = (why) => {
  console.error(`verify-updater-sigs: ${why}`);
  process.exit(1);
};

// tauri stores the minisign files base64-wrapped whole; the second line of each is the payload.
const unwrap = (b64, what) => {
  const lines = Buffer.from(b64.trim(), 'base64').toString('utf8').split('\n');
  if (!/^untrusted comment: /.test(lines[0] ?? '')) fail(`${what} is not a minisign file`);
  return lines;
};
const ed25519 = (raw) =>
  crypto.createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw]), format: 'der', type: 'spki' });

function blake2b512(file) {
  const h = crypto.createHash('blake2b512');
  const fd = fs.openSync(file, 'r');
  const buf = Buffer.alloc(1 << 20);
  for (let n; (n = fs.readSync(fd, buf, 0, buf.length, null)) > 0; ) h.update(buf.subarray(0, n));
  fs.closeSync(fd);
  return h.digest();
}

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  if (i < 0 || args[i + 1] === undefined) fail(`usage: verify-updater-sigs.mjs --version <ver> --not-before <unix time> <tauri.conf.json> <file>...`);
  return args.splice(i, 2)[1];
};
const version = flag('--version');
const notBefore = Number(flag('--not-before'));
if (!Number.isInteger(notBefore)) fail('--not-before is not a unix time');
const [conf, ...files] = args;
if (!conf || !files.length) fail('usage: verify-updater-sigs.mjs --version <ver> --not-before <unix time> <tauri.conf.json> <file>...');
const BUNDLER_TARBALL = 'Nexus.app.tar.gz';

const pubB64 = JSON.parse(fs.readFileSync(conf, 'utf8'))?.plugins?.updater?.pubkey;
if (!pubB64) fail(`no plugins.updater.pubkey in ${conf}`);
const pk = Buffer.from(unwrap(pubB64, 'the updater public key')[1] ?? '', 'base64');
if (pk.length !== 42 || pk.subarray(0, 2).toString() !== 'Ed') fail('the updater public key is not an Ed25519 minisign key');
const keyId = pk.subarray(2, 10);
const key = ed25519(pk.subarray(10));

for (const file of files) {
  if (!fs.statSync(file, { throwIfNoEntry: false })?.isFile()) fail(`${file} is not a file`);
  const sigFile = `${file}.sig`;
  if (!fs.existsSync(sigFile)) fail(`${file} has no signature beside it (${sigFile})`);
  const lines = unwrap(fs.readFileSync(sigFile, 'utf8'), sigFile);
  const sig = Buffer.from(lines[1] ?? '', 'base64');
  const comment = (lines[2] ?? '').match(/^trusted comment: (.*)$/)?.[1];
  const global = Buffer.from(lines[3] ?? '', 'base64');
  if (sig.length !== 74 || comment === undefined || global.length !== 64) fail(`${sigFile} is not a complete minisign signature`);
  const alg = sig.subarray(0, 2).toString();
  if (alg !== 'ED' && alg !== 'Ed') fail(`${sigFile} uses signature algorithm ${JSON.stringify(alg)}`);
  if (!sig.subarray(2, 10).equals(keyId)) {
    fail(`${sigFile} is by key ${sig.subarray(2, 10).toString('hex')}, but clients trust ${keyId.toString('hex')}`);
  }
  const signed = alg === 'ED' ? blake2b512(file) : fs.readFileSync(file);
  if (!crypto.verify(null, signed, key, sig.subarray(10))) fail(`${sigFile} does not verify over ${file}: the file is not the one that was signed`);
  if (!crypto.verify(null, Buffer.concat([sig.subarray(10), Buffer.from(comment)]), key, global)) {
    fail(`${sigFile}: the trusted comment does not verify (edited after signing)`);
  }
  // Only now is the comment worth reading: it is what the key holder signed.
  const fields = new Map(comment.split('\t').map((f) => [f.slice(0, f.indexOf(':')), f.slice(f.indexOf(':') + 1)]).filter(([k]) => k));
  const name = path.basename(file);
  const signedFor = fields.get('file');
  if (signedFor === undefined) fail(`${sigFile}: the trusted comment names no file`);
  if (signedFor !== name && !(name.endsWith('.app.tar.gz') && signedFor === BUNDLER_TARBALL)) {
    fail(`${sigFile} was signed for ${signedFor}, not ${name}`);
  }
  if (!fields.has('version')) fail(`${sigFile}: the trusted comment binds no version`);
  if (fields.get('version') !== version) fail(`${sigFile} was signed for version ${fields.get('version')}, not ${version}`);
  const ts = fields.get('timestamp');
  if (!/^[0-9]+$/.test(ts ?? '')) fail(`${sigFile}: the trusted comment has no timestamp`);
  if (Number(ts) < notBefore) fail(`${sigFile} was signed at ${ts}, older than this run (not before ${notBefore})`);
  console.log(`verified: ${file} (${comment})`);
}

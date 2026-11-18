#!/usr/bin/env node
// verify-updater-sigs.mjs — prove each file's updater signature against the public key every
// shipped client trusts, before the release goes out.
//
//   usage: node scripts/verify-updater-sigs.mjs <tauri.conf.json> <file>...
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
//     (timestamp, file, version) cannot have been edited.
// No secret: it reads only the public key. Node's own crypto, so nothing is installed for it.
import crypto from 'node:crypto';
import fs from 'node:fs';

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

const [conf, ...files] = process.argv.slice(2);
if (!conf || !files.length) fail('usage: verify-updater-sigs.mjs <tauri.conf.json> <file>...');

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
  console.log(`verified: ${file} (${comment})`);
}

// Verifica end-to-end del Web Worker (feature `worker`) in Node, senza browser.
//
// Carica il vero `core.js` / `core_bg.wasm` prodotti da wasm-bindgen (`--target web`), emula il
// `DedicatedWorkerGlobalScope` (`instanceof`, `postMessage`, `onmessage`) e usa `Blob`/`File`,
// `DOMException` e `performance` di Node, che hanno la stessa interfaccia del browser. Verifica il
// protocollo di docs/PIANO.md §5.3: ready, started/progress/done, digest (confrontati con
// `node:crypto`, `node:zlib` e con i vettori noti), annullamento, coda FIFO ed errori.
//
// Uso (dalla radice del workspace):
//   cargo build --target wasm32-unknown-unknown -p hasher-core --features worker
//   wasm-bindgen --target web --no-typescript --out-dir <dir> --out-name core \
//       target/wasm32-unknown-unknown/debug/hasher_core.wasm
//   node --openssl-legacy-provider crates/hasher-core/tests/worker_node.mjs <dir>
//
// (`--openssl-legacy-provider` serve per MD4, usato come riferimento di ED2K.)
// Esce con codice 0 se tutte le verifiche passano.

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { crc32 } from 'node:zlib';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

const dir = path.resolve(process.argv[2] ?? '.');
const ED2K_CHUNK = 9_728_000;

// --- Emulazione del DedicatedWorkerGlobalScope ---------------------------------------------------

globalThis.DedicatedWorkerGlobalScope = class {
  static [Symbol.hasInstance](value) {
    return value === globalThis;
  }
};

/** Messaggi inviati dal worker: { at: performance.now(), msg }. */
const inbox = [];
let waiters = [];
globalThis.postMessage = (msg) => {
  inbox.push({ at: performance.now(), msg: structuredClone(msg) });
  const ready = waiters;
  waiters = [];
  for (const check of ready) check();
};
const send = (data) => globalThis.onmessage({ data });

/** Primo messaggio che soddisfa `pred` (anche già arrivato). */
function waitFor(pred, label, timeoutMs = 180_000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timeout waiting for ${label}`)), timeoutMs);
    const check = () => {
      const hit = inbox.find((e) => pred(e.msg));
      if (hit) {
        clearTimeout(timer);
        resolve(hit.msg);
      } else {
        waiters.push(check);
      }
    };
    check();
  });
}
const waitEv = (task, ev) => waitFor((m) => m.task === task && m.ev === ev, `${ev} of task ${task}`);
const waitEnd = (task) =>
  waitFor((m) => m.task === task && ['done', 'failed', 'cancelled'].includes(m.ev), `end of task ${task}`);
const eventsOf = (task) => inbox.filter((e) => e.msg.task === task);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- Verifiche -----------------------------------------------------------------------------------

let failures = 0;
let checks = 0;
function check(cond, what) {
  checks++;
  if (!cond) {
    failures++;
    console.error(`FAIL: ${what}`);
  }
}
function eq(actual, expected, what) {
  check(actual === expected, `${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

const ALL_IDS = [
  'crc32', 'crc64', 'xxh64', 'xxh3-128', 'md2', 'md4', 'md5', 'sha1', 'sha224', 'sha256', 'sha384',
  'sha512', 'sha3-256', 'sha3-512', 'ripemd128', 'ripemd160', 'ripemd256', 'ripemd320', 'blake2b-512',
  'blake2s-256', 'blake3', 'ed2k',
];

// Stessi vettori di tests/hash_vectors.rs: [hash di "", hash di "abc"].
const VECTORS = {
  crc32: ['00000000', '352441c2'],
  crc64: ['0000000000000000', '2cd8094a1a277627'],
  xxh64: ['ef46db3751d8e999', '44bc2cf5ad770999'],
  'xxh3-128': ['99aa06d3014798d86001c324468d497f', '06b05ab6733a618578af5f94892f3950'],
  md2: ['8350e5a3e24c153df2275c9f80692773', 'da853b0d3f88d99b30283a69e6ded6bb'],
  md4: ['31d6cfe0d16ae931b73c59d7e0c089c0', 'a448017aaf21d8525fc10ae87aa6729d'],
  md5: ['d41d8cd98f00b204e9800998ecf8427e', '900150983cd24fb0d6963f7d28e17f72'],
  sha1: ['da39a3ee5e6b4b0d3255bfef95601890afd80709', 'a9993e364706816aba3e25717850c26c9cd0d89d'],
  sha224: [
    'd14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f',
    '23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7',
  ],
  sha256: [
    'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
  ],
  sha384: [
    '38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b',
    'cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7',
  ],
  sha512: [
    'cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e',
    'ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f',
  ],
  'sha3-256': [
    'a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a',
    '3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532',
  ],
  'sha3-512': [
    'a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26',
    'b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0',
  ],
  ripemd128: ['cdf26213a150dc3ecb610f18f6b38b46', 'c14a12199c66e4ba84636b0f69144c77'],
  ripemd160: ['9c1185a5c5e9fc54612808977ee8f548b2258d31', '8eb208f7e05d987a9b044a8e98c6b087f15a0bfc'],
  ripemd256: [
    '02ba4c4e5f8ecd1877fc52d64d30e37a2d9774fb1e5d026380ae0168e3c5522d',
    'afbd6e228b9d8cbbcef5ca2d03e6dba10ac0bc7dcbe4680e1e42d2e975459b65',
  ],
  ripemd320: [
    '22d65d5661536cdc75c1fdf5c6de7b41b9f27325ebc61e8557177d705a0ec880151c3a32a00899b8',
    'de4c01b3054f8930a79d09ae738e92301e5a17085beffdc1b8d116713e74f82fa942d64cdbc4682d',
  ],
  'blake2b-512': [
    '786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce',
    'ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923',
  ],
  'blake2s-256': [
    '69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9',
    '508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982',
  ],
  blake3: [
    'af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262',
    '6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85',
  ],
  ed2k: ['31d6cfe0d16ae931b73c59d7e0c089c0', 'a448017aaf21d8525fc10ae87aa6729d'],
};

/** Algoritmi di `node:crypto` usati come riferimento indipendente. */
const NODE_HASH = {
  md4: 'md4', md5: 'md5', sha1: 'sha1', sha224: 'sha224', sha256: 'sha256', sha384: 'sha384',
  sha512: 'sha512', 'sha3-256': 'sha3-256', 'sha3-512': 'sha3-512', ripemd160: 'ripemd160',
  'blake2b-512': 'blake2b512', 'blake2s-256': 'blake2s256',
};

function pseudoRandom(len, seed) {
  const out = new Uint8Array(len);
  let x = seed >>> 0 || 1;
  for (let i = 0; i < len; i++) {
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    out[i] = x & 0xff;
  }
  return out;
}

function ed2kReference(data) {
  const md4 = (b) => createHash('md4').update(b).digest();
  if (data.length < ED2K_CHUNK) return md4(data).toString('hex');
  const parts = [];
  for (let off = 0; off < data.length; off += ED2K_CHUNK) parts.push(md4(data.subarray(off, off + ED2K_CHUNK)));
  if (data.length % ED2K_CHUNK === 0) parts.push(md4(new Uint8Array(0)));
  return md4(Buffer.concat(parts)).toString('hex');
}

function references(data) {
  const ref = { crc32: crc32(data).toString(16).padStart(8, '0'), ed2k: ed2kReference(data) };
  for (const [id, name] of Object.entries(NODE_HASH)) ref[id] = createHash(name).update(data).digest('hex');
  return ref;
}

let nextTask = 1;
async function hashAndWait(file, algos, chunk) {
  const task = nextTask++;
  const cmd = { cmd: 'hash', task, file, algos };
  if (chunk !== undefined) cmd.chunk = chunk;
  send(cmd);
  return { task, end: await waitEnd(task) };
}

// --- Avvio ---------------------------------------------------------------------------------------

const { default: init, worker_main } = await import(pathToFileURL(path.join(dir, 'core.js')).href);
await init({ module_or_path: readFileSync(path.join(dir, 'core_bg.wasm')) });
worker_main();
eq(inbox.length, 1, 'exactly one message after worker_main');
eq(inbox[0]?.msg.ev, 'ready', 'first message is ready');
eq(Object.keys(inbox[0]?.msg ?? {}).join(), 'ev', 'ready carries only ev');

// 1. Vettori noti su "" e "abc", tutti gli algoritmi.
for (const [index, text] of ['', 'abc'].entries()) {
  const { task, end } = await hashAndWait(new File([text], 'v.txt'), ALL_IDS);
  eq(end.ev, 'done', `vectors ${JSON.stringify(text)}: outcome`);
  eq(end.bytes, text.length, `vectors ${JSON.stringify(text)}: bytes`);
  eq(Object.keys(end.digests ?? {}).join(), ALL_IDS.join(), `vectors ${JSON.stringify(text)}: digest keys in order`);
  for (const id of ALL_IDS) eq(end.digests?.[id], VECTORS[id][index], `${id}(${JSON.stringify(text)})`);
  eq(eventsOf(task)[0].msg.ev, 'started', `vectors ${JSON.stringify(text)}: started first`);
}

// 2. Dati grandi (2 blocchi ED2K + 5 byte) a blocchi di default: confronto con node:crypto/zlib.
{
  const data = pseudoRandom(2 * ED2K_CHUNK + 5, 42);
  const ref = references(data);
  const file = new File([data], 'big.bin');
  const t0 = performance.now();
  const { task, end } = await hashAndWait(file, ALL_IDS);
  const wall = performance.now() - t0;
  eq(end.ev, 'done', 'big: outcome');
  eq(end.bytes, data.length, 'big: bytes');
  check(typeof end.elapsed_ms === 'number' && end.elapsed_ms > 0 && end.elapsed_ms <= wall + 1, 'big: elapsed_ms');
  for (const [id, hex] of Object.entries(ref)) eq(end.digests?.[id], hex, `big: ${id}`);
  for (const hex of Object.values(end.digests ?? {})) check(hex === hex.toLowerCase(), 'big: lowercase hex');
  const evs = eventsOf(task);
  const progress = evs.filter((e) => e.msg.ev === 'progress');
  eq(evs[0].msg.ev, 'started', 'big: started first');
  eq(evs.at(-1).msg.ev, 'done', 'big: done last');
  check(progress.length > 0, `big: progress events (${progress.length} in ${wall.toFixed(0)} ms)`);
  check(progress.length <= Math.ceil(wall / 50) + 1, `big: at most 20 progress/s (${progress.length})`);
  for (let i = 1; i < progress.length; i++) {
    check(progress[i].msg.bytes > progress[i - 1].msg.bytes, 'big: progress bytes increase');
    check(progress[i].at - progress[i - 1].at >= 49.5, `big: progress spacing ${progress[i].at - progress[i - 1].at}`);
  }
  check(progress.every((e) => e.msg.bytes > 0 && e.msg.bytes <= data.length), 'big: progress bytes in range');
  console.log(`big file: ${(data.length / 1e6).toFixed(1)} MB, 22 algorithms, ${wall.toFixed(0)} ms, ${progress.length} progress events`);

  // Stesso file a blocchi da 100 000 byte: digest identici (anche per gli algoritmi senza
  // riferimento in Node: MD2, RIPEMD-128/256/320, CRC64, xxHash, BLAKE3).
  const again = await hashAndWait(file, ALL_IDS, 100_000);
  eq(again.end.ev, 'done', 'big/100k: outcome');
  for (const id of ALL_IDS) eq(again.end.digests?.[id], end.digests?.[id], `big/100k: ${id}`);
}

// 3. ED2K su un multiplo esatto del blocco (si accoda l'MD4 del blocco vuoto).
{
  const data = pseudoRandom(ED2K_CHUNK, 7);
  const { end } = await hashAndWait(new File([data], 'exact.bin'), ['ed2k', 'md4']);
  eq(end.digests?.ed2k, ed2kReference(data), 'exact ED2K chunk: ed2k');
  eq(end.digests?.md4, createHash('md4').update(data).digest('hex'), 'exact ED2K chunk: md4');
  check(end.digests?.ed2k !== end.digests?.md4, 'exact ED2K chunk: ed2k differs from md4');
}

// 4. Id tolleranti e duplicati.
{
  const { end } = await hashAndWait(new File(['abc'], 'a'), ['SHA-256', 'md5', 'sha256']);
  eq(Object.keys(end.digests ?? {}).join(), 'sha256,md5', 'labels: normalised and deduplicated');
}

// 5. Annullamento del task in esecuzione.
const BIG = new File([new Uint8Array(64 * 1024 * 1024)], 'zeros.bin');
{
  const task = nextTask++;
  send({ cmd: 'hash', task, file: BIG, algos: ['sha256', 'md5'], chunk: 65536 });
  await waitEv(task, 'progress');
  send({ cmd: 'cancel', task });
  const end = await waitEnd(task);
  eq(end.ev, 'cancelled', 'cancel running: outcome');
  await sleep(200);
  eq(eventsOf(task).filter((e) => ['done', 'failed'].includes(e.msg.ev)).length, 0, 'cancel running: no done');
  eq(eventsOf(task).at(-1).msg.ev, 'cancelled', 'cancel running: cancelled is last');
}

// 6. cancel(-1) con un task in esecuzione e uno in coda.
{
  const a = nextTask++;
  const b = nextTask++;
  send({ cmd: 'hash', task: a, file: BIG, algos: ['sha256'], chunk: 65536 });
  send({ cmd: 'hash', task: b, file: BIG, algos: ['sha256'], chunk: 65536 });
  await waitEv(a, 'started');
  send({ cmd: 'cancel', task: -1 });
  eq((await waitEnd(a)).ev, 'cancelled', 'cancel all: running task');
  eq((await waitEnd(b)).ev, 'cancelled', 'cancel all: waiting task');
  eq(eventsOf(b).length, 1, 'cancel all: waiting task never started');
}

// 7. cancel(-1) inviato subito dopo due `hash` (nessuno ancora partito).
{
  const a = nextTask++;
  const b = nextTask++;
  send({ cmd: 'hash', task: a, file: BIG, algos: ['md5'] });
  send({ cmd: 'hash', task: b, file: BIG, algos: ['md5'] });
  send({ cmd: 'cancel', task: -1 });
  eq((await waitEnd(a)).ev, 'cancelled', 'immediate cancel all: a');
  eq((await waitEnd(b)).ev, 'cancelled', 'immediate cancel all: b');
  await sleep(100);
  eq(eventsOf(a).length + eventsOf(b).length, 2, 'immediate cancel all: only the two cancelled events');
}

// 8. Annullamento di un task in coda: il task in esecuzione prosegue; coda FIFO.
{
  const small = new File([pseudoRandom(3_000_000, 9)], 's.bin');
  const [a, b, c] = [nextTask++, nextTask++, nextTask++];
  send({ cmd: 'hash', task: a, file: BIG, algos: ['crc32'] });
  send({ cmd: 'hash', task: b, file: small, algos: ['sha1'] });
  send({ cmd: 'hash', task: c, file: small, algos: ['md5'] });
  await waitEv(a, 'started');
  send({ cmd: 'cancel', task: b });
  send({ cmd: 'cancel', task: 999_999 }); // sconosciuto: ignorato
  eq((await waitEnd(b)).ev, 'cancelled', 'cancel waiting: cancelled');
  eq(eventsOf(b).length, 1, 'cancel waiting: never started');
  const endA = await waitEnd(a);
  const endC = await waitEnd(c);
  eq(endA.ev, 'done', 'cancel waiting: running task completes');
  eq(endA.digests?.crc32, crc32(new Uint8Array(64 * 1024 * 1024)).toString(16).padStart(8, '0'), 'zeros: crc32');
  eq(endC.ev, 'done', 'FIFO: next task completes');
  const idx = (task, ev) => inbox.findIndex((e) => e.msg.task === task && e.msg.ev === ev);
  check(idx(a, 'done') < idx(c, 'started'), 'FIFO: one task at a time');
  eq(inbox.filter((e) => e.msg.task === 999_999).length, 0, 'unknown cancel: no reply');
}

// 9. Comandi non validi ed errori di lettura.
{
  const bad = async (cmd, what, needle) => {
    const task = nextTask++;
    send({ cmd: 'hash', task, ...cmd });
    const end = await waitEnd(task);
    eq(end.ev, 'failed', `${what}: outcome`);
    check(typeof end.error === 'string' && end.error.includes(needle), `${what}: error ${JSON.stringify(end.error)}`);
  };
  const f = new File(['abc'], 'a');
  await bad({ file: f, algos: ['sha256', 'whirlpool'] }, 'unknown algorithm', 'whirlpool');
  await bad({ file: f, algos: [] }, 'empty algos', 'no algorithm');
  await bad({ file: f, algos: 'sha256' }, 'algos not an array', 'array');
  await bad({ file: f, algos: ['sha256', 7] }, 'algos with a number', 'strings');
  await bad({ algos: ['sha256'] }, 'missing file', 'file');
  await bad({ file: 'abc', algos: ['sha256'] }, 'file not a Blob', 'file');

  // File modificato durante la lettura: le letture dopo 4 MiB falliscono con NotReadableError.
  class FlakyFile extends File {
    slice(start, end) {
      const piece = super.slice(start, end);
      if (start >= 4 * 1024 * 1024) {
        piece.arrayBuffer = () =>
          Promise.reject(new DOMException('The requested file could not be read.', 'NotReadableError'));
      }
      return piece;
    }
  }
  await bad({ file: new FlakyFile([new Uint8Array(10_000_000)], 'f'), algos: ['md5'] }, 'NotReadableError', 'NotReadableError: The requested file could not be read.');

  // Messaggi senza `task` valido o con `cmd` sconosciuto: nessuna risposta (solo console).
  const before = inbox.length;
  send({ cmd: 'hash', file: f, algos: ['md5'] });
  send({ cmd: 'hash', task: -3, file: f, algos: ['md5'] });
  send({ cmd: 'cancel' });
  send({ cmd: 'bogus', task: 1 });
  send(null);
  send('hash');
  await sleep(100);
  eq(inbox.length, before, 'invalid messages: no reply');

  // Il worker è ancora operativo.
  const { end } = await hashAndWait(f, ['crc32']);
  eq(end.digests?.crc32, '352441c2', 'still working after errors');
}

// 10. Ogni task ha esattamente un esito.
for (let task = 1; task < nextTask; task++) {
  const ends = eventsOf(task).filter((e) => ['done', 'failed', 'cancelled'].includes(e.msg.ev));
  eq(ends.length, 1, `task ${task}: exactly one outcome`);
}

console.log(`${checks - failures}/${checks} checks passed`);
process.exit(failures === 0 ? 0 : 1);

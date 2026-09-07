// Derive "what serde will accept" from the Rust type definitions THEMSELVES, so the
// node publish gate (scripts/check-fd-rules.mjs) and `tempo_core::fd_rules::parse_spec`
// cannot disagree about the SHAPE of a rules file.
//
// Why this exists. The two validators check different things and always did:
// check-fd-rules.mjs checks VALUES (`by_call` must be true, tiers must ascend, a
// pattern must be anchored) while serde checks PRESENCE and INTEGER WIDTH before any
// of that runs. Four measured mutations — `role.selector` absent, `role.id` absent,
// `text.max_len` 300 (the Rust field is `u8`), `number.min` -1 (the Rust field is
// `u32`) — passed the node gate with exit 0 and were refused by every shipped app.
// That is the exact failure the gate exists to prevent, landing on users instead of
// on CI.
//
// Hand-adding those four checks would leave the fifth for someone to find in
// production, so the fix is structural: read `crates/tempo-core/src/fd_rules.rs`,
// extract every `#[derive(… Deserialize …)]` struct and enum, and check the candidate
// JSON against THAT — required-field presence and integer bounds come from the Rust
// types, never from a restatement here. Adding a field, changing `u8` to `u16` or
// dropping a `#[serde(default)]` moves both halves in the same commit, by
// construction.
//
// Scope: the serde subset `fd_rules.rs` actually uses. Anything outside it is a hard
// error rather than a silent pass — see `schemaFromRust`'s reachability check.
//
// ⚠️ WHAT THIS READS, AND WHY IT IS NOT `JSON.parse` OUTPUT. The shape check walks
// the document `readJson` builds from the SOURCE TEXT, never a parsed value, because
// by the time `JSON.parse` returns, JavaScript has already destroyed the three things
// serde judges on:
//
//   1. the LEXICAL FORM of a number — `2.0` and `2` are the same JS number, and serde
//      refuses the first (`invalid type: floating point`) for every integer field;
//   2. REPEATED KEYS — `JSON.parse` keeps the last silently, serde's derive returns
//      `duplicate field`;
//   3. the MAGNITUDE of an integer — a JS double cannot hold `u64::MAX`, so the bound
//      itself rounds UP to 2^64 and a literal past the range compares as inside it.
//      Here every bound and every literal is a BigInt read off the text.
//
// `readJson` also refuses the one thing `JSON.parse` accepts and serde_json does not:
// an unpaired UTF-16 surrogate in a `\u` escape.
//
// The three integer/dup classes and the surrogate each have a corpus fixture; a rule
// here without one is a rule only one validator has.

/** Inclusive bounds per Rust integer width — BigInt, because `u64`/`i64` do not fit a double. */
const INT_BOUNDS = {
  u8: [0n, 255n],
  u16: [0n, 65535n],
  u32: [0n, 4294967295n],
  u64: [0n, 18446744073709551615n],
  i8: [-128n, 127n],
  i16: [-32768n, 32767n],
  i32: [-2147483648n, 2147483647n],
  i64: [-9223372036854775808n, 9223372036854775807n],
}

/** Rust `SomeName` → serde's `rename_all = "snake_case"` wire name. */
const snakeCase = (s) =>
  s
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
    .toLowerCase()

/** One Rust type expression, as the thing a JSON value has to be. */
function parseType(src) {
  const s = src.trim()
  let m
  if ((m = /^Option<(.+)>$/.exec(s))) return { t: 'option', of: parseType(m[1]) }
  if ((m = /^Vec<(.+)>$/.exec(s))) return { t: 'vec', of: parseType(m[1]) }
  if ((m = /^(?:BTreeMap|HashMap)<\s*String\s*,\s*(.+)>$/.exec(s)))
    return { t: 'map', of: parseType(m[1]) }
  if (s === 'String') return { t: 'string' }
  if (s === 'bool') return { t: 'bool' }
  if (s in INT_BOUNDS) return { t: 'int', rust: s }
  if (/^[A-Z]\w*$/.test(s)) return { t: 'named', name: s }
  throw new Error(`unsupported Rust type ${JSON.stringify(s)}`)
}

/** The `{ … }` body of the item whose opening brace is on `lines[start]`. */
function bodyOf(lines, start) {
  let depth = 0
  const out = []
  for (let i = start; i < lines.length; i++) {
    for (const c of lines[i]) {
      if (c === '{') depth++
      else if (c === '}') depth--
    }
    if (i > start) out.push(lines[i])
    if (depth === 0) return { body: out.slice(0, -1), end: i }
  }
  throw new Error('unterminated item body')
}

/** `#[serde(default)]` etc. on a FIELD — anything unrecognised is a hard error. */
function fieldAttr(line, state) {
  const m = /^#\[serde\((.*)\)\]$/.exec(line)
  if (!m) {
    // Non-serde attributes cannot change the accepted JSON, with one exception
    // that would silently remove a field from the schema.
    if (/^#\[cfg[(_]/.test(line)) throw new Error(`conditional field: ${line}`)
    return
  }
  for (const part of m[1].split(',').map((p) => p.trim())) {
    if (part === 'default') state.optional = true
    else if ((/^rename\s*=\s*"(.*)"$/.exec(part) || [])[1] !== undefined)
      state.rename = /^rename\s*=\s*"(.*)"$/.exec(part)[1]
    else throw new Error(`unsupported serde field attribute ${JSON.stringify(part)}`)
  }
}

/** The fields of a struct body (or of a struct enum variant). */
function parseFields(lines) {
  const fields = []
  const state = { optional: false, rename: null }
  for (const raw of lines) {
    const line = raw.trim()
    if (!line || line.startsWith('//')) continue
    if (line.startsWith('#[')) {
      fieldAttr(line, state)
      continue
    }
    const m = /^(?:pub\s+)?(\w+)\s*:\s*(.+?)\s*,$/.exec(line)
    if (!m) throw new Error(`unparsed field line ${JSON.stringify(line)}`)
    const ty = parseType(m[2])
    fields.push({
      // serde fills a missing `Option` with `None` even with no attribute, so an
      // `Option` field is optional exactly like a `#[serde(default)]` one.
      name: state.rename ?? m[1],
      ty,
      optional: state.optional || ty.t === 'option',
    })
    state.optional = false
    state.rename = null
  }
  return fields
}

/** The variants of an internally tagged enum body. */
function parseVariants(lines, renameAll) {
  const variants = []
  let buf = ''
  for (const raw of lines) {
    const line = raw.trim()
    if (!line || line.startsWith('//') || line.startsWith('#[')) continue
    buf += (buf ? ' ' : '') + line
    // A struct variant can wrap; only commit once its braces balance.
    const open = (buf.match(/\{/g) || []).length
    if (open !== (buf.match(/\}/g) || []).length) continue
    let m
    if ((m = /^(\w+)\s*\{(.*)\}\s*,?$/.exec(buf))) {
      const inner = m[2]
        .split(',')
        .map((f) => f.trim())
        .filter(Boolean)
        .map((f) => `${f},`)
      variants.push({ name: m[1], fields: parseFields(inner) })
    } else if ((m = /^(\w+)\s*,?$/.exec(buf))) {
      variants.push({ name: m[1], fields: [] })
    } else {
      throw new Error(`unparsed enum variant ${JSON.stringify(buf)}`)
    }
    const v = variants[variants.length - 1]
    if (renameAll !== 'snake_case') throw new Error(`unsupported rename_all ${renameAll}`)
    v.wire = snakeCase(v.name)
    buf = ''
  }
  return variants
}

/**
 * Every `#[derive(… Deserialize …)]` struct/enum in `src`, then a reachability walk
 * from `root`: a type the checker could ever need but could not be derived is a HARD
 * error here rather than a shape node silently stops checking.
 */
export function schemaFromRust(src, root) {
  const lines = src.split('\n')
  const types = new Map()
  const broken = new Map()
  for (let i = 0; i < lines.length; i++) {
    if (!/^\s*#\[derive\(.*\bDeserialize\b/.test(lines[i])) continue
    let j = i + 1
    let container = ''
    while (j < lines.length && /^\s*#\[/.test(lines[j])) container += lines[j++].trim()
    const head = /^\s*(?:pub\s+)?(struct|enum)\s+(\w+)\s*\{\s*$/.exec(lines[j] ?? '')
    if (!head) continue // tuple/unit struct — nothing this schema can be built from
    const [, kw, name] = head
    const { body, end } = bodyOf(lines, j)
    i = end
    try {
      if (kw === 'struct') {
        types.set(name, { kind: 'struct', name, fields: parseFields(body) })
      } else {
        const tag = /tag\s*=\s*"(.*?)"/.exec(container)
        const renameAll = /rename_all\s*=\s*"(.*?)"/.exec(container)
        // Externally tagged is a different JSON shape entirely; refuse to guess.
        if (!tag) throw new Error('enum is not internally tagged')
        types.set(name, {
          kind: 'enum',
          name,
          tag: tag[1],
          variants: parseVariants(body, renameAll?.[1]),
        })
      }
    } catch (e) {
      broken.set(name, e.message)
    }
  }
  if (!types.has(root) && !broken.has(root))
    throw new Error(`fd-rules schema: no \`${root}\` in the Rust source — the derivation is dead`)
  // Reachability: everything the checker can arrive at must be fully derived.
  const seen = new Set()
  const need = (ty, from) => {
    if (ty.t === 'option' || ty.t === 'vec' || ty.t === 'map') return need(ty.of, from)
    if (ty.t !== 'named') return
    if (broken.has(ty.name))
      throw new Error(`fd-rules schema: ${ty.name} (from ${from}): ${broken.get(ty.name)}`)
    if (!types.has(ty.name))
      throw new Error(`fd-rules schema: ${from} names unknown type ${ty.name}`)
    if (seen.has(ty.name)) return
    seen.add(ty.name)
    const d = types.get(ty.name)
    const fields = d.kind === 'struct' ? d.fields : d.variants.flatMap((v) => v.fields)
    for (const f of fields) need(f.ty, ty.name)
  }
  need({ t: 'named', name: root }, 'the root')
  return { types, root }
}

// ---- the document, read off the SOURCE TEXT --------------------------------

/**
 * A JSON document as its source text, for the three things `JSON.parse` throws
 * away (see the header): a number keeps its LITERAL, a map keeps its entries as a
 * LIST so a repeat is still visible, and a string keeps its decoded value.
 *
 * Nodes: `{t:'map',entries:[{key,node}]}` · `{t:'seq',items:[node]}` ·
 * `{t:'num',raw}` · `{t:'str',v}` · `{t:'bool',v}` · `{t:'null'}`.
 *
 * Throws with `serdeJson: true` for a fault serde_json reports and `JSON.parse`
 * does not; any OTHER throw is a bug in this reader (the caller parses first, so
 * the text is known-good JSON) and must stay loud rather than pass silently.
 */
export function readJson(text) {
  let i = 0
  const fault = (msg) => {
    throw Object.assign(new Error(msg), { serdeJson: true })
  }
  const bug = (msg) => {
    throw new Error(
      `fd-rules JSON reader: ${msg} at offset ${i} — the text parsed as JSON, so this is a reader bug`,
    )
  }
  const WS = new Set([' ', '\t', '\n', '\r'])
  const ws = () => {
    while (i < text.length && WS.has(text[i])) i++
  }
  const hex4 = () => {
    const h = text.slice(i, i + 4)
    if (!/^[0-9a-fA-F]{4}$/.test(h)) bug('malformed hex escape')
    i += 4
    return parseInt(h, 16)
  }
  // serde_json's own rule (read.rs `parse_unicode_escape`): a UTF-8 string's
  // surrogates must be PAIRED. `JSON.parse` keeps a lone one, so a rules file
  // carrying (say) half a truncated emoji shipped green and the app refused the
  // whole download. The two message texts are serde_json's, including the
  // mislabelled one it uses for a leading trail-surrogate.
  const unicodeEscape = () => {
    const c = hex4()
    if (c >= 0xdc00 && c <= 0xdfff) fault('lone leading surrogate in hex escape')
    if (c < 0xd800 || c > 0xdbff) return String.fromCharCode(c)
    if (text[i] !== '\\') fault('unexpected end of hex escape')
    i++
    if (text[i] !== 'u') fault('unexpected end of hex escape')
    i++
    const c2 = hex4()
    if (c2 < 0xdc00 || c2 > 0xdfff) fault('lone leading surrogate in hex escape')
    return String.fromCharCode(c, c2)
  }
  const ESCAPES = { '"': '"', '\\': '\\', '/': '/', b: '\b', f: '\f', n: '\n', r: '\r', t: '\t' }
  const str = () => {
    i++ // the opening quote
    let out = ''
    for (;;) {
      const ch = text[i]
      if (ch === undefined) bug('unterminated string')
      if (ch === '"') {
        i++
        return out
      }
      if (ch !== '\\') {
        out += ch
        i++
        continue
      }
      i++
      const e = text[i++]
      if (e === 'u') out += unicodeEscape()
      else if (e in ESCAPES) out += ESCAPES[e]
      else bug('unknown string escape')
    }
  }
  const digits = () => {
    while (i < text.length && text[i] >= '0' && text[i] <= '9') i++
  }
  const num = () => {
    const start = i
    if (text[i] === '-') i++
    digits()
    if (text[i] === '.') {
      i++
      digits()
    }
    if (text[i] === 'e' || text[i] === 'E') {
      i++
      if (text[i] === '+' || text[i] === '-') i++
      digits()
    }
    const raw = text.slice(start, i)
    if (!/^-?\d/.test(raw)) bug('expected a value')
    // serde_json errors rather than handing the visitor an infinity (de.rs's
    // NumberOutOfRange); `JSON.parse` yields `Infinity` and says nothing.
    if (!Number.isFinite(Number(raw))) fault('number out of range')
    return { t: 'num', raw }
  }
  const lit = (word, node) => {
    if (text.slice(i, i + word.length) !== word) bug('expected a value')
    i += word.length
    return node
  }
  const value = () => {
    ws()
    switch (text[i]) {
      case '{': {
        i++
        const entries = []
        ws()
        if (text[i] === '}') {
          i++
          return { t: 'map', entries }
        }
        for (;;) {
          ws()
          if (text[i] !== '"') bug('expected a key')
          const key = str()
          ws()
          if (text[i] !== ':') bug('expected `:`')
          i++
          entries.push({ key, node: value() })
          ws()
          if (text[i] === ',') {
            i++
            continue
          }
          if (text[i] === '}') {
            i++
            return { t: 'map', entries }
          }
          bug('expected `,` or `}`')
        }
      }
      case '[': {
        i++
        const items = []
        ws()
        if (text[i] === ']') {
          i++
          return { t: 'seq', items }
        }
        for (;;) {
          items.push(value())
          ws()
          if (text[i] === ',') {
            i++
            continue
          }
          if (text[i] === ']') {
            i++
            return { t: 'seq', items }
          }
          bug('expected `,` or `]`')
        }
      }
      case '"':
        return { t: 'str', v: str() }
      case 't':
        return lit('true', { t: 'bool', v: true })
      case 'f':
        return lit('false', { t: 'bool', v: false })
      case 'n':
        return lit('null', { t: 'null' })
      default:
        return num()
    }
  }
  const doc = value()
  ws()
  if (i !== text.length) bug('trailing text')
  return doc
}

/** Inclusive range of the integers serde_json's own accumulator can hold. */
const MACHINE_INT = [-9223372036854775808n, 18446744073709551615n]

/**
 * The integer a number LITERAL hands serde's visitor, or `null` when the visitor
 * is handed a float instead — which is a float-shaped literal (`2.0`, `1e2`), an
 * integer past the machine integers (serde_json's parser falls back to `f64` when
 * its `u64`/`i64` accumulator overflows, de.rs `parse_long_integer`), and NEGATIVE
 * ZERO.
 *
 * `-0` is the same trap as `2.0`, one level further down: `BigInt` collapses it to
 * `0n` exactly as `JSON.parse` collapses `2.0` to `2`, so the sign that decides the
 * answer is gone before the comparison. It has to be read off the TEXT. serde_json's
 * branch (de.rs `parse_number`) is
 *
 *     let neg = (significand as i64).wrapping_neg();
 *     // Convert into a float if we underflow, or on `-0`.
 *     if neg >= 0 { ParserNumber::F64(-(significand as f64)) } else { ... }
 *
 * and `wrapping_neg() >= 0` holds for exactly two inputs: a significand past
 * `i64::MAX` (the `MACHINE_INT` bound below already covers that half) and a
 * significand of zero — which JSON's grammar can only spell `-0`.
 */
function machineInt(raw) {
  if (!/^-?\d+$/.test(raw)) return null
  if (raw === '-0') return null
  const v = BigInt(raw)
  return v < MACHINE_INT[0] || v > MACHINE_INT[1] ? null : v
}

/**
 * A float as serde_json prints it in an error — `zmij::Buffer::format` (error.rs's
 * `JsonUnexpected`), NOT Rust's `Display`, which would render 2^64 as
 * `18446744073709552000` where serde_json says `1.8446744073709552e+19`.
 * Shortest-round-trip digits (what `toExponential()` gives) laid out zmij's way:
 * plain decimal with a forced point while the leading digit sits in `10^-5..10^15`,
 * else `d.ddde±N` with the exponent as plain digits and a sign that is always written.
 * Checked against serde_json itself for 44 literals — see the fix report.
 *
 * `raw` is always finite here: `readJson` refuses a literal that is not, exactly
 * where serde_json does.
 */
function zmijFloat(raw) {
  const n = Number(raw)
  const sign = n < 0 || Object.is(n, -0) ? '-' : ''
  const [mant, ex] = Math.abs(n).toExponential().split('e')
  const d = mant.replace('.', '')
  const exp = Number(ex)
  if (exp >= -5 && exp <= 15) {
    if (d.length - 1 <= exp) return `${sign}${d}${'0'.repeat(exp - d.length + 1)}.0`
    if (exp >= 0) return `${sign}${d.slice(0, exp + 1)}.${d.slice(exp + 1)}`
    return `${sign}0.${'0'.repeat(-exp - 1)}${d}`
  }
  const m = d.length > 1 ? `${d[0]}.${d.slice(1)}` : d
  return `${sign}${m}e${exp < 0 ? '-' : '+'}${Math.abs(exp)}`
}

/** How serde_json names the JSON value it actually found. */
function found(v) {
  switch (v.t) {
    case 'null':
      return 'null'
    case 'seq':
      return 'sequence'
    case 'map':
      return 'map'
    case 'str':
      return `string ${JSON.stringify(v.v)}`
    case 'bool':
      return `boolean \`${v.v}\``
    default: {
      const n = machineInt(v.raw)
      return n === null ? `floating point \`${zmijFloat(v.raw)}\`` : `integer \`${n}\``
    }
  }
}

/** How serde names the thing it wanted. */
function wanted(ty, schema) {
  switch (ty.t) {
    case 'string':
      return 'a string'
    case 'bool':
      return 'a boolean'
    case 'int':
      return ty.rust
    case 'vec':
      return 'a sequence'
    case 'map':
      return 'a map'
    case 'option':
      return wanted(ty.of, schema)
    default: {
      const d = schema.types.get(ty.name)
      return d.kind === 'enum' ? `internally tagged enum ${ty.name}` : `struct ${ty.name}`
    }
  }
}

/**
 * Check the declared fields of a struct (or struct enum variant) against a document
 * map, in serde's own order: present fields are typed as the DOCUMENT orders them,
 * then missing fields are reported in DECLARATION order. Unknown keys are ignored,
 * exactly as serde does without `deny_unknown_fields`.
 *
 * A REPEATED known field is `duplicate field` — the derive's `if x.is_some()` guard
 * fires before it deserializes that entry's value, so the repeat beats a type error
 * on the same entry. Only a struct field is a duplicate: `BTreeMap`/`HashMap` takes
 * the repeats as inserts and the last one wins, in node and in serde alike.
 */
function checkFields(fields, v, schema) {
  const byName = new Map(fields.map((f) => [f.name, f]))
  const seen = new Set()
  for (const { key, node } of v.entries) {
    const f = byName.get(key)
    if (!f) continue
    if (seen.has(key)) return `duplicate field \`${key}\``
    seen.add(key)
    const e = checkType(node, f.ty, schema)
    if (e) return e
  }
  for (const f of fields) if (!f.optional && !seen.has(f.name)) return `missing field \`${f.name}\``
  return null
}

/** `null` if `v` is a shape serde would accept for `ty`, else serde's own message. */
export function checkType(v, ty, schema) {
  const mismatch = () => `invalid type: ${found(v)}, expected ${wanted(ty, schema)}`
  switch (ty.t) {
    case 'option':
      return v.t === 'null' ? null : checkType(v, ty.of, schema)
    case 'string':
      return v.t === 'str' ? null : mismatch()
    case 'bool':
      return v.t === 'bool' ? null : mismatch()
    case 'int': {
      if (v.t !== 'num') return mismatch()
      const got = machineInt(v.raw)
      // A float-shaped literal and one past `u64`/`i64` are both a FLOAT to the
      // visitor, so both are an `invalid type` — never a range complaint.
      if (got === null) return mismatch()
      const [lo, hi] = INT_BOUNDS[ty.rust]
      // Out of range is an `invalid value`, not an `invalid type` — serde parses the
      // integer first and only then finds it does not fit.
      return got >= lo && got <= hi ? null : `invalid value: integer \`${got}\`, expected ${ty.rust}`
    }
    case 'vec': {
      if (v.t !== 'seq') return mismatch()
      for (const e of v.items) {
        const err = checkType(e, ty.of, schema)
        if (err) return err
      }
      return null
    }
    case 'map': {
      if (v.t !== 'map') return mismatch()
      for (const { node } of v.entries) {
        const err = checkType(node, ty.of, schema)
        if (err) return err
      }
      return null
    }
    default: {
      const d = schema.types.get(ty.name)
      if (v.t !== 'map') return mismatch()
      if (d.kind === 'struct') return checkFields(d.fields, v, schema)
      // Internally tagged: the tag names the variant, and the variant's own fields
      // sit beside it in the same map. serde BUFFERS the whole map looking for the
      // tag before it types anything (private::de::TaggedContentVisitor), so a
      // repeated tag is found ahead of every other fault in the map.
      const tags = v.entries.filter((e) => e.key === d.tag)
      if (tags.length > 1) return `duplicate field \`${d.tag}\``
      if (tags.length === 0) return `missing field \`${d.tag}\``
      const tag = tags[0].node
      if (tag.t !== 'str') return `invalid type: ${found(tag)}, expected variant identifier`
      const variant = d.variants.find((x) => x.wire === tag.v)
      if (!variant)
        return (
          `unknown variant \`${tag.v}\`, expected one of ` +
          d.variants.map((x) => `\`${x.wire}\``).join(', ')
        )
      return checkFields(variant.fields, v, schema)
    }
  }
}

/**
 * `null`, or the message serde would have produced for `doc` as `schema.root`.
 * `doc` is a [`readJson`] document — the SOURCE TEXT, never `JSON.parse` output;
 * see this file's header for the three things that distinction is load-bearing for.
 */
export function checkAgainstSchema(doc, schema) {
  return checkType(doc, { t: 'named', name: schema.root }, schema)
}

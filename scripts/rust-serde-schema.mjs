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

/** Inclusive bounds per Rust integer width. */
const INT_BOUNDS = {
  u8: [0, 255],
  u16: [0, 65535],
  u32: [0, 4294967295],
  u64: [0, 18446744073709551615],
  i8: [-128, 127],
  i16: [-32768, 32767],
  i32: [-2147483648, 2147483647],
  i64: [-9223372036854775808, 9223372036854775807],
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

/** How serde_json names the JSON value it actually found. */
function found(v) {
  if (v === null) return 'null'
  if (Array.isArray(v)) return 'sequence'
  switch (typeof v) {
    case 'string':
      return `string ${JSON.stringify(v)}`
    case 'boolean':
      return `boolean \`${v}\``
    case 'number':
      return Number.isInteger(v) ? `integer \`${v}\`` : `floating point \`${v}\``
    default:
      return 'map'
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

const isMap = (v) => v !== null && typeof v === 'object' && !Array.isArray(v)

/**
 * Check the declared fields of a struct (or struct enum variant) against a JSON map,
 * in serde's own order: present fields are typed as the DOCUMENT orders them, then
 * missing fields are reported in DECLARATION order. Unknown keys are ignored, exactly
 * as serde does without `deny_unknown_fields`.
 */
function checkFields(fields, v, schema) {
  const byName = new Map(fields.map((f) => [f.name, f]))
  for (const k of Object.keys(v)) {
    const f = byName.get(k)
    if (!f) continue
    const e = checkType(v[k], f.ty, schema)
    if (e) return e
  }
  for (const f of fields) if (!f.optional && !(f.name in v)) return `missing field \`${f.name}\``
  return null
}

/** `null` if `v` is a shape serde would accept for `ty`, else serde's own message. */
export function checkType(v, ty, schema) {
  const mismatch = () => `invalid type: ${found(v)}, expected ${wanted(ty, schema)}`
  switch (ty.t) {
    case 'option':
      return v === null ? null : checkType(v, ty.of, schema)
    case 'string':
      return typeof v === 'string' ? null : mismatch()
    case 'bool':
      return typeof v === 'boolean' ? null : mismatch()
    case 'int': {
      if (typeof v !== 'number' || !Number.isInteger(v)) return mismatch()
      const [lo, hi] = INT_BOUNDS[ty.rust]
      // Out of range is an `invalid value`, not an `invalid type` — serde parses the
      // integer first and only then finds it does not fit.
      return v >= lo && v <= hi
        ? null
        : `invalid value: integer \`${v}\`, expected ${ty.rust}`
    }
    case 'vec': {
      if (!Array.isArray(v)) return mismatch()
      for (const e of v) {
        const err = checkType(e, ty.of, schema)
        if (err) return err
      }
      return null
    }
    case 'map': {
      if (!isMap(v)) return mismatch()
      for (const k of Object.keys(v)) {
        const err = checkType(v[k], ty.of, schema)
        if (err) return err
      }
      return null
    }
    default: {
      const d = schema.types.get(ty.name)
      if (!isMap(v)) return mismatch()
      if (d.kind === 'struct') return checkFields(d.fields, v, schema)
      // Internally tagged: the tag names the variant, and the variant's own fields
      // sit beside it in the same map.
      if (!(d.tag in v)) return `missing field \`${d.tag}\``
      const tag = v[d.tag]
      if (typeof tag !== 'string')
        return `invalid type: ${found(tag)}, expected variant identifier`
      const variant = d.variants.find((x) => x.wire === tag)
      if (!variant)
        return (
          `unknown variant \`${tag}\`, expected one of ` +
          d.variants.map((x) => `\`${x.wire}\``).join(', ')
        )
      return checkFields(variant.fields, v, schema)
    }
  }
}

/** `null`, or the message serde would have produced for `value` as `schema.root`. */
export function checkAgainstSchema(value, schema) {
  return checkType(value, { t: 'named', name: schema.root }, schema)
}

# sim-codecs

sim-codecs gives you SIM's read/write layer: a set of codec libraries that
decode text or bytes -- Lisp, JSON, binary, Algol, configuration text, and more
-- into one shared expression graph, and encode that graph back out again.

## Example

Add one concrete codec crate:

```bash
cargo add sim-codec-lisp
```

Register the codec, decode s-expression text into an `Expr`, then encode the
`Expr` back to Lisp text -- a semantic round-trip:

```rust
use std::sync::Arc;
use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_codec_lisp::LispCodecLib;
use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};

let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
sim_test_support::register_core_classes(&mut cx);
sim_test_support::register_f64_number_domain(&mut cx);

let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id())?;
cx.load_lib(&lib)?;
let lisp = Symbol::qualified("codec", "lisp");

// Decode text into a checked `Expr` form.
let expr = decode_with_codec(
    &mut cx,
    &lisp,
    Input::Text("(quote [1 2])".to_owned()),
    ReadPolicy::default(),
)?;
assert!(matches!(expr, Expr::Quote { .. }));

// Encode the `Expr` back to Lisp text.
let text = encode_with_codec(&mut cx, &lisp, &expr, Default::default())?
    .into_text()
    .unwrap();
assert_eq!(text, "(quote [1 2])");
# Ok::<(), sim_kernel::Error>(())
```

This is the crate-root doctest of `sim-codec-lisp`
(`crates/sim-codec-lisp/src/lib.rs:27`); it compiles and passes as written.

## How it works

SIM is an expandable Rust runtime built around a small protocol kernel plus a
large set of loadable libraries: the kernel defines contracts, libraries provide
behavior. This repository provides the libraries that move data across the
runtime boundary.

The full data flow of the runtime is:

```text
tokens -> checked forms -> objects -> checked calls -> objects -> encoded forms
```

The first and last legs of that flow are codecs. A codec turns external bytes
or text into the shared `Expr` graph on the way in, and turns `Expr` (or
runtime `Value`) back into bytes or text on the way out. Lisp is one codec
here, not the identity of the system: SIM presents many codec surfaces over a
single shared expression model.

The kernel types this repository builds on (`Expr`, `Value`, `Symbol`,
`Shape`, `Origin`, `Cx`, `ReadPolicy`, `EncodeOptions`, `EncodePosition`,
`LocatedExpr`, `LocatedExprTree`) are defined in `sim-kernel`. This README is
self-contained, but those protocol types are the kernel's, not this repo's.

## Crates

| Crate | Role |
| --- | --- |
| `sim-codec` | The codec protocol and runtime: `Decoder`/`Encoder` traits, the `CodecRuntime` object, the shared `decode_*`/`encode_*` helper API, decode limits, and shared string-literal and domain-form support. Concrete codecs depend on this; it depends on no concrete codec. |
| `sim-codec-lisp` | `codec:lisp`, a general-purpose expression codec over s-expression text, plus the `cli/main/codec-lisp` loaded CLI entrypoint for Lisp eval, script, stdin, and bare REPL handoff. |
| `sim-codec-json` | `codec:json`, a general-purpose expression codec over tagged JSON, plus JSON projection and shape-to-JSON-schema helpers. |
| `sim-codec-binary` | `codec:binary`, a general-purpose expression codec over the versioned `SLB8` binary frame format. |
| `sim-codec-binary-base64` | `codec:binary-base64`, a general-purpose expression codec that wraps the `SLB8` frame in ASCII Base64 text (`.simb64`). |
| `sim-codec-bitwise` | `codec:bitwise`, a general-purpose expression codec over a canonical, minimal, bit-packed self-delimiting frame -- the canonical/minimal sibling of `codec:binary`, and the smallest canonical byte string for an `Expr` value. It also offers an opt-in dense structural-sharing mode. |
| `sim-codec-bitwise-base64` | `codec:bitwise-base64`, a general-purpose expression codec that wraps the bitwise frame in ASCII Base64 text -- the text-transport analog of `codec:bitwise`, sharing one base64 implementation with `sim-codec-binary-base64`. |
| `sim-codec-algol` | `codec:algol`, a general-purpose expression codec over an Algol-style infix surface with registered Pratt operators. |
| `sim-codec-pratt` | Shared Pratt parser substrate for codec lexers: a token-source driven precedence parser that builds located `Expr` trees while preserving spans, trivia, calls, and prefix, infix, and postfix operators. |
| `sim-codec-chat` | `codec:chat`, a domain codec for provider-neutral model transcripts, with native OpenAI, Anthropic, Ollama, LM Studio, and Lemonade projection helpers. |
| `sim-codec-config` | `codec:config`, a config codec that decodes per-library settings as one table map and shared launcher files as a directory of library id to table map. |
| `sim-codec-doc` | `codec:doc`, a document codec that reads and writes Markdown, Typst, AsciiDoc, and LaTeX through one semantic markup value, catalogs tracked formats that fail closed until implemented, and provides fixed, recursive, and heading-aware chunk operations. |
| `sim-codec-mcp` | `codec:mcp`, a domain codec for one MCP JSON-RPC 2.0 envelope per frame. |
| `sim-wasm-abi` | The wasm guest ABI: byte-frame value/manifest/exports transport and a wasm-backed `Lib`. This is the plugin ABI surface, not an expression codec. |
| `sim-test-support` | Shared test harness (`install_core_runtime`-style `Cx`, `roundtrip` helper). Used only as a `dev-dependency`; depends only on `sim-kernel`, `sim-value`, and `sim-codec`, so it forms no dependency cycle. |

## The codec contract

### Codecs are first-class runtime objects

A codec is not a free function. It is a runtime object (`CodecRuntime`)
registered under a codec symbol (for example `codec:lisp`) by a codec `Lib`,
resolvable through the runtime `Cx`, and described by browse metadata. Each
codec carries an `expr_shape` and an `options_shape` so that callers can ask
the runtime what a codec accepts rather than reading static documentation.

### Decoders and encoders are split

Decode and encode are separate, independently optional capabilities. A
`CodecRuntime` holds up to six slots:

- `Decoder` / `LocatedDecoder` / `TreeDecoder`
- `Encoder` / `LocatedEncoder` / `TreeEncoder`

The plain decoder/encoder operate on `Expr`. The located and tree variants
operate on `LocatedExpr` and `LocatedExprTree`, the origin-aware forms whose
`Origin` carries codec id, source id, byte span, and leading trivia. A codec
may implement any subset; the shared helper API smooths over the gaps (see
[Fallback contract](#fallback-contract)).

### Encoders know their output position

Encoding is parameterized by `EncodeOptions`, whose `position`
(`EncodePosition`: `Eval`, `Quote`, `Data`, `Pattern`) tells the encoder where
its output will land. Position changes what a codec is allowed to emit. For
example, a constructor-shaped object encodes as `#(Class ...)` in quote
position, as `(Class ...)` in eval position, and falls back to `(object ...)`
otherwise. Decode mirrors this with `DecodePosition`.

`EncodeOptions` also carries:

- `canonical`: emit the codec's stable canonical surface, versus a best-effort
  `preserve-input` pass that never overrides codec grammar limits.
- `lossless_origin`: request origin/trivia retention. This only binds for
  codecs with located/tree encoder support.
- `read_construct` / `read_eval`: govern whether constructor forms and
  explicitly declared read-eval forms may be emitted.

### Two codec classes

**General-purpose expression codecs** (`codec:lisp`, `codec:json`,
`codec:binary`, `codec:binary-base64`, `codec:algol`) are expected to cover the
whole shared `Expr` graph. Variants that have no native surface syntax in a
given codec round-trip through explicit escape or tagged forms (for example
Lisp's `expr:map` / `expr:set` / `expr:call` / `expr:infix` forms, or JSON's
`"$expr"` discriminator) rather than being dropped.

**Domain codecs** (`codec:chat`, `codec:mcp`) round-trip only their own domain.
They still pass through the shared `Expr` model, but they validate and **fail
closed** on any expression outside their accepted shape, instead of claiming
total coverage.

**Configuration codecs** (`codec:config`) decode settings into `Expr::Map`
values. A per-library config file decodes as one table; a shared launcher file
decodes as a directory whose keys are library ids and whose values are tables.
Callers that accept configuration maps can also use `codec:lisp` or
`codec:json` when those inputs decode to a map.

### The shared `Expr` model and canonical equality

All codecs round-trip through the shared `Expr` graph defined in `sim-kernel`.
Its variants are `Nil`, `Bool`, `Number`, `Symbol`, `Local`, `String`,
`Bytes`, `List`, `Vector`, `Map`, `Set`, `Call`, `Infix`, `Prefix`, `Postfix`,
`Block`, `Quote`, `Annotated`, and `Extension`.

The cross-codec contract is **structural** equality (`Expr::canonical_eq`), not
byte-for-byte source identity and not runtime-`Value` identity (which is
object-identity based). Canonical JSON and canonical binary normalize map-entry
order and set-member order by canonical key, so insertion order does not affect
canonical equality.

Numbers are domain-tagged at the expression level: `Expr::Number` carries a
`NumberLiteral { domain, canonical }`. Text codecs do not emit an undecided raw
number token; the Lisp and Algol readers call into the installed
number-domain registry (`cx.parse_number_literal(...)`), the first domain
accepting the text by `parse_priority()` wins, and the chosen domain is stored
in the expression. Decode behavior therefore depends on the number-domain
libraries loaded into the runtime, by design.

### The helper API

The public round-trip surface lives in `sim-codec`. Every entry point resolves
a codec by symbol through the `Cx` and applies its `ReadPolicy` / `EncodeOptions`:

```rust
decode_with_codec(cx, symbol, input, read_policy) -> Result<Expr>
decode_with_codec_and_limits(cx, symbol, input, read_policy, limits) -> Result<Expr>
decode_located_with_codec(cx, symbol, input, read_policy, source_id) -> Result<LocatedExpr>
decode_tree_with_codec(cx, symbol, input, read_policy, source_id) -> Result<LocatedExprTree>

encode_with_codec(cx, symbol, expr, options) -> Result<Output>
encode_located_with_codec(cx, symbol, expr, options) -> Result<Output>
encode_tree_with_codec(cx, symbol, expr, options) -> Result<Output>
encode_value_with_codec(cx, symbol, value, options) -> Result<Output>
```

`Input` is text or bytes; `Output` is text or bytes. Decoding is
resource-limited through `DecodeLimits` to reject malformed or hostile input
with errors rather than panics or unbounded work. `encode_value_with_codec`
bridges runtime `Value`s back to a codec by converting them to `Expr` first
(for example table and list backends lower through their `as_table`/`as_list`
forms), so value-based round-trip preserves runtime identity while reusing the
shared expression machinery.

### Capability gating is part of decode

Read-construct is a decode-time behavior. Read-eval is explicit diminished eval:
the ordinary decode path stays inert, and hosts that accept eval-shaped holes
route those requests through the runtime read-eval broker.

- `#(...)` read-construct decodes constructor arguments as data and builds a
  runtime object. It requires `read-construct` in the `ReadPolicy` **and** the
  matching capability at runtime (`Cx::read_construct`).
- Explicit read-eval forms (`#eval(...)`, `#.expr`) are decode-only,
  capability-gated, and additionally require a trusted `ReadPolicy`: an
  untrusted policy denies them even when the capability is present. Surfaces
  that admit them use a declared result shape and diminished allowed
  capabilities. The canonical Lisp encoder never emits them by default.

Capability checks use the same `CapabilityName` model across `ReadPolicy`,
`Cx::require`, loader manifests, and eval requests; capability names are
ordinary runtime data, not leaked static strings.

### Fallback contract

The runtime presents plain, located, and tree entry points for every codec and
fills gaps uniformly:

- A codec with no located decoder falls back to plain decode with `origin: None`.
- A codec with no tree decoder falls back to recursive reconstruction from the
  decoded `Expr`.
- Located/tree **encode** uses specialized encoders only when
  `EncodeOptions.lossless_origin` is `true`; otherwise it falls back to plain
  encode of the underlying `Expr`.

Origin retention is intentionally uneven: JSON, binary, and binary-base64 carry
nested origin payloads losslessly, while Lisp and Algol preserve parser-native
spans and leading trivia on a best-effort basis. The helper API smooths the
call shape without pretending the guarantees are equal.

## Canonical surfaces

### Lisp (`codec:lisp`)

S-expression text tokenized through `proc_macro2`, parsed to one top-level
expression.

- Atoms: `nil` (the `core:Nil` atom, not a list), `true`, `false`, strings,
  byte strings, symbols, and number literals accepted by the active
  number-domain registry.
- Logic locals: `?name` decodes to `Expr::Local`.
- Lists `(...)` (with `()` the empty runtime list), vectors `[...]`, blocks
  `{...}`.
- Canonical quote forms prefer explicit list heads: `(quote ...)`,
  `(quasiquote ...)`, `(unquote ...)`, `(splice ...)`, `(syntax ...)`.
- Non-native `Expr` variants escape through `expr:map`, `expr:set`,
  `expr:call`, `expr:infix`, `expr:prefix`, `expr:postfix`, `expr:annotated`,
  `expr:extension`.
- `#(Class ...)` read-construct (capability-gated, see above). Common shapes
  promoted to runtime values have canonical read-construct forms, for example
  `#(core/AnyShape)`, `#(core/ClassShape core/String)`, and
  `#(core/ListShape (...))`; decoded shapes rebuild equivalent values that
  expose `as_shape()` and `as_callable()` (pointer identity is not preserved).
- `#eval(...)` and `#.expr` are explicit read-eval forms. They are never emitted
  by the canonical encoder under default options.

### JSON (`codec:json`)

Tagged JSON object with a required `"$expr"` discriminator.

- Logic locals: `{"$expr":"local","name":"x"}`.
- Numbers: tagged number objects carrying domain symbol and canonical literal
  string.
- Maps and sets encode in canonical order.
- One of the stronger lossless-origin codecs: plain, located, tree, and nested
  origin payload decode are all supported.

### Binary (`codec:binary`)

The versioned `SLB8` frame format.

- Logic locals use a dedicated `Local` tag and the shared symbol table.
- Frames intern lib names, symbols, and number-domain symbols through frame
  tables.
- Numbers carry domain identity plus canonical literal string or canonical
  binary payload.
- Nested origin payloads ride in frame flags.
- Decode is resource-limited and rejects malformed lengths, tables, and trees
  with errors rather than panicking. The other stronger lossless-origin codec.

### Binary-Base64 (`codec:binary-base64`)

Standard padded Base64 text wrapping the same `SLB8` bytes as `codec:binary`,
extension `.simb64`. Canonical output is unwrapped ASCII text with no trailing
newline; decode ignores ASCII whitespace, then delegates to the binary frame
decoder. It is a text wrapper, not a new frame format, and inherits the binary
codec's origin retention.

### Algol (`codec:algol`)

An Algol-style infix surface covering arithmetic, identifiers, literals, calls,
and registered Pratt operators.

- Unsupported `Expr` variants escape through `expr.lisp(...)`.
- String escaping uses the shared string-literal codec.
- Tree decode preserves parser-native spans and best-effort comments; exact
  trivia round-trip is not part of the canonical contract.

### Chat (`codec:chat`) -- domain codec

A provider-neutral model transcript codec. Canonical text framing starts with
the `SIMCHAT1` header. Accepted values are transcript-shaped `Expr::Map` values
for model requests, responses, events, and cards. Encode and decode both run
`validate_chat_transcript`; non-transcript values fail validation.
OpenAI-compatible and Ollama helpers project provider request/response
envelopes into and out of the same transcript shapes. Chat is not a total codec
for arbitrary `Expr`.

### MCP (`codec:mcp`) -- domain codec

An MCP JSON-RPC 2.0 envelope codec. Exactly one request, notification,
response, or error envelope per frame; JSON-RPC batch arrays are rejected.
Canonical output is compact JSON with `jsonrpc` set to `"2.0"`. The in-process
surface is an `Expr::Map` with an `mcp` version field plus the envelope fields
`id`, `method`, `params`, `result`, or `error`. Request ids round-trip as
strings, numbers, or `nil`. `params`, `result`, and error `data` reuse the
tagged JSON expression mapping from `codec:json`. Unknown envelope fields,
duplicate envelope fields, duplicate structured error fields, invalid field
combinations, unsupported versions, malformed ids, and non-envelope expressions
fail closed. Duplicate policy inside tagged expression payloads belongs to
`codec:json`. MCP is not a total codec for arbitrary `Expr`.

## What round-trip guarantees

The strongest cross-codec guarantee, asserted by the codec test matrix, is:

- Malformed input returns errors instead of panicking.
- Every general-purpose codec round-trips the shared expression corpus by
  `Expr::canonical_eq`.
- Cross-codec transcodes preserve canonical semantics.
- Repeated encode of the same `Expr` is stable.
- Canonical JSON and canonical binary ignore map/set insertion order.
- Codec metadata points at specific shape objects rather than `core/Any`.
- Table and list backends round-trip through every general-purpose codec.
- Registry catalog snapshots round-trip through Lisp, JSON, binary, and
  binary-base64, keeping live host payloads unresolved.
- Value-based round-trip preserves identity through `encode_value_with_codec`.

Domain codecs are tested against their accepted domains: `codec:chat` for
model transcript maps and `codec:mcp` for one JSON-RPC envelope per frame. This
is stronger than a compile-only claim and weaker than a byte-exact source claim.

## Validation

This repo builds standalone against the published SIM crates on crates.io.
The validation gates are:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo run -p xtask -- check-file-sizes
cargo run -p xtask -- simdoc --check
```

The simdoc launcher uses `../sim-tooling` in a constellation checkout. In a
lone checkout, set `SIMDOC_TOOLING_MANIFEST` to the checked-out
`sim-tooling/Cargo.toml`; CI checks out `sim-tooling` and sets that variable.

## Documentation lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagrams: `docs/diagrams/src/` and `docs/diagrams/generated/`

The same command writes split contract files under `docs/generated/`.
Everything under `docs/` is generated and must not be hand-edited.

### Rustdoc conventions

Public API documentation in `src/` follows one house style:

- Every public item opens with a one-line summary sentence, then context.
- Each codec is framed by its kind: a decoder turns tokens/text into checked
  forms; an encoder knows its output position (eval, quote, data, pattern).
  General-purpose codecs round-trip every expression through the shared `Expr`
  graph; domain codecs round-trip only their domain and fail closed outside it.
- The first-reach types carry a `# Examples` doctest that compiles and passes.
- Cross-reference with intra-doc links, and link back to this README rather than
  restating it.

The public API is documentation-gated: each crate's `lib.rs` denies
`missing_docs`, so every public item, field, and variant must be documented for
the crate to build.

### Examples and recipes

Every codec crate ships runnable recipes under its `recipes/` directory. The two
non-codec crates do not: `sim-test-support` is a dev-dependency test helper, and
`sim-wasm-abi` is the wasm ABI transport. Their examples are their rustdoc
doctests.

# COMPOSE3 JSON reuse ledger

This note freezes the source boundary read for COMPOSE3.00. It describes the
current code; it does not import the retired COMPOSE1 migration assumptions.

## Guest calls into `sim-codec-json`

`sim-runtime/crates/sim-lib-lang-javascript/src/json.rs` already uses the
canonical codec projection at these exact seams:

- `parse_javascript_json` calls `project_json_to_expr` with
  `JsonProjectionMode::UntaggedInterop`, then calls `project_expr_to_json` with
  the same mode before applying the JavaScript reviver walk.
- `stringify_javascript_json` calls `project_json_to_expr` with
  `JsonProjectionMode::UntaggedInterop`, then calls `project_expr_to_json` with
  the same mode before `serde_json::to_string` renders the result.

The called functions live in
`crates/sim-codec-json/src/projection.rs`. `JsonCodec::decode`,
`JsonCodec::encode`, `JsonCodec::decode_located`, `JsonCodec::encode_located`,
`JsonCodec::decode_tree`, and `JsonCodec::encode_tree` in `codec.rs` remain the
tagged codec surfaces. `json_to_located_expr`, `located_expr_to_json`,
`json_to_tree`, and `tree_to_json` in `tree_json.rs` remain the origin-aware
surfaces. The JavaScript guest does not call those tagged or located/tree
surfaces.

## Retired claims checked against current source

The COMPOSE1 inventory claim that `json.rs` depends only on standard
collections and declares a model without composing `sim-codec-json` is stale.
The calls above are direct source evidence that the guest already consumes the
codec's untagged projection in both directions. The private
`JavascriptJsonValue` still exists, but it is JavaScript policy around that
projection rather than evidence that the codec is unused.

The corresponding COMPOSE1 collection claim is still accurate:
`collections.rs` imports `JavascriptValue` and `BTreeMap`, stores arrays, maps,
sets, symbols, and iterators locally, and does not call
`sim-lib-sequence/src/persistent.rs`. The sequence owner currently offers
persistent vectors, lists, sets, and symbol-keyed maps, but its existing
operations do not directly express JavaScript holes, arbitrary SameValueZero
keys, or live mutation-aware iteration.

## Frozen boundary

The canonical codec owns JSON/Expr projection, tagged expression encoding,
located/tree encoding, parse limits, and JSON rendering. JavaScript continues
to own reviver, replacer, `toJSON`, undefined policy, property ordering, and
its public error identities. The scenario ledger in
`sim-runtime/fixtures/compose3-json-collections.toml` is the exact behavioral
baseline. Its content identities cover outcomes, not this explanatory prose.

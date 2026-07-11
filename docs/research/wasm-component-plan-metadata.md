# Wasm Component plan metadata research

## Scope

This note answers issue #79’s question for Wasvy’s **Module Plan** as artifact metadata on a final **WebAssembly Component**. It separates **verified facts**, **recommendations**, and **unresolved uncertainties**.

---

## Verified facts

### 1) Top-level custom sections are valid in components

- The component-model binary format’s top-level grammar explicitly allows `section_0(<core:custom>)` in a component, alongside core-module, nested-component, import, export, and other component sections.[^cmp-binary]
- The component format reuses the core custom-section payload format. In core WebAssembly, a custom section is section id `0` and its contents are `name` followed by arbitrary bytes.[^core-custom]
- Current Bytecode Alliance encoding APIs model the same rule directly: `wasm_encoder::ComponentSectionId::CoreCustom = 0`, `CustomSection` and `RawCustomSection` both implement `ComponentSection`, and component custom sections use the same name+bytes encoding as core custom sections.[^encoder-component][^encoder-custom]
- For components, current `wasm-encoder` explicitly documents that sections “may appear in any order and may be repeated”. So a top-level component custom section is not restricted to one fixed slot; appending it at the end is valid.[^encoder-component]
- The component-model explainer defines a component `name` custom section (`component-name`) as a component custom section and says engines should not reject a component with an invalid one. That is extra evidence that top-level component custom sections are part of the model, not a core-module-only escape hatch.[^cmp-name]

### 2) Top-level component custom sections are already used by Bytecode Alliance tooling

- `wit-parser` documents that the current component binary format has no inline place for docs/stability metadata, so it stores that information in a custom section **within the component** that encodes a WIT package.[^wit-parser-metadata]
- `wit-parser` decoding enforces its own application-level rule for that component-level custom section: if more than one `package-docs` section is found, decoding fails. This shows that duplicate-checking and schema validation are tool responsibilities, not WebAssembly validation responsibilities.[^wit-parser-decoding]

### 3) `component-type*` metadata today lives in core-module custom sections, not final-component top-level metadata

- `wit-component`’s documented workflow is: language bindings embed `component-type*` custom sections into the **core wasm module**; those sections are carried through core-wasm linking and then consumed during component creation.[^wit-readme][^cmp-linking]
- The helper `embed_component_metadata` writes a core custom section named exactly `component-type`.[^wit-lib]
- `wit-component::metadata::decode` consumes every custom section whose name starts with `component-type`, merges its WIT metadata, and rebuilds a replacement core module **without** those sections.[^wit-metadata]
- `ComponentEncoder::module` uses that decode path, and `EncodingState::encode_core_modules` then embeds the resulting core module bytes into the final component as a **core module section** (`section_1`), not as a top-level component custom section.[^wit-encoding][^wit-encode-core]
- Therefore: if Wasvy stores its Plan only in a guest-produced core-module custom section, that Plan will remain nested inside the embedded core module (or be stripped if Wasvy chooses a consumed prefix like `component-type*`); it will **not** become top-level component metadata automatically.[^wit-metadata][^wit-encode-core]

### 4) How to read a top-level Wasvy Plan from a final component

- `wasmparser::Payload::CustomSection` is produced for both modules and components.[^parser-payload]
- `CustomSectionReader` exposes the section `name()` and raw `data()` bytes.[^custom-reader]
- Existing Bytecode Alliance code reads only outer-component custom sections by tracking nesting depth (`ModuleSection` / `ComponentSection` increase depth; `End` decreases it). `wasm_metadata::Producers::from_wasm` and `rewrite_wasm` both use that pattern and intentionally ignore nested module/component sections when operating on outer metadata.[^producers-from-wasm][^metadata-rewrite]

### 5) How to attach a Wasvy Plan to the final component

- `wasm-encoder::ComponentBuilder::custom_section` / `raw_custom_section` append a top-level component custom section.[^component-builder]
- Generic component re-encoding in `wasm-encoder` preserves unknown component custom sections: `parse_component_custom_section` re-emits `ComponentName` specially, and otherwise re-emits the original custom section.[^reencode-component]
- `wasm-metadata::rewrite_wasm` shows a preservation-oriented pattern for outer metadata rewrites: rewrite only recognized outer custom sections, copy all other sections through as raw bytes, and append missing outer metadata sections at the end.[^metadata-rewrite]

### 6) Validation does not validate Wasvy Plan semantics

- `wasmparser` validation explicitly does nothing for `CustomSection { .. }` payloads.[^validator]
- The core spec also says custom sections are ignored by WebAssembly semantics, and errors in interpreted custom-section data or placement must not invalidate an otherwise valid module.[^core-custom]
- So `wasm-tools validate` can tell Wasvy whether the component is structurally valid WebAssembly, but it will **not** prove that a Wasvy Plan section exists, is unique, has the right schema version, or matches the artifact body.[^validator]

### 7) Current wasm-tools behavior across parsing, encoding, stripping, optimization, and printing

#### Parsing / re-encoding

- `wasmparser` parses component custom sections, and `wasm-encoder`’s generic component reencoder preserves unknown ones.[^parser-payload][^reencode-component]

#### Componentization (`wasm-tools component new` / `wit-component`)

- `component new` consumes `component-type*` metadata from the input core module, strips those sections from the embedded core module, merges producers metadata, and synthesizes a new component that also appends component names and a top-level producers section.[^wit-metadata][^wit-encoding]
- Unknown core-module custom sections that are **not** `component-type*` are copied into the rebuilt core module and then embedded in the component’s nested core module section.[^wit-metadata][^wit-encode-core]
- Componentization therefore preserves unknown guest custom sections **inside the nested core module**, but does not hoist them to top-level component metadata.[^wit-metadata][^wit-encode-core]

#### Validation

- Validation ignores custom sections entirely beyond ordinary binary parsing.[^validator]

#### Metadata rewriting

- `wasm-metadata` rewrites only outermost custom sections it understands (`name`, `component-name`, `producers`, and OCI-style metadata when enabled); all other sections are copied through unchanged. If a requested outer metadata section is missing, it is appended at the end.[^metadata-rewrite]

#### Stripping

- `wasm-tools strip` removes custom sections. By default it intends to keep `name`, `component-type`, and `dylink.0`, but the actual keep rule is `name != "name" && !name.starts_with("component-type:") && name != "dylink.0"`.[^strip]
- That means a custom section named exactly `component-type` (which `embed_component_metadata` writes) does **not** match the implemented keep-prefix rule and can be stripped by default.[^strip][^wit-lib]
- A Wasvy-specific unknown custom section will also be stripped by default unless `strip` is avoided or explicitly configured otherwise.[^strip]

#### Optimization / GC-like rewriting in current BA component tooling

- `wit-component`’s adapter GC path ignores all adapter-module custom sections except `name` and `producers`.[^gc]
- So any plan-like metadata attached to an intermediate adapter core module is not a safe artifact source of truth.[^gc]

#### Printing / text round-trip

- `wasmprinter` prints unknown custom sections as raw `@custom` blocks rather than silently dropping them.[^wasmprinter]
- Known `name` / `component-name` sections are consumed for naming and are not printed as raw custom sections.[^wasmprinter]
- `wasmprinter --name-unnamed` explicitly warns that converting the text output back to binary will change the resulting `name` custom section.[^wasmprinter]
- The `wast` component parser/encoder supports arbitrary component `@custom` sections and writes them back as `wasm_encoder::CustomSection` values, so unknown component custom sections can round-trip through the text format in principle.[^wast-component]

### 8) Top-level component custom section vs nested core-module custom section

- A final component can directly contain top-level custom sections (`section_0`) and nested core modules (`section_1`). Those are different layers in the binary format.[^cmp-binary]
- When `wit-component` embeds a core module into a component, it does so by inserting the entire core module byte stream as a **core module section**.[^component-builder][^wit-encode-core]
- Therefore a nested core-module custom section is still “inside the guest module”; host tooling must parse the outer component, find the nested core module, and then parse that inner module to read it. A top-level component custom section is immediately available at outer depth 0.[^component-builder][^producers-from-wasm]

### 9) Rust / LLVM custom-section behavior is not enough for final-component top-level metadata

- Rust’s `#[used]` only guarantees retention in the **output object file**; the linker may still remove it.[^rust-abi]
- Rust’s `#[link_section]` only places a function/static into a named **object-file** section.[^rust-abi]
- So Rust-side custom-section tricks are relevant for embedding metadata in an intermediate core module, but they do not by themselves guarantee a final **top-level component** custom section. That final attachment point exists after componentization, not during guest crate compilation.[^rust-abi][^wit-encoding]

---

## Recommendations

### Recommended Wasvy metadata placement

1. **Put the Wasvy Module Plan in exactly one top-level custom section on the final component**, not only inside the nested core module.
2. Use a **Wasvy-owned section name** that does not overlap with current BA-reserved conventions such as `component-type*`, `package-docs`, `name`, or `component-name`.
3. Treat the section payload as a **canonical byte format** owned by Wasvy (for example canonical JSON or CBOR with an explicit schema/version field). WebAssembly itself will not canonicalize it for you.

### Recommended read path

Implement `wasvy plan read` roughly as:

1. Parse the final component with `wasmparser::Parser::new(0).parse_all(...)`.[^parser-payload]
2. Track nesting depth exactly like `wasm-metadata` does.[^producers-from-wasm][^metadata-rewrite]
3. At depth 0, collect custom sections whose name matches the Wasvy Plan section name.[^custom-reader]
4. Enforce Wasvy rules yourself:
   - exactly one section;
   - recognized schema version;
   - canonical payload decoding;
   - any embedded digest claims match the artifact bytes or covered region.

### Recommended attach/finalize path

Implement a dedicated Wasvy finalizer that:

1. Starts from the already-componentized `.wasm` component.
2. Rewrites the **outer component only**:
   - preserve every existing section byte-for-byte;
   - remove any old Wasvy Plan section at depth 0;
   - append one new top-level Wasvy Plan custom section at the end.
3. Runs `wasm-tools validate` (or the same validator API) afterward.[^validator]
4. Reads the Wasvy Plan section back and byte-compares it with the intended payload.

This “copy-through + append one outer custom section” strategy matches the preservation approach used by `wasm-metadata` and minimizes byte churn outside the single Wasvy metadata section.[^metadata-rewrite]

### Recommended Wasvy CLI finalization pipeline

1. **Build guest core module(s)**.
2. If WIT metadata is not already embedded, run **`wasm-tools component embed`** or equivalent guest tooling so `component-type*` exists in the core module.[^wit-readme][^wit-lib]
3. **Do not run default `wasm-tools strip` after that point**; it can remove both exact `component-type` metadata and any future Wasvy custom section.[^strip][^wit-lib]
4. Run **`wasm-tools component new`** to produce the final component.[^wit-encoding]
5. Run any **outer metadata rewrite** (`wasm-tools metadata add`, producer/name stamping, etc.) **before** attaching the Wasvy Plan, because these steps can rewrite or append outer custom sections and therefore change bytes.[^metadata-rewrite]
6. Run **`wasvy finalize`** to attach/replace the single top-level Wasvy Plan section.
7. Run **structural validation** and **Wasvy-specific Plan validation**.
8. Compute the **Module Artifact ID from the exact final bytes**.
9. Sign **after** finalization. Prefer a **detached signature** unless Wasvy also defines a precise embedded-signature exclusion/coverage scheme.

### Canonicalization / reproducibility / Artifact ID implications

- Current sources do **not** provide a general component canonicalization pass. Current BA tools can legitimately rewrite bytes by stripping sections, merging/re-emitting producers, adding names, consuming `component-type*`, and printing/reparsing text.[^wit-metadata][^metadata-rewrite][^strip][^wasmprinter]
- Therefore Wasvy should treat the **final emitted component bytes** as the artifact identity surface. Hashing the pre-finalization core module, a parsed AST, or a pre-Plan component is not stable enough.
- If the Plan payload itself contains content-derived claims, compute them over an explicitly defined coverage set. The simplest rule is: **Artifact ID = hash(final component bytes after Plan insertion, before any detached signature packaging)**.

### Signing-order implications

- If Wasvy wants the signature to cover the Plan, the Plan must be attached **before** signing.
- If Wasvy wants the Artifact ID to identify the signed bytes exactly, compute it **after every byte-changing transformation** and either use detached signatures or define an embedded-signature exclusion rule.
- Without a Wasvy-defined exclusion/canonicalization rule, an embedded signature section creates a self-reference problem and weakens reproducibility.

### Why the Plan should be top-level, not nested

- Wasvy’s Plan is the **artifact source of truth** for the final component artifact, not just for one embedded guest module.
- Top-level placement makes it readable without descending into nested core modules, avoids conflating it with guest-compiler metadata, and keeps it outside tool paths that intentionally consume or ignore specific inner-module custom sections (`component-type*`, adapter GC, guest linkers, etc.).[^wit-metadata][^gc]

---

## Failure modes and validation requirements

### Failure modes

- **Malformed custom section encoding**: the component will fail to parse structurally before Wasvy logic runs.[^core-custom][^custom-reader]
- **Missing Wasvy Plan section**: WebAssembly validation still succeeds; Wasvy must reject the artifact itself.[^validator]
- **Duplicate Wasvy Plan sections**: WebAssembly validation still succeeds; Wasvy must detect and reject duplicates, like `wit-parser` does for `package-docs`.[^wit-parser-decoding]
- **Wrong nesting level**: a Plan embedded only in the nested core module is easy to miss and is not equivalent to final-component top-level metadata.[^wit-encode-core][^producers-from-wasm]
- **Post-finalization strip/rewrite**: `strip`, metadata rewrite, text round-trip, or other byte-changing tools can invalidate the Plan digest, Artifact ID, or signature coverage.[^strip][^metadata-rewrite][^wasmprinter]
- **Intermediate adapter/module attachment**: metadata attached to adapter modules can be dropped by current GC-like rewriting paths.[^gc]

### Wasvy-specific validation requirements

Wasvy should enforce all of these itself:

1. exactly one top-level Plan section;
2. expected section name;
3. schema/version compatibility;
4. canonical payload decoding;
5. any embedded digest claims match the intended coverage;
6. Plan identity fields agree with the CLI inputs / build graph;
7. no later pipeline step mutates bytes after Artifact ID/signature generation.

---

## Unresolved uncertainties

1. **Formal standard status**: the component binary references used here are current proposal/specification sources, but the checked binary-format text comes from the component-model proposal/explainer rather than a separate finalized 1.0 formal component binary spec artifact.[^cmp-binary]
2. **Standard component signing**: this research did not find a WebAssembly-component-specific signing/canonicalization standard in the requested primary sources. If Wasvy wants embedded signatures, it likely needs either a Wasvy-defined scheme or a later-adopted external standard.
3. **Non-BA post-processing tools**: this note intentionally did not evaluate secondary tooling outside the requested source set (for example Binaryen/`wasm-opt`), so no claims are made here about their custom-section preservation behavior.

---

## Bottom line

Top-level custom sections **are valid in WebAssembly Components** and use the ordinary custom-section encoding (`id = 0`, `name`, raw bytes). Current Bytecode Alliance tooling already uses component-level custom sections for some metadata, but `component new` consumes only `component-type*` from **inner core modules** and does not hoist arbitrary guest metadata to the component top level. For Wasvy, the safest design is:

- build the component first;
- attach one **top-level** Wasvy Plan section in a dedicated finalizer;
- validate structurally and semantically;
- compute Artifact ID from the **exact final bytes**;
- sign only after finalization.

---

[^core-custom]: WebAssembly core spec, custom sections and module section ordering: <https://webassembly.github.io/spec/core/binary/modules.html#custom-section>, <https://webassembly.github.io/spec/core/binary/modules.html#binary-module>.
[^cmp-binary]: Component model binary explainer, component top-level grammar: <https://github.com/WebAssembly/component-model/blob/92263d0d670dd3c887b2fe648b81608268f176f3/design/mvp/Binary.md#L23-L40>.
[^cmp-name]: Component model binary explainer, component `name` custom section: <https://github.com/WebAssembly/component-model/blob/92263d0d670dd3c887b2fe648b81608268f176f3/design/mvp/Binary.md#L505-L523>.
[^cmp-linking]: Component model linking explainer on custom sections being opaquely propagated through core-wasm build steps: <https://github.com/WebAssembly/component-model/blob/92263d0d670dd3c887b2fe648b81608268f176f3/design/mvp/Linking.md#L61-L65>.
[^encoder-component]: `wasm-encoder` component section ids and ordering note: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/component.rs#L40-L64>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/component.rs#L101-L106>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/component.rs#L153-L160>.
[^encoder-custom]: `wasm-encoder` core `CustomSection` encoding: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/core/custom.rs#L5-L26>.
[^component-builder]: `wasm-encoder` component builder APIs for raw/custom sections and core-module embedding: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/component/builder.rs#L153-L180>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/component/builder.rs#L751-L760>.
[^parser-payload]: `wasmparser::Payload::CustomSection` applies to “A module or component custom section”: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmparser/src/parser.rs#L333-L334>.
[^custom-reader]: `CustomSectionReader` accessors for `name()` and `data()`: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmparser/src/readers/core/custom.rs#L12-L33>.
[^validator]: `wasmparser` validator ignores custom sections: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmparser/src/validator.rs#L641-L644>.
[^reencode-component]: Generic component reencoding preserves unknown custom sections: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-encoder/src/reencode/component.rs#L576-L588>.
[^wit-readme]: `wit-component` README on `component-type*` custom sections: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/README.md#L118-L128>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/README.md#L184-L189>.
[^wit-lib]: `embed_component_metadata` writes a `component-type` custom section: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/lib.rs#L92-L108>.
[^wit-metadata]: `wit-component::metadata` docs and decode behavior: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/metadata.rs#L1-L25>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/metadata.rs#L226-L260>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/metadata.rs#L287-L303>.
[^wit-encoding]: `ComponentEncoder::module` and final `encode` behavior: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/encoding.rs#L3161-L3188>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/encoding.rs#L3348-L3392>.
[^wit-encode-core]: `wit-component` embeds the rebuilt module as a core-module section: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/encoding.rs#L428-L433>.
[^strip]: `wasm-tools strip` docs and implemented keep rule: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/src/bin/wasm-tools/strip.rs#L6-L10>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/src/bin/wasm-tools/strip.rs#L85-L100>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/src/bin/wasm-tools/strip.rs#L138-L142>.
[^gc]: `wit-component` adapter GC ignores unknown custom sections: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-component/src/gc.rs#L317-L322>.
[^metadata-rewrite]: `wasm-metadata` outer-only rewrite and pass-through behavior: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-metadata/src/rewrite.rs#L50-L60>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-metadata/src/rewrite.rs#L168-L175>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-metadata/src/rewrite.rs#L177-L221>.
[^producers-from-wasm]: `wasm-metadata::Producers::from_wasm` only reads outer-component producers metadata: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-metadata/src/producers.rs#L37-L49>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasm-metadata/src/producers.rs#L150-L153>.
[^wit-parser-metadata]: `wit-parser` stores package metadata in a custom section within the component: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-parser/src/metadata.rs#L1-L15>.
[^wit-parser-decoding]: `wit-parser` rejects multiple `package-docs` sections: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wit-parser/src/decoding.rs#L113-L118>.
[^wasmprinter]: `wasmprinter` raw custom-section printing and `name_unnamed` caveat: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmprinter/src/lib.rs#L224-L225>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmprinter/src/lib.rs#L534-L551>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wasmprinter/src/lib.rs#L1965-L2005>.
[^wast-component]: `wast` component custom-section parse/emit support: <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wast/src/component/custom.rs#L5-L24>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wast/src/component/binary.rs#L582-L585>, <https://github.com/bytecodealliance/wasm-tools/blob/bf2ad792fcbe5e915bed7bd9a8a3be4d00ca875f/crates/wast/src/component/binary.rs#L740-L748>.
[^rust-abi]: Rust Reference on `#[used]` and `#[link_section]`: <https://doc.rust-lang.org/reference/abi.html#the-used-attribute>, <https://doc.rust-lang.org/reference/abi.html#the-link_section-attribute>.

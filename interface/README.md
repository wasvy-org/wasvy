# Wasvy's Wasm Interface

Rather than building a custom interface on top of WASM, Wasvy uses the [wasm component model](https://component-model.bytecodealliance.org/). Our interface is defined in the [WIT (Wasm Interface Type) language](https://component-model.bytecodealliance.org/design/wit.html).

The crate in this repo re-exports the definitions (defined in `./wit`) to other crates.

Centralizing the declarations prevents published host crates from embedding
different copies of the same interface.

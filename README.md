<div align="center">
<img src="assets/banner.png" alt="Wasvy banner art"/>
</div>
<p align="right">Thanks <a href="https://pencartstudio.carrd.co">AreKayHen</a> for the lovely banner art!</p>

# Wasvy - WASI Modding for [Bevy](https://bevy.org/) (Just hatched! 🪺)

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/wasvy-org/wasvy#license)
[![Crates.io](https://img.shields.io/crates/v/wasvy.svg)](https://crates.io/crates/wasvy)
[![Downloads](https://img.shields.io/crates/d/wasvy.svg)](https://crates.io/crates/wasvy)
[![Docs](https://docs.rs/wasvy/badge.svg)](https://docs.rs/wasvy)
[![Rust Version](https://img.shields.io/badge/rust-1.89.0+-blue.svg)](https://www.rust-lang.org)
[![CI](https://github.com/wasvy-org/wasvy/workflows/CI/badge.svg)](https://github.com/wasvy-org/wasvy/actions)
[![Discord](https://img.shields.io/discord/691052431525675048.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.com/channels/691052431525675048/1034543904478998539)

> 🪺 **Just hatched**: This project in in the process of stabilization. Some features are still missing and breaking API changes are likely to happen as we approach a 1.0 release. Use at your own risk!

> 🎯 **Community Feedback**: Your ideas, suggestions, and contributions are highly valued! Come and talk with us in the [Bevy Discord](https://discord.com/channels/691052431525675048/1034543904478998539). Report bugs and discuss new features on [Github](https://github.com/wasvy-org/wasvy/issues). Together, we can shape this into something great!

> 💡 **TIP**: Check out the [`justfile`](justfile) for useful commands and as a reference for common operations in the project.

## Overview

Wasvy is an experimental Bevy modding engine, enabling the execution of WebAssembly (WASM) binaries within Bevy applications. It's powered by WebAssembly System Interface (WASI), enabling full access to the Bevy ECS, network, filesystem, system clock and more!

## Features

- 🪶 Easy integration with your game (see the example below)
- 🔥 Hot reloading
- 🛠️ Devtools (soon) via [wasvy-cli](https://github.com/wasvy-org/wasvy-cli)
- 🧩 WASI support powered by [wasmtime](https://wasmtime.dev/)

## Vision

The ultimate goal of Wasvy is to create a vibrant ecosystem where:

- Game developers can easily make their games moddable with just a few lines of code
- The developer experience is fantastic (Hot reloading, live debugging tools, etc)
- Modders can write mods in their language of choice (Rust, Python, etc.)
- Performance is close to native
- Mods are sandboxed and safe to run
- The ecosystem is vibrant with shared components and tools

## Installation

Run the following command in your project directory:

```bash
cargo add wasvy
```

## Quick Start

```rust,ignore
use bevy::prelude::*;
use bevy::{DefaultPlugins, app::App};

// Get started by importing the prelude
use wasvy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Adding the [`ModloaderPlugin`] is all you need 😉
        .add_plugins(ModloaderPlugin::default())
        .add_systems(Startup, startup)
        .run();
}

/// Access the modloader's api through the Mods interface 🪄
fn startup(mut mods: Mods) {
    // Load one (or several) mods at once from the asset directory!
    mods.load("mods/my-mod.wasm");
}
```

## Creating Mods

Mods are WebAssembly binaries coded in any language of choice. We're working on a way to streamline mod creation with [wasvy-cli](https://github.com/wasvy-org/wasvy-cli), but until that hatches, you can reference the examples in the repo.

To create a WASM component that works with Wasvy, you'll need to use the WebAssembly Component Model and define your interfaces using WIT (WebAssembly Interface Types). Here's how to get started with a new mod written in rust:

### Prerequisites

1. Add the `wasm32-wasip2` target for rust

```bash
rustup target add wasm32-wasip2
```

2. Install `wkg` (WebAssembly Kit Generator from [wasm-pkg-tools](https://github.com/bytecodealliance/wasm-pkg-tools)):

```bash
cargo install wkg
```

And then configure `wkg`:

```bash
wkg config --default-registry wa.dev
```

This will make `wkg` use `wa.dev` as the default registry, which is where the latest version of `wasvy:ecs` is hosted.

### Creating a New WASM Mod

1. Create a new crate:

```bash
cargo new my-mod --lib
cd my-mod
```

2. Update your `cargo.toml`:

```toml
[package]
name = "my-mod"
version = "0.1.0"
edition = "2024"

[dependencies]
# Hassle-less codegen for wit
wit-bindgen = "0.46"

# Wasvy uses json for serializing components. We use serde instead
# of bevy_reflect since at the moment bevy-reflect bloats the wasm binary considerably
serde = "1.0"
serde_json = "1.0"

# Import any bevy-sub crates for any components you will be using
# Note: enable the serialize feature to allow using serde
bevy_transform = { version = "0.18.0", features = ["serialize"], default-features = false }
bevy_math = { version = "0.18.0", features = ["serialize"], default-features = false }

[lib]
crate-type = ["cdylib"] # necessary for wasm
```

3. Define your WIT interface in `wit/world.wit`. Here's a basic example:

```wit
package component:my-mod;

/// An example world for the component to target.
world example {
    /// This is the wit definition for wasvy's ecs access interface
    use wasvy:ecs/app.commands;

    /// An example system with a Commands system params
    export my-system: func(commands: commands);

    /// This is important.
    /// This makes it so the WASM module must implement the guest required functions by the Bevy host.
    include wasvy:ecs/guest;
}
```

4. Fetch the Wasvy WIT files from the registry:

```bash
wkg wit fetch
```

This will automatically fetch the latest version of `wasvy:ecs` from [wa.dev](https://wa.dev/wasvy:ecs) and add it to your `wit` folder. You can also copy them directly from the [source](/wit/ecs/ecs.wit).

5. Modify `my-mod/src/lib.rs` as shown:

```rust,ignore
wit_bindgen::generate!({
    path: ["./wit"],
    world: "component:my-mod/example",
    with: {
       "wasvy:ecs/app": generate,
    }
});
use wasvy::ecs::app::{App, Commands, Schedule, System};

struct MyMod;

impl Guest for MyMod {
    fn setup() {
        // This must match the system we have defined in world.wit
        // If you want more systems, just define more in wit
        let system = System::new("my-system");
        system.add_commands();

        // Register the system, similarly as you would in a Bevy App
        let app = App::new();
        app.add_systems(&Schedule::ModStartup, vec![system]);
    }

    fn my_system(commands: Commands) {
        println!("Hello from my mod!");

        commands.spawn(&[
            ("example::Component".to_string(), "{}".to_string())
        ]);
    }
}

export!(MyMod);
```

5. Build your mod:

```bash
cargo build --target wasm32-wasip2 --release
```

The resulting `.wasm` file will be in `target/wasm32-wasip2/release/my-mod.wasm`.

Hint: The binary must be in your game's assets library for it to be visible to Bevy. By default this is `assets` in the same directory as `src`. Then, make sure to load it via [Mods::load](https://docs.rs/wasvy/latest/wasvy/mods/struct.Mods.html#method.load) as shown above (e.g. `mods.load("mods/my-mod.wasm")`).

### Tips

- You can enable hot reloading for assets (including wasm files) simply by enabling Bevy's `file_watcher` feature. Try running your game with the command `cargo run --features bevy/file_watcher`
- The WIT registry at [WebAssembly Components Registry](https://wa.dev/) contains many useful interfaces

## Examples

Check out the examples directory for more detailed usage:

- [`examples/host_example`](examples/host_example): Basic example on how to use the [`ModloaderPlugin`](https://docs.rs/wasvy/latest/wasvy/plugin/struct.ModloaderPlugin.html) to enable modding with bevy.
- [`examples/simple`](examples/simple): Basic WASM component in Rust.
- [`examples/python_example`](examples/python_example): Basic WASM component in Python.

## Contributing

Contributions come in many forms, and we welcome all kinds of help! Here's how you can contribute:

### Code Contributions

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Non-Code Contributions

We're actively looking for:

- Feature suggestions and ideas
- Use case examples
- Documentation improvements
- Bug reports
- Design discussions
- Feedback on the API
- Modding workflow suggestions

You don't need to write code to help out! If you have an idea or suggestion, please:

1. Open an issue to discuss it
2. Share your thoughts in the discussions
3. Create a feature request

Please make sure to:

- Follow Rust coding standards (for code contributions)
- Add tests for new features (Not required for now, but will be appreciated)
- Update documentation as needed
- Keep the alpha status in mind when proposing changes

## Roadmap

### Phase 1: Core Integration (Current)

- [x] Basic WASM component loading
- [x] WASI support
- [x] Parallel WASM system execution
- [x] Mutable query data in systems
- [ ] [System ordering](https://github.com/wasvy-org/wasvy/issues/18)
- [ ] [Entity/Component lifecycle management](https://github.com/wasvy-org/wasvy/issues/27)
- [ ] [Panic-free error handling](https://github.com/wasvy-org/wasvy/issues/28)

### Phase 2: Enhanced Features

- [x] Hot reloading wasm files
- [ ] Seamless dev experience building mods (See [wasvy-cli](https://github.com/wasvy-org/wasvy-cli))
- [ ] [Mod permissions](https://github.com/wasvy-org/wasvy/issues/22)
- [ ] Improved rust modding experience (See [wasvy-rust](https://github.com/wasvy-org/wasvy-rust))
- [ ] Improved python modding experience
- [ ] Add more languages + examples
- [ ] Test suite
- [ ] Cross-component communication
- [ ] Performance optimizations

### Phase 3: Production Ready (1.0!)

- [ ] Comprehensive documentation
- [ ] Benchmarking suite
- [ ] Stable API

## Resources

- [WASM Component Model Documentation](https://component-model.bytecodealliance.org/introduction.html) - The best resource on WIT and how the WASM component model works in practicality.
- [Wasmtime Examples](https://github.com/bytecodealliance/wasmtime/tree/main/examples/component)
- [Component Model Examples](https://github.com/bytecodealliance/component-docs/tree/main/component-model/examples/example-host)
- [Cargo Component](https://github.com/bytecodealliance/cargo-component) - I used that for making the WASM component examples.
- [WASM Package Tools](https://github.com/bytecodealliance/wasm-pkg-tools) - How to publish and retreive WIT files for use in projects.
- [WebAssembly Coponents Registry](https://wa.dev/) - The WIT registry I'm using.
- [WASI.dev](https://wasi.dev/)

## License

This project (all code and files, excluding the assets directory) is dual-licensed under either:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

The banner is by [AreKayHen, Copyright 2026](https://pencartstudio.carrd.co).

## Your contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Acknowledgments

- [Bevy](https://bevyengine.org/) for the amazing game engine
- [Bytecode Alliance](https://bytecodealliance.org/) for all the amazing WASM tools
- [WebAssembly Coponents Registry](https://wa.dev/) - for providing a simple way to publish WIT components

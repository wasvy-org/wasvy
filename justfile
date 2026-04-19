# TIP: run the command `just -l` to get an overview

# Build an example rust crate to Wasm and copy it into host assets.
[group("examples")]
[arg("example", pattern="^(simple|guest_wit_example)$")]
build-example example:
	cargo build --release --target wasm32-wasip2 -p {{example}}
	mkdir -p ./examples/host_example/assets/mods
	cp ./target/wasm32-wasip2/release/{{example}}.wasm ./examples/host_example/assets/mods

# Run the host example app.
[group("examples")]
run-host-example:
	cargo run -p host_example --features bevy/file_watcher

# Run the host wit example app.
[group("examples")]
run-host-wit-example:
	cargo run -p host_example --features bevy/file_watcher

# Build the Python example mod.
[group("examples")]
[working-directory("examples/python_example/src")]
build-example-python:
	poetry run componentize-py --wit-path ../wit/ --world example componentize app -o ../../host_example/assets/mods/python.wasm

# Generate Python bindings for example mod.
[group("examples")]
[working-directory("examples/python_example")]
example-bindings-python:
	rm -rf ./src/componentize_py_async_support
	rm -rf ./src/wit_world
	rm -f ./src/componentize_py_runtime.pyi
	rm -f ./src/componentize_py_types.py
	rm -f ./src/poll_loop.py
	poetry run componentize-py --wit-path wit/ --world example bindings src

# Build the Go example mod.
[group("examples")]
[working-directory("examples/go_example/src")]
[env("GOARCH", "wasm")]
[env("GOOS", "wasip1")]
build-example-go:
	go build -o core.wasm -buildmode=c-shared -ldflags=-checklinkname=0
	wasm-tools component embed -w example ../wit/ core.wasm -o core-with-wit.wasm
	wasm-tools component new --adapt ../wasi_snapshot_preview1.reactor.wasm core-with-wit.wasm -o go.wasm

# Generate Go bindings for example mod.
[group("examples")]
[working-directory("examples/go_example/src")]
example-bindings-go:
	wit-bindgen go -w example ../wit/

# Fetch WIT dependencies for one example (deprecated, use cli instead).
[group("examples")]
example-fetch-deps example:
	cd examples/{{example}} && wkg wit fetch

# Enable repository git hooks.
[group("setup")]
enable-git-hooks:
	git config core.hooksPath .githooks
	chmod +x .githooks/pre-push

# Build the ECS WIT package.
[group("chores")]
build-wasvy-ecs:
	wkg wit build --wit-dir ./wit/ecs/

# Publish the ECS package to wa.dev.
[group("chores")]
publish-wasvy-ecs file_path version:
	wkg publish --package wasvy:ecs@{{version}} {{file_path}} --registry wa.dev

# Replace the existing (1.92.0) rust toolchain version with a new one.
[group("chores")]
[arg("new", pattern="^\\d+\\.\\d+\\.\\d+$")]
bump-toolchain new:
	rg -l 'rust' . | xargs sed -i "/rust/s/1.92.0/{{new}}/g"

# Replace the existing (0.18.0) bevy version with a new one.
[group("chores")]
[arg("new", pattern="^\\d+\\.\\d+\\.\\d+$")]
bump-bevy new:
	rg -l -g '!Cargo.lock' 'bevy' . | xargs sed -i "/bevy/s/0.18.0/{{new}}/g"
	cargo check

# Replace the existing (0.0.8) wasvy version with a new one.
[group("chores")]
[arg("new", pattern="^\\d+\\.\\d+\\.\\d+$")]
bump-version new:
	rg -l -g '!Cargo.lock' 'version' . | xargs sed -i "/version/s/0.0.8/{{new}}/g"
	cargo check

# Publishes all crates
[group("chores")]
[confirm]
publish:
	# CI
	cargo test
	cargo clippy -- -D warnings
	cargo fmt --all -- --check
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features

	# Publish
	cargo publish -p wasvy_macros
	cargo publish -p wasvy

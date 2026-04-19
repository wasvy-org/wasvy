build-example example:
	cargo build --release --target wasm32-wasip2 -p {{example}}
	mkdir -p ./examples/host_example/assets/mods
	cp ./target/wasm32-wasip2/release/{{example}}.wasm ./examples/host_example/assets/mods

run-host-example:
	cargo run -p host_example --features bevy/file_watcher

# Requires `poetry` to run
build-example-python:
	cd examples/python_example/src && poetry run componentize-py --wit-path ../wit/ --world example componentize app -o ../../host_example/assets/mods/python.wasm

# Create the bindings for the python example
example-bindings-python:
	cd examples/python_example && rm -rf ./src/componentize_py_async_support && rm -rf ./src/wit_world && rm -rf ./src/componentize_py_runtime.pyi && rm -rf ./src/componentize_py_types.py && rm -rf ./src/poll_loop.py && poetry run componentize-py --wit-path wit/ --world example bindings src

build-example-go:
	cd examples/go_example/src && GOARCH="wasm" GOOS="wasip1" go build -o core.wasm -buildmode=c-shared -ldflags=-checklinkname=0 && wasm-tools component embed -w example ../wit/ core.wasm -o core-with-wit.wasm && wasm-tools component new --adapt ../wasi_snapshot_preview1.reactor.wasm core-with-wit.wasm -o go.wasm

example-bindings-go:
	cd examples/go_example/src && wit-bindgen go -w example ../wit/

# For the fetching to take effect you must delete the deps folder manually
example-fetch-deps example:
	cd examples/{{example}} && wkg wit fetch

build-host:
	cargo build -p wasvy

build-wasvy-ecs:
	wkg wit build --wit-dir ./wit/ecs/

publish-wasvy-ecs file_path version:
	wkg publish --package wasvy:ecs@{{version}} {{file_path}} --registry wa.dev

# Enables git hooks
enable-git-hooks:
	git config core.hooksPath .githooks
	chmod +x .githooks/pre-push

# Replace the existing (1.92.0) rust toolchain version with a new one
toolchain new:
    rg -l 'rust' . | xargs sed -i "/rust/s/1.92.0/{{new}}/g"

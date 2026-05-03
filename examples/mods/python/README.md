### How To Build

You must have `poetry` install!

From the root project run:

```bash
just example-fetch-deps python_example
just build-example-python
```

### Important

It takes a few seconds for the python WASM to load. If there are no errors at runtime it means it's working, just give it 10-20 seconds.

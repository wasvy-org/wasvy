# Rust tests

Tests creating new rust mod directories and loading them into the a host app.

The crates directory exists to build mods using the existing workspace.

## The Empty Crate

Cargo will refuse to compile a project if any cargo workspace wildcard matches nothing, hence we keep this directory in source version control.

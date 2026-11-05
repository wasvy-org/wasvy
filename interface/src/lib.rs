//! Canonical WIT declarations for Wasvy and Bevy interfaces.

#![no_std]

/// The base Bevy version targeted by these interfaces.
pub const BEVY_VERSION: &str = "0.20.0"; // Updated by ci

/// The version of the `wasvy:ecs` package declared in [`WASVY_ECS`].
pub const WASVY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Bevy ECS package declaration.
pub const BEVY_ECS: &str = include_str!("../wit/bevy-ecs.wit");

/// The Wasvy ECS interface, including the host and guest worlds.
pub const WASVY_ECS: &str = include_str!("../wit/wasvy-ecs.wit");

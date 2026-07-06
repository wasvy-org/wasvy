#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png",
    html_favicon_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png"
)]

pub mod plugin;

pub mod prelude {
    pub use crate::plugin::ModLoaderPlugin;
    pub use wasvy_macros::WasvyComponent;
    pub use wasvy_runtime::prelude::*;
}

pub mod runtime {
    pub use wasvy_runtime::*;
}

#[cfg(feature = "wasm")]
pub mod wasm {
    pub use wasvy_wasm::*;
}

pub use wasvy_macros::{
    auto_host_components, component, guest_bindings, guest_type_paths, include_wasvy_components,
    methods, skip, WasvyComponent,
};

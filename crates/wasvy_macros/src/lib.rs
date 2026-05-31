//! Procedural macros for Wasvy component authoring and bindings.

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use std::path::{Path, PathBuf};
use syn::{
    Attribute, DeriveInput, FnArg, GenericArgument, Ident, ImplItem, Item, ItemFn, ItemImpl,
    ItemStruct, PathArguments, ReturnType, Type, TypePath,
};
use wit_parser::{FunctionKind, Resolve, TypeDefKind, WorldItem};

/// Attribute used to skip exporting a method in a `#[wasvy::methods]` impl.
#[proc_macro_attribute]
pub fn skip(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Derive macro to mark a type as a Wasvy-exported component.
///
/// This is required even for components without methods so they can be exported
/// to mods and appear in generated WIT.
#[proc_macro_derive(WasvyComponent)]
pub fn derive_wasvy_component(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    let wasvy_path = wasvy_path();
    let ident = &input.ident;
    let register_ident = format_ident!("__wasvy_register_component_{}", ident);

    let expanded = quote! {
        impl #wasvy_path::authoring::WasvyComponent for #ident {}

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            <#ident as #wasvy_path::authoring::WasvyComponent>::register(app);
        }

        #wasvy_path::__wasvy_submit_component_registration!(
            #wasvy_path::authoring::WasvyComponentRegistration { register: #register_ident }
        );
    };

    expanded.into()
}

/// Generate host-side bindings for the WIT components interface.
///
/// This expands to `wasmtime::component::bindgen!`, implements host traits
/// for `WasmHost`, and exposes an `add_components_to_linker` helper.
///
/// # Example
/// ```ignore
/// wasvy::auto_host_components! {
///     path = "wit",
///     world = "host",
///     module = components_bindings,
/// }
/// ```
#[proc_macro]
pub fn auto_host_components(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as AutoHostArgs);
    match expand_auto_host_components(args) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Generate `type_path` helpers for guest bindings.
///
/// This reads `wasvy:type-path=` doc tags from resources and adds
/// `TYPE_PATH`, `type_path()`, and `type_path_str()` helpers.
///
/// # Example
/// ```ignore
/// wasvy::guest_type_paths! {
///     path = "wit",
///     package = "game:components",
///     interface = "components",
///     module = crate::bindings,
/// }
/// ```
#[proc_macro]
pub fn guest_type_paths(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as GuestTypePathsArgs);
    match expand_guest_type_paths(args) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Wrapper around `wit_bindgen::generate!` that also adds type-path helpers.
///
/// This is intended for mods so they only need to call this macro.
///
/// # Example
/// ```ignore
/// wasvy::guest_bindings!({
///     path: "wit",
///     world: "guest",
/// });
/// ```
#[proc_macro]
pub fn guest_bindings(input: TokenStream) -> TokenStream {
    let input_tokens = proc_macro2::TokenStream::from(input.clone());
    let args = syn::parse_macro_input!(input as GuestBindingsArgs);
    match expand_guest_bindings(args, input_tokens) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Include all Rust modules under a path that contain Wasvy macros.
///
/// This is primarily used in `build.rs` to ensure `inventory` sees all
/// components/methods when generating WIT.
///
/// # Example
/// ```ignore
/// fn main() {
///     wasvy::include_wasvy_components!("src");
/// }
/// ```
#[proc_macro]
pub fn include_wasvy_components(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as IncludeComponentsArgs);
    match expand_include_components(args) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Mark a type as a Wasvy component and register it for WIT generation.
///
/// This implements `WasvyComponent` and submits metadata to `inventory`.
///
/// # Example
/// ```ignore
/// #[wasvy::component]
/// #[derive(Reflect)]
/// pub struct Health {
///     pub current: f32,
///     pub max: f32,
/// }
/// ```
#[deprecated(note = "Use #[derive(WasvyComponent)] instead")]
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as Item);
    let wasvy_path = wasvy_path();

    let expanded = match input {
        Item::Struct(item) => expand_component_struct(item, &wasvy_path),
        Item::Enum(item) => {
            let ident = &item.ident;
            let register_ident = format_ident!("__wasvy_register_component_{}", ident);
            quote! {
                #item

                impl #wasvy_path::authoring::WasvyComponent for #ident {}

                #[allow(non_snake_case)]
                fn #register_ident(app: &mut #wasvy_path::authoring::App) {
                    <#ident as #wasvy_path::authoring::WasvyComponent>::register(app);
                }

                #wasvy_path::__wasvy_submit_component_registration!(
                    #wasvy_path::authoring::WasvyComponentRegistration { register: #register_ident }
                );
            }
        }
        other => {
            return syn::Error::new_spanned(
                other,
                "#[wasvy::component] can only be applied to structs or enums",
            )
            .to_compile_error()
            .into();
        }
    };

    expanded.into()
}

/// Export methods from an `impl` block for Wasvy.
///
/// All `&self` / `&mut self` methods are registered for dynamic invoke, and
/// argument names are captured for WIT generation. Use `#[wasvy::skip]` to
/// exclude a method from export.
///
/// # Example
/// ```ignore
/// #[wasvy::methods]
/// impl Health {
///     pub fn heal(&mut self, amount: f32) {
///         self.current = (self.current + amount).min(self.max);
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as ItemImpl);
    let wasvy_path = wasvy_path();

    let Some(self_ty) = extract_type_path(&input.self_ty) else {
        return syn::Error::new_spanned(
            input.self_ty,
            "#[wasvy::methods] requires a concrete type (no generics)",
        )
        .to_compile_error()
        .into();
    };

    let type_ident = self_ty
        .path
        .segments
        .last()
        .map(|seg| seg.ident.clone())
        .unwrap();
    let mut method_idents = Vec::new();
    let mut items = Vec::new();
    let mut metadata_submits = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for item in input.items.into_iter() {
        match item {
            ImplItem::Fn(mut func) => {
                let skip = has_wasvy_skip_attr(&func.attrs);
                func.attrs.retain(|attr| !is_wasvy_skip_attr(attr));

                if skip {
                    items.push(ImplItem::Fn(func));
                    continue;
                }

                match func.sig.receiver() {
                    Some(receiver) => {
                        if receiver.reference.is_none() {
                            let err = syn::Error::new_spanned(
                                &func.sig,
                                "#[wasvy::methods] only supports &self or &mut self receivers; add #[wasvy::skip] or move this method to another impl",
                            );
                            if let Some(errors) = errors.as_mut() {
                                errors.combine(err);
                            } else {
                                errors = Some(err);
                            }
                        }
                    }
                    None => {
                        let err = syn::Error::new_spanned(
                            &func.sig,
                            "#[wasvy::methods] requires a self receiver; add #[wasvy::skip] or move this method to another impl",
                        );
                        if let Some(errors) = errors.as_mut() {
                            errors.combine(err);
                        } else {
                            errors = Some(err);
                        }
                    }
                }

                if !func.sig.generics.params.is_empty() {
                    let err = syn::Error::new_spanned(
                        &func.sig.generics,
                        "#[wasvy::methods] does not support generic methods; add #[wasvy::skip] or move this method to another impl",
                    );
                    if let Some(errors) = errors.as_mut() {
                        errors.combine(err);
                    } else {
                        errors = Some(err);
                    }
                }

                let method_ident = func.sig.ident.clone();
                let type_path_expr = quote!(concat!(module_path!(), "::", stringify!(#type_ident)));
                let method_lit =
                    syn::LitStr::new(&method_ident.to_string(), proc_macro2::Span::call_site());
                let mut arg_names = Vec::new();
                for (idx, arg) in func.sig.inputs.iter().skip(1).enumerate() {
                    if let syn::FnArg::Typed(pat) = arg {
                        match pat.pat.as_ref() {
                            syn::Pat::Ident(ident) => {
                                arg_names.push(ident.ident.to_string());
                            }
                            _ => {
                                let err = syn::Error::new_spanned(
                                    pat.pat.as_ref(),
                                    format!(
                                        "#[wasvy::methods] only supports identifier arguments; add #[wasvy::skip] or rename parameter {} to an identifier",
                                        idx
                                    ),
                                );
                                if let Some(errors) = errors.as_mut() {
                                    errors.combine(err);
                                } else {
                                    errors = Some(err);
                                }
                            }
                        }
                    }
                }
                let arg_name_lits: Vec<syn::LitStr> = arg_names
                    .iter()
                    .map(|name| syn::LitStr::new(name, proc_macro2::Span::call_site()))
                    .collect();
                let metadata_ident = format_ident!("__wasvy_args_{}_{}", type_ident, method_ident);

                metadata_submits.push(quote! {
                    #[allow(non_upper_case_globals)]
                    const #metadata_ident: &[&str] = &[#(#arg_name_lits),*];
                    #wasvy_path::__wasvy_submit_method_metadata!(
                        #wasvy_path::authoring::WasvyMethodMetadata {
                            type_path: #type_path_expr,
                            method: #method_lit,
                            arg_names: #metadata_ident,
                        }
                    );
                });

                method_idents.push(method_ident);
                items.push(ImplItem::Fn(func));
            }
            other => items.push(other),
        }
    }

    let impl_block = ItemImpl { items, ..input };

    let register_ident = format_ident!("__wasvy_register_methods_{}", type_ident);

    let expanded = quote! {
        #impl_block

        impl #wasvy_path::authoring::WasvyMethods for #type_ident {
            fn register_methods(app: &mut #wasvy_path::authoring::App) {
                #(
                    app.register_function(#type_ident::#method_idents);
                )*
            }
        }

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            <#type_ident as #wasvy_path::authoring::WasvyMethods>::register_methods(app);
        }

        #(#metadata_submits)*

        #wasvy_path::__wasvy_submit_methods_registration!(
            #wasvy_path::authoring::WasvyMethodsRegistration { register: #register_ident }
        );
    };

    if let Some(err) = errors {
        err.to_compile_error().into()
    } else {
        expanded.into()
    }
}

/// Declare crate-level Wasvy Module metadata and generate the native adapter plugin.
#[proc_macro]
pub fn module(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as ModuleDeclarationArgs);
    let wasvy_path = wasvy_path();
    let name = args.name;

    let guest_source = match guest_module_source() {
        Ok(source) => source,
        Err(err) => return err.to_compile_error().into(),
    };
    let guest_wit = generate_guest_wit(&guest_source.systems);
    let guest_wit_lit = syn::LitStr::new(&guest_wit, proc_macro2::Span::call_site());
    let guest_wit_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../wit/wasvy-ecs.wit");
    let guest_wit_path_lit = syn::LitStr::new(
        &guest_wit_path.to_string_lossy(),
        proc_macro2::Span::call_site(),
    );

    let guest_register_systems = match guest_source
        .systems
        .iter()
        .map(|system| {
            let export_name = system.ident.to_string();
            let schedule = guest_schedule_tokens(&system.schedule)?;
            let param_registration = system.params.iter().map(|param| match param {
                GuestSourceParam::Commands => quote!(system.add_commands();),
                GuestSourceParam::Res(ty) => {
                    quote!(system.add_res(<#ty as ::bevy_reflect::TypePath>::type_path());)
                }
                GuestSourceParam::ResMut(ty) => {
                    quote!(system.add_res_mut(<#ty as ::bevy_reflect::TypePath>::type_path());)
                }
                GuestSourceParam::Query(items) => {
                    let items = guest_query_for_tokens(items);
                    quote!(system.add_query(&[#(#items),*]);)
                }
            });
            Ok::<_, syn::Error>(quote! {
                let system = __wasvy_guest_bindings::wasvy::ecs::app::System::new(#export_name);
                #(#param_registration)*
                app.add_systems(&#schedule, &[&system]);
            })
        })
        .collect::<syn::Result<Vec<_>>>()
    {
        Ok(tokens) => tokens,
        Err(err) => return err.to_compile_error().into(),
    };

    let guest_system_exports = guest_source.systems.iter().map(|system| {
        let ident = &system.ident;
        let export_ident = format_ident!("__wasvy_guest_export_{}", ident);
        let args = system.params.iter().enumerate().map(|(index, param)| {
            let ident = format_ident!("arg{index}");
            let ty = match param {
                GuestSourceParam::Commands => {
                    quote!(__wasvy_guest_bindings::wasvy::ecs::app::Commands)
                }
                GuestSourceParam::Query(_) => {
                    quote!(__wasvy_guest_bindings::wasvy::ecs::app::Query)
                }
                GuestSourceParam::Res(_) | GuestSourceParam::ResMut(_) => {
                    quote!(__wasvy_guest_bindings::wasvy::ecs::app::WorldResource)
                }
            };
            quote!(#ident: #ty)
        });
        let call_args = (0..system.params.len()).map(|index| format_ident!("arg{index}"));
        quote! {
            fn #ident(#(#args),*) {
                #export_ident(#(#call_args),*);
            }
        }
    });

    let guest_first_load = if let Some(ident) = guest_source.first_load {
        let export_ident = format_ident!("__wasvy_guest_export_{}", ident);
        quote! {
            fn on_first_load(commands: __wasvy_guest_bindings::wasvy::ecs::app::Commands) {
                #export_ident(commands);
            }
        }
    } else {
        quote! {
            fn on_first_load(_commands: __wasvy_guest_bindings::wasvy::ecs::app::Commands) {}
        }
    };

    let expanded = quote! {
        pub const MODULE_NAME: &str = #name;

        #[derive(Default)]
        pub struct NativeAdapterPlugin;

        impl #wasvy_path::authoring::Plugin for NativeAdapterPlugin {
            fn build(&self, app: &mut #wasvy_path::authoring::App) {
                let scope = module_path!();
                for registration in #wasvy_path::authoring::inventory::iter::<#wasvy_path::authoring::WasvyModuleSystemRegistration> {
                    if #wasvy_path::authoring::module_scope_matches(scope, registration.scope) {
                        (registration.register_native)(app);
                    }
                }
                for registration in #wasvy_path::authoring::inventory::iter::<#wasvy_path::authoring::WasvyModuleFirstLoadRegistration> {
                    if #wasvy_path::authoring::module_scope_matches(scope, registration.scope) {
                        (registration.register_native)(app);
                    }
                }
            }
        }

        impl #wasvy_path::module_plugin::NativeAdapterPlugin for NativeAdapterPlugin {}

        #[cfg(target_arch = "wasm32")]
        mod __wasvy_guest_bindings {
            wit_bindgen::generate!({
                path: [#guest_wit_path_lit],
                inline: #guest_wit_lit,
                world: "guest",
                pub_export_macro: true,
                with: {
                    "wasvy:ecs/app@0.0.7": generate,
                }
            });
        }

        #[cfg(target_arch = "wasm32")]
        impl #wasvy_path::module_guest::GuestCommandsBinding for __wasvy_guest_bindings::wasvy::ecs::app::Commands {
            fn insert_resource(&self, type_path: &str, value: &[u8]) {
                __wasvy_guest_bindings::wasvy::ecs::app::Commands::insert_resource(self, type_path, value);
            }

            fn remove_resource(&self, type_path: &str) {
                __wasvy_guest_bindings::wasvy::ecs::app::Commands::remove_resource(self, type_path);
            }
        }

        #[cfg(target_arch = "wasm32")]
        impl #wasvy_path::module_guest::GuestWorldResourceBinding for __wasvy_guest_bindings::wasvy::ecs::app::WorldResource {
            fn get(&self) -> Vec<u8> {
                __wasvy_guest_bindings::wasvy::ecs::app::WorldResource::get(self)
            }

            fn set(&self, value: &[u8]) {
                __wasvy_guest_bindings::wasvy::ecs::app::WorldResource::set(self, value);
            }
        }

        #[cfg(target_arch = "wasm32")]
        impl #wasvy_path::module_guest::GuestComponentBinding for __wasvy_guest_bindings::wasvy::ecs::app::Component {
            fn get(&self) -> Vec<u8> {
                __wasvy_guest_bindings::wasvy::ecs::app::Component::get(self)
            }

            fn set(&self, value: &[u8]) {
                __wasvy_guest_bindings::wasvy::ecs::app::Component::set(self, value);
            }
        }

        #[cfg(target_arch = "wasm32")]
        impl #wasvy_path::module_guest::GuestQueryResultBinding for __wasvy_guest_bindings::wasvy::ecs::app::QueryResult {
            type Component = __wasvy_guest_bindings::wasvy::ecs::app::Component;

            fn component(&self, index: u8) -> Self::Component {
                __wasvy_guest_bindings::wasvy::ecs::app::QueryResult::component(self, index)
            }
        }

        #[cfg(target_arch = "wasm32")]
        impl #wasvy_path::module_guest::GuestQueryBinding for __wasvy_guest_bindings::wasvy::ecs::app::Query {
            type QueryResult = __wasvy_guest_bindings::wasvy::ecs::app::QueryResult;

            fn next(&mut self) -> Option<Self::QueryResult> {
                __wasvy_guest_bindings::wasvy::ecs::app::Query::iter(self)
            }
        }

        #[cfg(target_arch = "wasm32")]
        struct __WasvyModuleGuest;

        #[cfg(target_arch = "wasm32")]
        impl __wasvy_guest_bindings::Guest for __WasvyModuleGuest {
            fn register(app: __wasvy_guest_bindings::wasvy::ecs::app::App) {
                #(#guest_register_systems)*
            }

            #guest_first_load

            #(#guest_system_exports)*
        }

        #[cfg(target_arch = "wasm32")]
        __wasvy_guest_bindings::export!(__WasvyModuleGuest with_types_in __wasvy_guest_bindings);

        #[doc(hidden)]
        #[used]
        #[unsafe(export_name = concat!("__wasvy_module_decl_guard__", module_path!()))]
        static __WASVY_MODULE_DECL_GUARD: u8 = 0;

        #wasvy_path::__wasvy_submit_module_declaration!(
            #wasvy_path::authoring::WasvyModuleDeclaration {
                scope: module_path!(),
                name: #name,
            }
        );
    };

    expanded.into()
}

/// Mark a function as a Wasvy Module system on a specific schedule.
#[proc_macro_attribute]
pub fn system(attr: TokenStream, item: TokenStream) -> TokenStream {
    let schedule_tokens = proc_macro2::TokenStream::from(attr);
    let input = syn::parse_macro_input!(item as ItemFn);
    match expand_module_system(input, schedule_tokens) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Mark a function as Wasvy Module first-load initialization.
#[proc_macro_attribute]
pub fn on_first_load(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as ItemFn);
    match expand_first_load(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_component_struct(
    item: ItemStruct,
    wasvy_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let ident = &item.ident;
    let register_ident = format_ident!("__wasvy_register_component_{}", ident);
    quote! {
        #item

        impl #wasvy_path::authoring::WasvyComponent for #ident {}

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            <#ident as #wasvy_path::authoring::WasvyComponent>::register(app);
        }

        #wasvy_path::__wasvy_submit_component_registration!(
            #wasvy_path::authoring::WasvyComponentRegistration { register: #register_ident }
        );
    }
}

fn has_wasvy_skip_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_wasvy_skip_attr)
}

fn is_wasvy_skip_attr(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "skip")
}

fn extract_type_path(ty: &Type) -> Option<&TypePath> {
    match ty {
        Type::Path(path) if path.qself.is_none() => Some(path),
        _ => None,
    }
}

fn wasvy_path() -> proc_macro2::TokenStream {
    let name = match crate_name("wasvy") {
        Ok(FoundCrate::Name(name)) => name,
        Ok(FoundCrate::Itself) | Err(_) => "wasvy".to_string(),
    };
    let ident = Ident::new(&name, proc_macro2::Span::call_site());
    quote!(::#ident)
}

struct AutoHostArgs {
    path: syn::LitStr,
    world: syn::LitStr,
    module: Ident,
}

struct GuestTypePathsArgs {
    path: syn::LitStr,
    package: syn::LitStr,
    interface: syn::LitStr,
    module: syn::Path,
}

struct GuestBindingsArgs {
    paths: Vec<syn::LitStr>,
}

struct IncludeComponentsArgs {
    path: syn::LitStr,
}

impl syn::parse::Parse for AutoHostArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut path = None;
        let mut world = None;
        let mut module = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            match key.to_string().as_str() {
                "path" => path = Some(input.parse()?),
                "world" => world = Some(input.parse()?),
                "module" => module = Some(input.parse()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown key `{other}`"),
                    ));
                }
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }

        Ok(Self {
            path: path.ok_or_else(|| input.error("missing `path`"))?,
            world: world.ok_or_else(|| input.error("missing `world`"))?,
            module: module.unwrap_or_else(|| {
                Ident::new("components_bindings", proc_macro2::Span::call_site())
            }),
        })
    }
}

impl syn::parse::Parse for GuestTypePathsArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut path = None;
        let mut package = None;
        let mut interface = None;
        let mut module = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            match key.to_string().as_str() {
                "path" => path = Some(input.parse()?),
                "package" => package = Some(input.parse()?),
                "interface" => interface = Some(input.parse()?),
                "module" => module = Some(input.parse()?),
                _ => return Err(input.error("unsupported key")),
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }

        Ok(Self {
            path: path.ok_or_else(|| input.error("missing `path`"))?,
            package: package.ok_or_else(|| input.error("missing `package`"))?,
            interface: interface.ok_or_else(|| input.error("missing `interface`"))?,
            module: module.ok_or_else(|| input.error("missing `module`"))?,
        })
    }
}

impl syn::parse::Parse for GuestBindingsArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let tokens: proc_macro2::TokenStream = input.parse()?;
        let paths = extract_paths_from_stream(tokens)?;
        Ok(Self { paths })
    }
}

impl syn::parse::Parse for IncludeComponentsArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let lit: syn::LitStr = input.parse()?;
        Ok(Self { path: lit })
    }
}

fn expand_auto_host_components(args: AutoHostArgs) -> syn::Result<proc_macro2::TokenStream> {
    let wasvy_path = wasvy_path();
    let path_value = resolve_wit_path_with_fallbacks(&args.path);
    let world_value = args.world.value();

    let mut resolve = Resolve::default();
    let (pkg_id, _sources) = resolve
        .push_path(&path_value)
        .map_err(|err| syn::Error::new(args.path.span(), err.to_string()))?;
    let world_id = resolve
        .select_world(&[pkg_id], Some(&world_value))
        .map_err(|err| syn::Error::new(args.world.span(), err.to_string()))?;
    let world = &resolve.worlds[world_id];

    let interface_id = world
        .imports
        .iter()
        .find_map(|(name, item)| match (name, item) {
            (wit_parser::WorldKey::Name(name), WorldItem::Interface { id, .. })
                if name == "components" =>
            {
                Some(*id)
            }
            (wit_parser::WorldKey::Interface(id), WorldItem::Interface { .. }) => {
                let iface = &resolve.interfaces[*id];
                if iface.name.as_deref() == Some("components") {
                    Some(*id)
                } else {
                    None
                }
            }
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new(args.world.span(), "missing `components` interface import")
        })?;
    let interface = &resolve.interfaces[interface_id];
    let package_id = interface
        .package
        .ok_or_else(|| syn::Error::new(args.world.span(), "interface has no package"))?;
    let package = &resolve.packages[package_id];

    let pkg_namespace = rust_ident(&package.name.namespace.to_string());
    let pkg_name = rust_ident(&package.name.name.to_string());
    let interface_name = rust_ident(interface.name.as_deref().unwrap_or("components"));

    let module_ident = args.module;

    let mut with_entries = Vec::new();
    for (name, type_id) in interface.types.iter() {
        let type_def = &resolve.types[*type_id];
        if !matches!(type_def.kind, TypeDefKind::Resource) {
            continue;
        }
        let path = format!(
            "{}:{}/{}.{}",
            package.name.namespace,
            package.name.name,
            interface.name.as_deref().unwrap_or("components"),
            name
        );
        let lit = syn::LitStr::new(&path, proc_macro2::Span::call_site());
        with_entries.push(quote!(#lit: #wasvy_path::host::WasmComponent));
    }

    let wasvy_component =
        syn::LitStr::new("wasvy:ecs/app.component", proc_macro2::Span::call_site());
    with_entries.push(quote!(#wasvy_component: #wasvy_path::host::WasmComponent));

    let mut impls = Vec::new();

    for (name, type_id) in interface.types.iter() {
        let type_def = &resolve.types[*type_id];
        if !matches!(type_def.kind, TypeDefKind::Resource) {
            continue;
        }
        let trait_ident = format_ident!("Host{}", upper_camel(name));

        let mut methods = Vec::new();
        for function in interface.functions.values() {
            match function.kind {
                FunctionKind::Constructor(id) if id == *type_id => {
                    let params = render_params(&resolve, &function.params, &wasvy_path, true);
                    let ret_tokens =
                        quote!(::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>);
                    let body = quote!(component);
                    methods.push(quote! {
                        fn new(&mut self, #params) -> #ret_tokens {
                            #body
                        }
                    });
                }
                FunctionKind::Method(id) if id == *type_id => {
                    let method_name = method_name(&function.name);
                    let method_ident = rust_ident(&method_name);
                    let params = render_params(&resolve, &function.params, &wasvy_path, false);
                    let ret = render_return(&resolve, function.result.as_ref(), &wasvy_path);
                    let invoke = render_invoke_body(
                        &method_name,
                        &function.params,
                        function.result.as_ref(),
                        &wasvy_path,
                    );
                    methods.push(quote! {
                        fn #method_ident(&mut self, #params) #ret {
                            #invoke
                        }
                    });
                }
                _ => {}
            }
        }

        methods.push(quote! {
            fn drop(&mut self, component: ::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>) -> Result<(), ::wasmtime::Error> {
                let _ = component;
                Ok(())
            }
        });

        let trait_path =
            quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::#trait_ident);
        impls.push(quote! {
            impl #trait_path for #wasvy_path::host::WasmHost {
                #(#methods)*
            }
        });
    }

    let trait_host_path = quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::Host);
    let add_to_linker_path =
        quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::add_to_linker);

    let expanded = quote! {
        mod #module_ident {
            ::wasmtime::component::bindgen!({
                path: #path_value,
                world: #world_value,
                with: { #(#with_entries),* },
            });
        }

        pub fn add_components_to_linker(linker: &mut #wasvy_path::engine::Linker) {
            type Data = ::wasmtime::component::HasSelf<#wasvy_path::host::WasmHost>;
            #add_to_linker_path::<_, Data>(linker, |state| state)
                .expect("implement components interface");
        }

        impl #trait_host_path for #wasvy_path::host::WasmHost {}

        #(#impls)*
    };

    Ok(expanded)
}

fn expand_guest_type_paths(args: GuestTypePathsArgs) -> syn::Result<proc_macro2::TokenStream> {
    let path_value = resolve_wit_path(&args.path);
    let package_value = args.package.value();
    let interface_value = args.interface.value();
    let module = args.module;

    let (namespace, name) = package_value
        .split_once(':')
        .ok_or_else(|| syn::Error::new(args.package.span(), "package must be `namespace:name`"))?;

    let mut resolve = Resolve::default();
    resolve
        .push_path(&path_value)
        .map_err(|err| syn::Error::new(args.path.span(), err.to_string()))?;

    let package_id = resolve
        .packages
        .iter()
        .find_map(|(id, pkg)| {
            if pkg.name.namespace == namespace && pkg.name.name == name {
                Some(id)
            } else {
                None
            }
        })
        .ok_or_else(|| syn::Error::new(args.package.span(), "package not found"))?;

    let interface_id = resolve
        .interfaces
        .iter()
        .find_map(|(id, iface)| {
            if iface.name.as_deref() == Some(&interface_value) && iface.package == Some(package_id)
            {
                Some(id)
            } else {
                None
            }
        })
        .ok_or_else(|| syn::Error::new(args.interface.span(), "interface not found"))?;

    let interface = &resolve.interfaces[interface_id];
    let mut impls = Vec::new();

    for (name, type_id) in interface.types.iter() {
        let type_def = &resolve.types[*type_id];
        if !matches!(type_def.kind, TypeDefKind::Resource) {
            continue;
        }
        let type_path = extract_wit_type_path(&type_def.docs).ok_or_else(|| {
            syn::Error::new(
                args.interface.span(),
                format!("resource `{name}` missing wasvy:type-path doc"),
            )
        })?;

        let type_ident = format_ident!("{}", upper_camel(name));
        let type_path_lit = syn::LitStr::new(&type_path, proc_macro2::Span::call_site());
        impls.push(quote! {
            impl #module::#type_ident {
                pub const TYPE_PATH: &'static str = #type_path_lit;

                pub fn type_path() -> String {
                    Self::TYPE_PATH.to_string()
                }

                pub fn type_path_str() -> &'static str {
                    Self::TYPE_PATH
                }
            }
        });
    }

    Ok(quote! {
        #(#impls)*
    })
}

fn expand_guest_bindings(
    args: GuestBindingsArgs,
    input_tokens: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut resolve = Resolve::default();
    for path in &args.paths {
        let resolved = resolve_wit_path(path);
        resolve
            .push_path(&resolved)
            .map_err(|err| syn::Error::new(path.span(), err.to_string()))?;
    }

    let mut impls = Vec::new();

    for (_id, interface) in resolve.interfaces.iter() {
        let Some(package_id) = interface.package else {
            continue;
        };
        let package = &resolve.packages[package_id];
        let namespace = rust_ident(&package.name.namespace);
        let name = rust_ident(&package.name.name);
        let interface_name = rust_ident(interface.name.as_deref().unwrap_or("components"));
        let module = quote!(self::#namespace::#name::#interface_name);

        for (resource_name, type_id) in interface.types.iter() {
            let type_def = &resolve.types[*type_id];
            if !matches!(type_def.kind, TypeDefKind::Resource) {
                continue;
            }
            let Some(type_path) = extract_wit_type_path(&type_def.docs) else {
                continue;
            };
            let type_ident = format_ident!("{}", upper_camel(resource_name));
            let type_path_lit = syn::LitStr::new(&type_path, proc_macro2::Span::call_site());
            impls.push(quote! {
                impl #module::#type_ident {
                    pub const TYPE_PATH: &'static str = #type_path_lit;

                    pub fn type_path() -> String {
                        Self::TYPE_PATH.to_string()
                    }

                    pub fn type_path_str() -> &'static str {
                        Self::TYPE_PATH
                    }
                }
            });
        }
    }

    Ok(quote! {
        wit_bindgen::generate!(#input_tokens);
        #(#impls)*
    })
}

fn expand_include_components(args: IncludeComponentsArgs) -> syn::Result<proc_macro2::TokenStream> {
    let base = resolve_wit_path(&args.path);
    let base = PathBuf::from(base);
    let mut files = Vec::new();
    collect_rs_files(&base, &mut files)
        .map_err(|err| syn::Error::new(args.path.span(), err.to_string()))?;

    let mut root = ModuleNode::default();
    for path in files.iter() {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        if !contains_wasvy_attr(&contents) {
            continue;
        }
        let segments =
            module_segments(&base, path).map_err(|err| syn::Error::new(args.path.span(), err))?;
        root.insert(&segments, path.clone());
    }

    let rendered = render_modules(&root);
    Ok(rendered)
}

fn render_params(
    resolve: &Resolve,
    params: &[wit_parser::Param],
    wasvy_path: &proc_macro2::TokenStream,
    is_constructor: bool,
) -> proc_macro2::TokenStream {
    let mut out = Vec::new();
    if !is_constructor {
        out.push(
            quote!(component: ::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>),
        );
    }
    for param in params.iter().filter(|param| param.name != "self") {
        let ident = rust_ident(&param.name);
        let ty_tokens = ty_to_tokens(resolve, &param.ty, wasvy_path);
        out.push(quote!(#ident: #ty_tokens));
    }
    quote!(#(#out),*)
}

fn render_return(
    resolve: &Resolve,
    result: Option<&wit_parser::Type>,
    wasvy_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match result {
        None => quote!(),
        Some(ty) => {
            let tokens = ty_to_tokens(resolve, ty, wasvy_path);
            quote!(-> #tokens)
        }
    }
}

fn render_invoke_body(
    method: &str,
    params: &[wit_parser::Param],
    result: Option<&wit_parser::Type>,
    wasvy_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let arg_idents: Vec<Ident> = params
        .iter()
        .filter(|param| param.name != "self")
        .map(|param| rust_ident(&param.name))
        .collect();
    let args_expr = if arg_idents.is_empty() {
        quote!(())
    } else {
        quote!((#(#arg_idents),*,))
    };
    let method_lit = syn::LitStr::new(method, proc_macro2::Span::call_site());
    match result {
        None => quote! {
            #[allow(unused_imports)]
            use #wasvy_path::serialize::*;
            // Note: when implementing a custom codec, a wasvy_encode method is expected to be in scope
            let params = wasvy_encode(&#args_expr).expect("serialize params");
            let _ = #wasvy_path::host::invoke_component_method(self, component, #method_lit, &params)
                .expect("invoke method");
        },
        Some(_) => quote! {
            #[allow(unused_imports)]
            use #wasvy_path::serialize::*;
            // Note: when implementing a custom codec, a wasvy_encode and wasvy_decode method is expected to be in scope
            let params = wasvy_encode(&#args_expr).expect("serialize params");
            let output = #wasvy_path::host::invoke_component_method(self, component, #method_lit, &params)
                .expect("invoke method");
            wasvy_decode(&output).expect("deserialize")
        },
    }
}

fn ty_to_tokens(
    resolve: &Resolve,
    ty: &wit_parser::Type,
    wasvy_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match ty {
        wit_parser::Type::Bool => quote!(bool),
        wit_parser::Type::U8 => quote!(u8),
        wit_parser::Type::U16 => quote!(u16),
        wit_parser::Type::U32 => quote!(u32),
        wit_parser::Type::U64 => quote!(u64),
        wit_parser::Type::S8 => quote!(i8),
        wit_parser::Type::S16 => quote!(i16),
        wit_parser::Type::S32 => quote!(i32),
        wit_parser::Type::S64 => quote!(i64),
        wit_parser::Type::F32 => quote!(f32),
        wit_parser::Type::F64 => quote!(f64),
        wit_parser::Type::Char => quote!(char),
        wit_parser::Type::String => quote!(String),
        wit_parser::Type::Id(id) => match &resolve.types[*id].kind {
            TypeDefKind::Resource => {
                quote!(::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>)
            }
            TypeDefKind::Handle(handle) => match handle {
                wit_parser::Handle::Borrow(_) | wit_parser::Handle::Own(_) => {
                    quote!(::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>)
                }
            },
            TypeDefKind::Option(inner) => {
                let inner = ty_to_tokens(resolve, inner, wasvy_path);
                quote!(Option<#inner>)
            }
            TypeDefKind::List(inner) => {
                let inner = ty_to_tokens(resolve, inner, wasvy_path);
                quote!(Vec<#inner>)
            }
            _ => quote!(String),
        },
        wit_parser::Type::ErrorContext => quote!(String),
    }
}

struct ModuleDeclarationArgs {
    name: syn::LitStr,
}

impl syn::parse::Parse for ModuleDeclarationArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        if key != "name" {
            return Err(syn::Error::new(key.span(), "expected `name`"));
        }
        let _: syn::Token![:] = input.parse()?;
        let name: syn::LitStr = input.parse()?;
        if input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
        }
        if !input.is_empty() {
            return Err(input.error("unexpected extra tokens in wasvy::module! declaration"));
        }
        Ok(Self { name })
    }
}

fn expand_module_system(
    input: ItemFn,
    schedule_tokens: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    if schedule_tokens.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.sig.ident,
            "#[wasvy::system(...)] requires a schedule, e.g. #[wasvy::system(Update)]",
        ));
    }

    validate_fn_shape(&input)?;
    let referenced_types = validate_system_params(&input.sig.inputs)?;
    let wasvy_path = wasvy_path();
    let ident = input.sig.ident.clone();
    let register_ident = format_ident!("__wasvy_register_module_system_{}", ident);
    let export_name = syn::LitStr::new(&ident.to_string(), ident.span());
    let metadata_ident = format_ident!("__wasvy_module_system_types_{}", ident);
    let guest_impl_ident = format_ident!("__wasvy_guest_impl_{}", ident);
    let guest_export_ident = format_ident!("__wasvy_guest_export_{}", ident);
    let type_lits: Vec<syn::LitStr> = referenced_types
        .iter()
        .map(|name| syn::LitStr::new(name, proc_macro2::Span::call_site()))
        .collect();
    let guest_params = input
        .sig
        .inputs
        .iter()
        .map(|arg| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let pat = &arg.pat;
            let ty = guest_param_wrapper_tokens(arg, &wasvy_path)?;
            Ok::<_, syn::Error>(quote!(#pat: #ty))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_raw_params = input
        .sig
        .inputs
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let raw_ident = format_ident!("__wasvy_arg_{index}");
            let ty = guest_raw_param_type_tokens(arg)?;
            Ok::<_, syn::Error>(quote!(#raw_ident: #ty))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_wrappers = input
        .sig
        .inputs
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let pat = &arg.pat;
            let raw_ident = format_ident!("__wasvy_arg_{index}");
            let ctor = guest_wrapper_ctor_tokens(arg, &raw_ident, &wasvy_path)?;
            Ok::<_, syn::Error>(quote!(let #pat = #ctor;))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_call_args = input
        .sig
        .inputs
        .iter()
        .map(|arg| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let syn::Pat::Ident(pat_ident) = arg.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &arg.pat,
                    "Wasvy Module parameters must use identifier bindings",
                ));
            };
            let ident = &pat_ident.ident;
            Ok::<_, syn::Error>(quote!(#ident))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let body = &input.block;

    Ok(quote! {
        #input

        #[allow(non_upper_case_globals)]
        const #metadata_ident: &[&str] = &[#(#type_lits),*];

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            app.add_systems(#schedule_tokens, #ident);
        }

        #[cfg(target_arch = "wasm32")]
        fn #guest_impl_ident(#(#guest_params),*) #body

        #[cfg(target_arch = "wasm32")]
        fn #guest_export_ident(#(#guest_raw_params),*) {
            #(#guest_wrappers)*
            #guest_impl_ident(#(#guest_call_args),*);
        }

        #wasvy_path::__wasvy_submit_module_system_registration!(
            #wasvy_path::authoring::WasvyModuleSystemRegistration {
                scope: module_path!(),
                export_name: #export_name,
                register_native: #register_ident,
                referenced_types: #metadata_ident,
            }
        );
    })
}

fn expand_first_load(input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    validate_fn_shape(&input)?;
    let referenced_types = validate_first_load_params(&input.sig.inputs)?;
    let wasvy_path = wasvy_path();
    let ident = input.sig.ident.clone();
    let register_ident = format_ident!("__wasvy_register_module_first_load_{}", ident);
    let export_name = syn::LitStr::new(&ident.to_string(), ident.span());
    let metadata_ident = format_ident!("__wasvy_module_first_load_types_{}", ident);
    let guest_impl_ident = format_ident!("__wasvy_guest_impl_{}", ident);
    let guest_export_ident = format_ident!("__wasvy_guest_export_{}", ident);
    let type_lits: Vec<syn::LitStr> = referenced_types
        .iter()
        .map(|name| syn::LitStr::new(name, proc_macro2::Span::call_site()))
        .collect();
    let guest_params = input
        .sig
        .inputs
        .iter()
        .map(|arg| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let pat = &arg.pat;
            let ty = guest_param_wrapper_tokens(arg, &wasvy_path)?;
            Ok::<_, syn::Error>(quote!(#pat: #ty))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_raw_params = input
        .sig
        .inputs
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let raw_ident = format_ident!("__wasvy_arg_{index}");
            let ty = guest_raw_param_type_tokens(arg)?;
            Ok::<_, syn::Error>(quote!(#raw_ident: #ty))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_wrappers = input
        .sig
        .inputs
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let pat = &arg.pat;
            let raw_ident = format_ident!("__wasvy_arg_{index}");
            let ctor = guest_wrapper_ctor_tokens(arg, &raw_ident, &wasvy_path)?;
            Ok::<_, syn::Error>(quote!(let #pat = #ctor;))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let guest_call_args = input
        .sig
        .inputs
        .iter()
        .map(|arg| {
            let FnArg::Typed(arg) = arg else {
                unreachable!("validated above")
            };
            let syn::Pat::Ident(pat_ident) = arg.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &arg.pat,
                    "Wasvy Module parameters must use identifier bindings",
                ));
            };
            let ident = &pat_ident.ident;
            Ok::<_, syn::Error>(quote!(#ident))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let body = &input.block;

    Ok(quote! {
        #input

        #[allow(non_upper_case_globals)]
        const #metadata_ident: &[&str] = &[#(#type_lits),*];

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            app.add_systems(#wasvy_path::module_guest::Startup, #ident);
        }

        #[cfg(target_arch = "wasm32")]
        fn #guest_impl_ident(#(#guest_params),*) #body

        #[cfg(target_arch = "wasm32")]
        fn #guest_export_ident(#(#guest_raw_params),*) {
            #(#guest_wrappers)*
            #guest_impl_ident(#(#guest_call_args),*);
        }

        #[doc(hidden)]
        #[used]
        #[unsafe(export_name = concat!("__wasvy_first_load_guard__", module_path!()))]
        static __WASVY_FIRST_LOAD_GUARD: u8 = 0;

        #wasvy_path::__wasvy_submit_module_first_load_registration!(
            #wasvy_path::authoring::WasvyModuleFirstLoadRegistration {
                scope: module_path!(),
                export_name: #export_name,
                register_native: #register_ident,
                referenced_types: #metadata_ident,
            }
        );
    })
}

fn validate_fn_shape(input: &ItemFn) -> syn::Result<()> {
    if !input.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.sig.generics,
            "Wasvy Module functions cannot be generic",
        ));
    }
    if input.sig.variadic.is_some() {
        return Err(syn::Error::new_spanned(
            &input.sig.variadic,
            "Wasvy Module functions cannot be variadic",
        ));
    }
    if !matches!(input.sig.output, ReturnType::Default) {
        return Err(syn::Error::new_spanned(
            &input.sig.output,
            "Wasvy Module functions must return ()",
        ));
    }
    Ok(())
}

fn validate_system_params(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
) -> syn::Result<Vec<String>> {
    let mut referenced = Vec::new();
    for arg in inputs {
        let FnArg::Typed(arg) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "Wasvy Module systems cannot take self receivers",
            ));
        };
        referenced.extend(classify_param(&arg.ty)?.referenced_type_paths());
    }
    Ok(referenced)
}

fn validate_first_load_params(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
) -> syn::Result<Vec<String>> {
    let referenced = Vec::new();
    for arg in inputs {
        let FnArg::Typed(arg) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "Wasvy Module first-load functions cannot take self receivers",
            ));
        };
        match classify_param(&arg.ty)? {
            ModuleParam::Commands => {}
            ModuleParam::Res(_) | ModuleParam::ResMut(_) | ModuleParam::Query(_) => {
                return Err(syn::Error::new_spanned(
                    &arg.ty,
                    "#[wasvy::on_first_load] currently supports Commands only",
                ));
            }
        }
    }
    Ok(referenced)
}

#[derive(Clone)]
enum ModuleParam {
    Commands,
    Query(ModuleQuerySignature),
    Res(String),
    ResMut(String),
}

#[derive(Clone)]
struct ModuleQuerySignature {
    items: Vec<ModuleQueryItem>,
}

#[derive(Clone)]
enum ModuleQueryItem {
    Ref(String),
    Mut(String),
    With(String),
    Without(String),
    Entity,
}

impl ModuleParam {
    fn referenced_type_paths(&self) -> Vec<String> {
        match self {
            ModuleParam::Commands => Vec::new(),
            ModuleParam::Res(name) | ModuleParam::ResMut(name) => vec![name.clone()],
            ModuleParam::Query(signature) => signature
                .items
                .iter()
                .filter_map(|item| match item {
                    ModuleQueryItem::Ref(name)
                    | ModuleQueryItem::Mut(name)
                    | ModuleQueryItem::With(name)
                    | ModuleQueryItem::Without(name) => Some(name.clone()),
                    ModuleQueryItem::Entity => None,
                })
                .collect(),
        }
    }
}

fn classify_param(ty: &Type) -> syn::Result<ModuleParam> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter type",
        ));
    };

    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter type",
        ));
    };

    match segment.ident.to_string().as_str() {
        "Commands" => Ok(ModuleParam::Commands),
        "Res" => Ok(ModuleParam::Res(extract_single_generic_type_path(segment)?)),
        "ResMut" => Ok(ModuleParam::ResMut(extract_single_generic_type_path(
            segment,
        )?)),
        "Query" => classify_query(segment),
        _ => Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter; supported params are Commands, Query<...>, Res<T>, and ResMut<T>",
        )),
    }
}

fn classify_query(segment: &syn::PathSegment) -> syn::Result<ModuleParam> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must use angle-bracketed arguments",
        ));
    };

    let mut args_iter = args.args.iter();
    let Some(GenericArgument::Type(data_ty)) = args_iter.next() else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must specify data, e.g. Query<&Health>",
        ));
    };

    let mut items = Vec::new();
    collect_query_data_types(data_ty, &mut items)?;

    if let Some(GenericArgument::Type(filter_ty)) = args_iter.next() {
        validate_query_filters(filter_ty, &mut items)?;
    }

    if args_iter.next().is_some() {
        return Err(syn::Error::new_spanned(
            segment,
            "Query supports at most data and filters type arguments",
        ));
    }

    Ok(ModuleParam::Query(ModuleQuerySignature { items }))
}

fn collect_query_data_types(ty: &Type, out: &mut Vec<ModuleQueryItem>) -> syn::Result<()> {
    match ty {
        Type::Reference(reference) => {
            let type_path = render_type_path(&reference.elem)?;
            if reference.mutability.is_some() {
                out.push(ModuleQueryItem::Mut(type_path));
            } else {
                out.push(ModuleQueryItem::Ref(type_path));
            }
            Ok(())
        }
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                match elem {
                    Type::Reference(reference) => {
                        let type_path = render_type_path(&reference.elem)?;
                        if reference.mutability.is_some() {
                            out.push(ModuleQueryItem::Mut(type_path));
                        } else {
                            out.push(ModuleQueryItem::Ref(type_path));
                        }
                    }
                    Type::Path(path)
                        if path
                            .path
                            .segments
                            .last()
                            .is_some_and(|seg| seg.ident == "Entity") =>
                    {
                        out.push(ModuleQueryItem::Entity);
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            elem,
                            "Query data only supports Entity, &T, &mut T, or tuples of those",
                        ));
                    }
                }
            }
            Ok(())
        }
        Type::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|seg| seg.ident == "Entity") =>
        {
            out.push(ModuleQueryItem::Entity);
            Ok(())
        }
        _ => Err(syn::Error::new_spanned(
            ty,
            "Query data only supports Entity, &T, &mut T, or tuples of those",
        )),
    }
}

fn validate_query_filters(ty: &Type, out: &mut Vec<ModuleQueryItem>) -> syn::Result<()> {
    match ty {
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                validate_single_query_filter(elem, out)?;
            }
            Ok(())
        }
        other => validate_single_query_filter(other, out),
    }
}

fn validate_single_query_filter(ty: &Type, out: &mut Vec<ModuleQueryItem>) -> syn::Result<()> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        ));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        ));
    };
    match segment.ident.to_string().as_str() {
        "With" => {
            out.push(ModuleQueryItem::With(extract_single_generic_type_path(
                segment,
            )?));
            Ok(())
        }
        "Without" => {
            out.push(ModuleQueryItem::Without(extract_single_generic_type_path(
                segment,
            )?));
            Ok(())
        }
        _ => Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        )),
    }
}

fn extract_single_generic_type_path(segment: &syn::PathSegment) -> syn::Result<String> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "expected angle-bracketed generic arguments",
        ));
    };
    let Some(GenericArgument::Type(inner)) = args.args.first() else {
        return Err(syn::Error::new_spanned(
            segment,
            "expected a single type argument",
        ));
    };
    if args.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            segment,
            "expected a single type argument",
        ));
    }
    render_type_path(inner)
}

fn extract_query_data_type(segment: &syn::PathSegment) -> syn::Result<Type> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must use angle-bracketed arguments",
        ));
    };
    let Some(GenericArgument::Type(inner)) = args.args.first() else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must specify data, e.g. Query<&Health>",
        ));
    };
    Ok(inner.clone())
}

fn render_type_path(ty: &Type) -> syn::Result<String> {
    match ty {
        Type::Path(_) => Ok(quote!(#ty).to_string().replace(' ', "")),
        _ => Err(syn::Error::new_spanned(ty, "expected a concrete type path")),
    }
}

#[derive(Clone)]
struct GuestModuleSource {
    systems: Vec<GuestModuleSystem>,
    first_load: Option<Ident>,
}

#[derive(Clone)]
struct GuestModuleSystem {
    ident: Ident,
    schedule: String,
    params: Vec<GuestSourceParam>,
}

#[derive(Clone)]
enum GuestSourceParam {
    Commands,
    Query(Vec<GuestQueryItem>),
    Res(Type),
    ResMut(Type),
}

#[derive(Clone)]
enum GuestQueryItem {
    Ref(Type),
    Mut(Type),
    With(Type),
    Without(Type),
    Entity,
}

fn guest_module_source() -> syn::Result<GuestModuleSource> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        syn::Error::new(proc_macro2::Span::call_site(), "CARGO_MANIFEST_DIR not set")
    })?;
    let manifest_dir = PathBuf::from(manifest_dir);
    let source_path = if manifest_dir.join("src/lib.rs").exists() {
        manifest_dir.join("src/lib.rs")
    } else {
        manifest_dir.join("src/main.rs")
    };
    let Ok(source) = std::fs::read_to_string(&source_path) else {
        return Ok(GuestModuleSource {
            systems: Vec::new(),
            first_load: None,
        });
    };
    let Ok(file) = syn::parse_file(&source) else {
        return Ok(GuestModuleSource {
            systems: Vec::new(),
            first_load: None,
        });
    };

    let mut systems = Vec::new();
    let mut first_load = None;

    for item in file.items {
        let Item::Fn(function) = item else {
            continue;
        };

        for attr in &function.attrs {
            if is_wasvy_attr_named(attr, "system") {
                let params = function
                    .sig
                    .inputs
                    .iter()
                    .map(|arg| match arg {
                        FnArg::Typed(arg) => classify_guest_param(&arg.ty),
                        FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
                            receiver,
                            "Wasvy Module systems cannot take self receivers",
                        )),
                    })
                    .collect::<syn::Result<Vec<_>>>()?;
                systems.push(GuestModuleSystem {
                    ident: function.sig.ident.clone(),
                    schedule: extract_wasvy_system_schedule(attr)?,
                    params,
                });
            }

            if is_wasvy_attr_named(attr, "on_first_load") {
                first_load = Some(function.sig.ident.clone());
            }
        }
    }

    Ok(GuestModuleSource {
        systems,
        first_load,
    })
}

fn classify_guest_param(ty: &Type) -> syn::Result<GuestSourceParam> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter type",
        ));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter type",
        ));
    };

    match segment.ident.to_string().as_str() {
        "Commands" => Ok(GuestSourceParam::Commands),
        "Res" => Ok(GuestSourceParam::Res(extract_single_generic_type(segment)?)),
        "ResMut" => Ok(GuestSourceParam::ResMut(extract_single_generic_type(
            segment,
        )?)),
        "Query" => classify_guest_query(segment),
        _ => Err(syn::Error::new_spanned(
            ty,
            "unsupported Wasvy Module parameter; supported params are Commands, Query<...>, Res<T>, and ResMut<T>",
        )),
    }
}

fn classify_guest_query(segment: &syn::PathSegment) -> syn::Result<GuestSourceParam> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must use angle-bracketed arguments",
        ));
    };
    let mut args_iter = args.args.iter();
    let Some(GenericArgument::Type(data_ty)) = args_iter.next() else {
        return Err(syn::Error::new_spanned(
            segment,
            "Query must specify data, e.g. Query<&Health>",
        ));
    };

    let mut items = Vec::new();
    collect_guest_query_data_types(data_ty, &mut items)?;
    if let Some(GenericArgument::Type(filter_ty)) = args_iter.next() {
        collect_guest_query_filters(filter_ty, &mut items)?;
    }
    if args_iter.next().is_some() {
        return Err(syn::Error::new_spanned(
            segment,
            "Query supports at most data and filters type arguments",
        ));
    }

    Ok(GuestSourceParam::Query(items))
}

fn collect_guest_query_data_types(ty: &Type, out: &mut Vec<GuestQueryItem>) -> syn::Result<()> {
    match ty {
        Type::Reference(reference) => {
            let inner = (*reference.elem).clone();
            if reference.mutability.is_some() {
                out.push(GuestQueryItem::Mut(inner));
            } else {
                out.push(GuestQueryItem::Ref(inner));
            }
            Ok(())
        }
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                match elem {
                    Type::Reference(reference) => {
                        let inner = (*reference.elem).clone();
                        if reference.mutability.is_some() {
                            out.push(GuestQueryItem::Mut(inner));
                        } else {
                            out.push(GuestQueryItem::Ref(inner));
                        }
                    }
                    Type::Path(path)
                        if path
                            .path
                            .segments
                            .last()
                            .is_some_and(|seg| seg.ident == "Entity") =>
                    {
                        out.push(GuestQueryItem::Entity);
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            elem,
                            "Query data only supports Entity, &T, &mut T, or tuples of those",
                        ));
                    }
                }
            }
            Ok(())
        }
        Type::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|seg| seg.ident == "Entity") =>
        {
            out.push(GuestQueryItem::Entity);
            Ok(())
        }
        _ => Err(syn::Error::new_spanned(
            ty,
            "Query data only supports Entity, &T, &mut T, or tuples of those",
        )),
    }
}

fn collect_guest_query_filters(ty: &Type, out: &mut Vec<GuestQueryItem>) -> syn::Result<()> {
    match ty {
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_guest_single_filter(elem, out)?;
            }
            Ok(())
        }
        other => collect_guest_single_filter(other, out),
    }
}

fn collect_guest_single_filter(ty: &Type, out: &mut Vec<GuestQueryItem>) -> syn::Result<()> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        ));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        ));
    };
    match segment.ident.to_string().as_str() {
        "With" => {
            out.push(GuestQueryItem::With(extract_single_generic_type(segment)?));
            Ok(())
        }
        "Without" => {
            out.push(GuestQueryItem::Without(extract_single_generic_type(
                segment,
            )?));
            Ok(())
        }
        _ => Err(syn::Error::new_spanned(
            ty,
            "Query filters only support With<T> and Without<T>",
        )),
    }
}

fn is_wasvy_attr_named(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

fn extract_wasvy_system_schedule(attr: &Attribute) -> syn::Result<String> {
    let syn::Meta::List(list) = &attr.meta else {
        return Err(syn::Error::new_spanned(
            attr,
            "#[wasvy::system(...)] requires a schedule, e.g. #[wasvy::system(Update)]",
        ));
    };
    Ok(list.tokens.to_string().replace(' ', ""))
}

fn guest_schedule_tokens(schedule: &str) -> syn::Result<proc_macro2::TokenStream> {
    let tokens = match schedule {
        "ModStartup" => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::ModStartup),
        "PreUpdate" => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::PreUpdate),
        "Update" => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::Update),
        "PostUpdate" => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::PostUpdate),
        "FixedPreUpdate" => {
            quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::FixedPreUpdate)
        }
        "FixedUpdate" => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::FixedUpdate),
        "FixedPostUpdate" => {
            quote!(__wasvy_guest_bindings::wasvy::ecs::app::Schedule::FixedPostUpdate)
        }
        _ => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "unsupported Wasvy guest schedule `{schedule}`; use ModStartup, PreUpdate, Update, PostUpdate, FixedPreUpdate, FixedUpdate, or FixedPostUpdate"
                ),
            ));
        }
    };
    Ok(tokens)
}

fn guest_wit_param_type(param: &GuestSourceParam) -> &'static str {
    match param {
        GuestSourceParam::Commands => "commands",
        GuestSourceParam::Query(_) => "query",
        GuestSourceParam::Res(_) | GuestSourceParam::ResMut(_) => "world-resource",
    }
}

fn guest_query_for_tokens(items: &[GuestQueryItem]) -> Vec<proc_macro2::TokenStream> {
    items
        .iter()
        .filter_map(|item| match item {
            GuestQueryItem::Ref(ty) => Some(quote! {
                __wasvy_guest_bindings::wasvy::ecs::app::QueryFor::Ref(<#ty as ::bevy_reflect::TypePath>::type_path().to_string())
            }),
            GuestQueryItem::Mut(ty) => Some(quote! {
                __wasvy_guest_bindings::wasvy::ecs::app::QueryFor::Mut(<#ty as ::bevy_reflect::TypePath>::type_path().to_string())
            }),
            GuestQueryItem::With(ty) => Some(quote! {
                __wasvy_guest_bindings::wasvy::ecs::app::QueryFor::With(<#ty as ::bevy_reflect::TypePath>::type_path().to_string())
            }),
            GuestQueryItem::Without(ty) => Some(quote! {
                __wasvy_guest_bindings::wasvy::ecs::app::QueryFor::Without(<#ty as ::bevy_reflect::TypePath>::type_path().to_string())
            }),
            GuestQueryItem::Entity => None,
        })
        .collect()
}

fn guest_param_wrapper_tokens(
    arg: &syn::PatType,
    wasvy_path: &proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let param = classify_param(&arg.ty)?;
    Ok(match param {
        ModuleParam::Commands => {
            quote!(#wasvy_path::module_guest::Commands<__wasvy_guest_bindings::wasvy::ecs::app::Commands>)
        }
        ModuleParam::Res(_) => {
            let Type::Path(path) = arg.ty.as_ref() else {
                unreachable!()
            };
            let segment = path.path.segments.last().expect("Res segment");
            let inner = extract_single_generic_type(segment)?;
            quote!(#wasvy_path::module_guest::Res<#inner, __wasvy_guest_bindings::wasvy::ecs::app::WorldResource>)
        }
        ModuleParam::ResMut(_) => {
            let Type::Path(path) = arg.ty.as_ref() else {
                unreachable!()
            };
            let segment = path.path.segments.last().expect("ResMut segment");
            let inner = extract_single_generic_type(segment)?;
            quote!(#wasvy_path::module_guest::ResMut<#inner, __wasvy_guest_bindings::wasvy::ecs::app::WorldResource>)
        }
        ModuleParam::Query(_) => {
            let Type::Path(path) = arg.ty.as_ref() else {
                unreachable!()
            };
            let segment = path.path.segments.last().expect("Query segment");
            let inner = extract_query_data_type(segment)?;
            quote!(#wasvy_path::module_guest::Query<#inner, __wasvy_guest_bindings::wasvy::ecs::app::Query>)
        }
    })
}

fn guest_raw_param_type_tokens(arg: &syn::PatType) -> syn::Result<proc_macro2::TokenStream> {
    let param = classify_param(&arg.ty)?;
    Ok(match param {
        ModuleParam::Commands => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Commands),
        ModuleParam::Query(_) => quote!(__wasvy_guest_bindings::wasvy::ecs::app::Query),
        ModuleParam::Res(_) | ModuleParam::ResMut(_) => {
            quote!(__wasvy_guest_bindings::wasvy::ecs::app::WorldResource)
        }
    })
}

fn guest_wrapper_ctor_tokens(
    arg: &syn::PatType,
    raw_ident: &Ident,
    wasvy_path: &proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let param = classify_param(&arg.ty)?;
    Ok(match param {
        ModuleParam::Commands => quote!(#wasvy_path::module_guest::Commands::new(#raw_ident)),
        ModuleParam::Query(_) => quote!(#wasvy_path::module_guest::Query::new(#raw_ident)),
        ModuleParam::Res(_) => quote!(#wasvy_path::module_guest::Res::new(#raw_ident)),
        ModuleParam::ResMut(_) => quote!(#wasvy_path::module_guest::ResMut::new(#raw_ident)),
    })
}

fn extract_single_generic_type(segment: &syn::PathSegment) -> syn::Result<Type> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "expected angle-bracketed generic arguments",
        ));
    };
    let Some(GenericArgument::Type(inner)) = args.args.first() else {
        return Err(syn::Error::new_spanned(
            segment,
            "expected a single type argument",
        ));
    };
    if args.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            segment,
            "expected a single type argument",
        ));
    }
    Ok(inner.clone())
}

fn generate_guest_wit(systems: &[GuestModuleSystem]) -> String {
    let mut wit = String::from(
        r#"package wasvy:modules-generated;

world guest {
    use wasvy:ecs/app@0.0.7.{app, commands, query, world-resource};

    export register: func(app: app);
    export on-first-load: func(commands: commands);
"#,
    );

    for system in systems {
        let params = system
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| format!("arg{index}: {}", guest_wit_param_type(param)))
            .collect::<Vec<_>>()
            .join(", ");
        wit.push_str(&format!("    export {}: func({params});\n", system.ident));
    }

    wit.push_str("}\n");
    wit
}

fn rust_ident(name: &str) -> Ident {
    let mut cleaned = String::new();
    for (i, ch) in name.chars().enumerate() {
        let c = if ch == '-' { '_' } else { ch };
        if i == 0 && c.is_ascii_digit() {
            cleaned.push('_');
        }
        cleaned.push(c);
    }
    Ident::new(&cleaned, proc_macro2::Span::call_site())
}

fn method_name(name: &str) -> String {
    let name = if let Some(pos) = name.rfind('.') {
        &name[pos + 1..]
    } else if let Some(pos) = name.rfind(']') {
        &name[pos + 1..]
    } else {
        name
    };
    name.to_string()
}

fn upper_camel(name: &str) -> String {
    let mut out = String::new();
    let mut capitalize = true;
    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            capitalize = true;
            continue;
        }
        if capitalize {
            out.push(ch.to_ascii_uppercase());
            capitalize = false;
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "Component".to_string()
    } else {
        out
    }
}

fn extract_wit_type_path(docs: &wit_parser::Docs) -> Option<String> {
    let contents = docs.contents.as_deref()?;
    for line in contents.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("wasvy:type-path=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_paths_from_stream(stream: proc_macro2::TokenStream) -> syn::Result<Vec<syn::LitStr>> {
    let mut paths = Vec::new();
    collect_paths(stream, &mut paths)?;
    if paths.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "missing `path`",
        ));
    }
    Ok(paths)
}

fn collect_paths(
    stream: proc_macro2::TokenStream,
    paths: &mut Vec<syn::LitStr>,
) -> syn::Result<()> {
    let mut iter = stream.into_iter().peekable();
    while let Some(tt) = iter.next() {
        match tt {
            proc_macro2::TokenTree::Group(group) => {
                collect_paths(group.stream(), paths)?;
            }
            proc_macro2::TokenTree::Ident(ident) if ident == "path" => {
                // Skip any punctuation until ':'
                while let Some(proc_macro2::TokenTree::Punct(p)) = iter.peek() {
                    if p.as_char() == ':' {
                        iter.next();
                        break;
                    }
                    iter.next();
                }

                let Some(next) = iter.next() else {
                    continue;
                };
                match next {
                    proc_macro2::TokenTree::Literal(lit) => {
                        if let Some(value) = lit_to_litstr(&lit) {
                            paths.push(value);
                        } else {
                            return Err(syn::Error::new(
                                lit.span(),
                                "path must be a string literal or array of string literals",
                            ));
                        }
                    }
                    proc_macro2::TokenTree::Group(group)
                        if group.delimiter() == proc_macro2::Delimiter::Bracket =>
                    {
                        for elem in group.stream() {
                            if let proc_macro2::TokenTree::Literal(lit) = elem {
                                if let Some(value) = lit_to_litstr(&lit) {
                                    paths.push(value);
                                } else {
                                    return Err(syn::Error::new(
                                        lit.span(),
                                        "path array entries must be string literals",
                                    ));
                                }
                            }
                        }
                    }
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "path must be a string literal or array of string literals",
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn lit_to_litstr(lit: &proc_macro2::Literal) -> Option<syn::LitStr> {
    syn::parse_str::<syn::LitStr>(&lit.to_string()).ok()
}

fn collect_rs_files(dir: &PathBuf, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().and_then(|s| s.to_str()) == Some("target") {
            continue;
        }
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn contains_wasvy_attr(contents: &str) -> bool {
    contents.contains("wasvy::component")
        || contents.contains("wasvy::methods")
        || contents.contains("WasvyComponent")
}

#[derive(Default)]
struct ModuleNode {
    file: Option<PathBuf>,
    children: std::collections::BTreeMap<String, ModuleNode>,
}

impl ModuleNode {
    fn insert(&mut self, segments: &[String], file: PathBuf) {
        if segments.is_empty() {
            self.file = Some(file);
            return;
        }
        let head = segments[0].clone();
        let tail = &segments[1..];
        let child = self.children.entry(head).or_default();
        child.insert(tail, file);
    }
}

fn render_modules(node: &ModuleNode) -> proc_macro2::TokenStream {
    render_module_node(None, node)
}

fn render_module_node(name: Option<&str>, node: &ModuleNode) -> proc_macro2::TokenStream {
    let mut items = Vec::new();
    if let Some(file) = &node.file {
        let lit = syn::LitStr::new(&file.to_string_lossy(), proc_macro2::Span::call_site());
        items.push(quote! {
            include!(#lit);
        });
    } else {
        for (child_name, child) in node.children.iter() {
            items.push(render_module_node(Some(child_name), child));
        }
    }

    if let Some(name) = name {
        let ident = rust_ident(name);
        quote! {
            mod #ident {
                #(#items)*
            }
        }
    } else {
        quote! {
            #(#items)*
        }
    }
}

fn module_segments(base: &Path, file: &Path) -> Result<Vec<String>, String> {
    let rel = file
        .strip_prefix(base)
        .map_err(|_| "file is not under base path".to_string())?;
    let file_name = rel
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid utf-8 path: {}", file.to_string_lossy()))?;

    let mut segments: Vec<String> = rel
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str().map(sanitize_ident))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if file_name == "lib.rs" || file_name == "main.rs" {
        segments.clear();
        return Ok(segments);
    }

    if file_name != "mod.rs" {
        let stem_path = PathBuf::from(file_name);
        let stem = stem_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "invalid file stem".to_string())?;
        segments.push(sanitize_ident(stem));
    }

    Ok(segments)
}

fn sanitize_ident(raw: &str) -> String {
    let mut cleaned = String::new();
    for (i, ch) in raw.chars().enumerate() {
        let c = if ch == '-' { '_' } else { ch };
        if i == 0 && c.is_ascii_digit() {
            cleaned.push('_');
        }
        cleaned.push(c);
    }
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

fn resolve_wit_path(path: &syn::LitStr) -> String {
    let path_value = path.value();
    let resolved_path = PathBuf::from(&path_value);
    let resolved_path = if resolved_path.is_absolute() {
        resolved_path
    } else if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(manifest_dir).join(resolved_path)
    } else {
        resolved_path
    };
    if resolved_path.exists() {
        return resolved_path.to_string_lossy().to_string();
    }

    if let Some(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR").ok().map(PathBuf::from) {
        let candidates = [
            manifest_dir.join("target/wasvy/components.wit"),
            manifest_dir.join("../target/wasvy/components.wit"),
            manifest_dir.join("../../target/wasvy/components.wit"),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    resolved_path.to_string_lossy().to_string()
}

fn resolve_wit_path_with_fallbacks(path: &syn::LitStr) -> String {
    let resolved_path = PathBuf::from(resolve_wit_path(path));
    if resolved_path.exists() {
        return resolved_path.to_string_lossy().to_string();
    }

    if let Some(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR").ok().map(PathBuf::from) {
        let candidates = [
            manifest_dir.join("target/wasvy/components.wit"),
            manifest_dir.join("../target/wasvy/components.wit"),
            manifest_dir.join("../../target/wasvy/components.wit"),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    resolved_path.to_string_lossy().to_string()
}

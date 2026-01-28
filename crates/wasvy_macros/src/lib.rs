//! Procedural macros for Wasvy component authoring and bindings.

use proc_macro::TokenStream;
use std::path::PathBuf;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, Ident, ImplItem, ImplItemFn, Item, ItemImpl, ItemStruct, Pat, PatIdent,
    Type, TypePath,
};
use wit_parser::{Resolve, WorldItem, FunctionKind, TypeDefKind};

/// Marker attribute for methods exported by `#[wasvy::methods]`.
///
/// Methods without this attribute are ignored by Wasvy.
///
/// # Example
/// ```ignore
/// #[wasvy::methods]
/// impl Health {
///     #[wasvy::method]
///     pub fn pct(&self) -> f32 {
///         self.current / self.max
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn method(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
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
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as Item);
    let wasvy_path = wasvy_path();

    let expanded = match input {
        Item::Struct(item) => expand_component_struct(item, &wasvy_path),
        Item::Enum(item) => {
            let ident = &item.ident;
            let fn_ident = format_ident!("__wasvy_component_type_path_{}", ident);
            let register_ident = format_ident!("__wasvy_register_component_{}", ident);
            quote! {
                #item

                impl #wasvy_path::authoring::WasvyComponent for #ident {}

                #[allow(non_snake_case)]
                fn #fn_ident() -> &'static str {
                    const RAW: &str = concat!(module_path!(), "::", stringify!(#ident));
                    const PREFIX: &str = "build_script_build::";
                    if let Some(rest) = RAW.strip_prefix(PREFIX) {
                        let fixed = format!("{}::{}", env!("CARGO_PKG_NAME"), rest);
                        Box::leak(fixed.into_boxed_str())
                    } else {
                        RAW
                    }
                }

                #wasvy_path::__wasvy_submit_component!(#wasvy_path::witgen::WitComponentInfo {
                    type_path: #fn_ident,
                    name: stringify!(#ident),
                });

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
/// Methods tagged with `#[wasvy::method]` are registered for dynamic invoke.
///
/// # Example
/// ```ignore
/// #[wasvy::methods]
/// impl Health {
///     #[wasvy::method]
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
    let mut registrations = Vec::new();
    let mut items = Vec::new();

    for item in input.items.into_iter() {
        match item {
            ImplItem::Fn(func) if has_wasvy_method_attr(&func.attrs) => {
                let (mut func, registration) = expand_method(func, &wasvy_path, &type_ident);
                func.attrs.retain(|attr| !is_wasvy_method_attr(attr));
                registrations.push(registration);
                items.push(ImplItem::Fn(func));
            }
            other => items.push(other),
        }
    }

    let impl_block = ItemImpl {
        items,
        ..input
    };

    let register_ident = format_ident!("__wasvy_register_methods_{}", type_ident);

    let expanded = quote! {
        #impl_block

        impl #wasvy_path::authoring::WasvyMethods for #type_ident {
            fn register_methods(registry: &mut #wasvy_path::methods::MethodRegistry) {
                #(#registrations)*
            }
        }

        #[allow(non_snake_case)]
        fn #register_ident(registry: &mut #wasvy_path::methods::MethodRegistry) {
            <#type_ident as #wasvy_path::authoring::WasvyMethods>::register_methods(registry);
        }

        #wasvy_path::__wasvy_submit_methods_registration!(
            #wasvy_path::authoring::WasvyMethodsRegistration { register: #register_ident }
        );
    };

    expanded.into()
}

fn expand_component_struct(item: ItemStruct, wasvy_path: &proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    let ident = &item.ident;
    let fn_ident = format_ident!("__wasvy_component_type_path_{}", ident);
    let register_ident = format_ident!("__wasvy_register_component_{}", ident);
    quote! {
        #item

        impl #wasvy_path::authoring::WasvyComponent for #ident {}

        #[allow(non_snake_case)]
        fn #fn_ident() -> &'static str {
            const RAW: &str = concat!(module_path!(), "::", stringify!(#ident));
            const PREFIX: &str = "build_script_build::";
            if let Some(rest) = RAW.strip_prefix(PREFIX) {
                let fixed = format!("{}::{}", env!("CARGO_PKG_NAME"), rest);
                Box::leak(fixed.into_boxed_str())
            } else {
                RAW
            }
        }

        #wasvy_path::__wasvy_submit_component!(#wasvy_path::witgen::WitComponentInfo {
            type_path: #fn_ident,
            name: stringify!(#ident),
        });

        #[allow(non_snake_case)]
        fn #register_ident(app: &mut #wasvy_path::authoring::App) {
            <#ident as #wasvy_path::authoring::WasvyComponent>::register(app);
        }

        #wasvy_path::__wasvy_submit_component_registration!(
            #wasvy_path::authoring::WasvyComponentRegistration { register: #register_ident }
        );
    }
}

fn expand_method(
    func: ImplItemFn,
    wasvy_path: &proc_macro2::TokenStream,
    type_ident: &Ident,
) -> (ImplItemFn, proc_macro2::TokenStream) {
    let sig = func.sig.clone();
    let method_ident = &sig.ident;

    let mut inputs = sig.inputs.iter();
    let receiver = inputs.next();

    let receiver = match receiver {
        Some(FnArg::Receiver(receiver)) => receiver,
        _ => {
            return (
                func,
                syn::Error::new_spanned(
                    sig,
                    "#[wasvy::method] requires a self receiver",
                )
                .to_compile_error(),
            );
        }
    };

    let is_mut = receiver.mutability.is_some();
    if receiver.reference.is_none() {
        return (
            func,
            syn::Error::new_spanned(
                receiver,
                "#[wasvy::method] requires &self or &mut self",
            )
            .to_compile_error(),
        );
    }

    let (arg_idents, arg_types) = collect_args(inputs);
    let args_tuple = tuple_type(&arg_types);
    let args_pattern = tuple_pattern(&arg_idents);

    let method_name = method_ident.to_string();
    let arg_name_tokens = arg_idents
        .iter()
        .map(|ident| quote!(stringify!(#ident)));
    let arg_type_tokens = arg_types.iter().map(|ty| quote!(stringify!(#ty)));
    let ret_type_tokens = match &sig.output {
        syn::ReturnType::Default => quote!("()"),
        syn::ReturnType::Type(_, ty) => quote!(stringify!(#ty)),
    };

    let type_fn_ident = format_ident!("__wasvy_method_type_path_{}_{}", type_ident, method_ident);
    let registration = if is_mut {
        quote! {
            registry.register_method_mut(#method_name, |target: &mut #type_ident, #args_pattern: #args_tuple| {
                target.#method_ident(#(#arg_idents),*)
            });

            #[allow(non_snake_case)]
            fn #type_fn_ident() -> &'static str {
                const RAW: &str = concat!(module_path!(), "::", stringify!(#type_ident));
                const PREFIX: &str = "build_script_build::";
                if let Some(rest) = RAW.strip_prefix(PREFIX) {
                    let fixed = format!("{}::{}", env!("CARGO_PKG_NAME"), rest);
                    Box::leak(fixed.into_boxed_str())
                } else {
                    RAW
                }
            }

            #wasvy_path::__wasvy_submit_method!(#wasvy_path::witgen::WitMethodInfo {
                type_path: #type_fn_ident,
                name: #method_name,
                arg_names: &[#(#arg_name_tokens),*],
                arg_types: &[#(#arg_type_tokens),*],
                ret: #ret_type_tokens,
                mutable: true,
            });
        }
    } else {
        quote! {
            registry.register_method_ref(#method_name, |target: &#type_ident, #args_pattern: #args_tuple| {
                target.#method_ident(#(#arg_idents),*)
            });

            #[allow(non_snake_case)]
            fn #type_fn_ident() -> &'static str {
                const RAW: &str = concat!(module_path!(), "::", stringify!(#type_ident));
                const PREFIX: &str = "build_script_build::";
                if let Some(rest) = RAW.strip_prefix(PREFIX) {
                    let fixed = format!("{}::{}", env!("CARGO_PKG_NAME"), rest);
                    Box::leak(fixed.into_boxed_str())
                } else {
                    RAW
                }
            }

            #wasvy_path::__wasvy_submit_method!(#wasvy_path::witgen::WitMethodInfo {
                type_path: #type_fn_ident,
                name: #method_name,
                arg_names: &[#(#arg_name_tokens),*],
                arg_types: &[#(#arg_type_tokens),*],
                ret: #ret_type_tokens,
                mutable: false,
            });
        }
    };

    (func, registration)
}

fn collect_args<'a>(inputs: impl Iterator<Item = &'a FnArg>) -> (Vec<Ident>, Vec<Type>) {
    let mut arg_idents = Vec::new();
    let mut arg_types = Vec::new();

    for (idx, arg) in inputs.enumerate() {
        let FnArg::Typed(pat_type) = arg else {
            continue;
        };

        let ident = match &*pat_type.pat {
            Pat::Ident(PatIdent { ident, .. }) => ident.clone(),
            _ => format_ident!("arg{idx}"),
        };

        arg_idents.push(ident);
        arg_types.push((*pat_type.ty).clone());
    }

    (arg_idents, arg_types)
}

fn tuple_type(types: &[Type]) -> Type {
    if types.is_empty() {
        syn::parse_quote!(())
    } else if types.len() == 1 {
        let ty = &types[0];
        syn::parse_quote!((#ty,))
    } else {
        syn::parse_quote!((#(#types),*))
    }
}

fn tuple_pattern(idents: &[Ident]) -> Pat {
    if idents.is_empty() {
        syn::parse_quote!(())
    } else if idents.len() == 1 {
        let ident = &idents[0];
        syn::parse_quote!((#ident,))
    } else {
        syn::parse_quote!((#(#idents),*))
    }
}

fn has_wasvy_method_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_wasvy_method_attr)
}

fn is_wasvy_method_attr(attr: &Attribute) -> bool {
    attr.path().segments.last().is_some_and(|seg| seg.ident == "method")
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
                    return Err(syn::Error::new(key.span(), format!("unknown key `{other}`")));
                }
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }

        Ok(Self {
            path: path.ok_or_else(|| input.error("missing `path`"))?,
            world: world.ok_or_else(|| input.error("missing `world`"))?,
            module: module.unwrap_or_else(|| Ident::new("components_bindings", proc_macro2::Span::call_site())),
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
        .ok_or_else(|| syn::Error::new(args.world.span(), "missing `components` interface import"))?;
    let interface = &resolve.interfaces[interface_id];
    let package_id = interface.package.ok_or_else(|| syn::Error::new(args.world.span(), "interface has no package"))?;
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

    let wasvy_component = syn::LitStr::new("wasvy:ecs/app.component", proc_macro2::Span::call_site());
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
                    let ret_tokens = quote!(::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>);
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
                    let invoke = render_invoke_body(&method_name, &function.params, function.result.as_ref(), &wasvy_path);
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

        let trait_path = quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::#trait_ident);
        impls.push(quote! {
            impl #trait_path for #wasvy_path::host::WasmHost {
                #(#methods)*
            }
        });
    }

    let trait_host_path = quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::Host);
    let add_to_linker_path = quote!(#module_ident::#pkg_namespace::#pkg_name::#interface_name::add_to_linker);

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
        let segments = module_segments(&base, path)
            .map_err(|err| syn::Error::new(args.path.span(), err))?;
        root.insert(&segments, path.clone());
    }

    let rendered = render_modules(&root);
    Ok(rendered)
}

fn render_params(
    resolve: &Resolve,
    params: &[(String, wit_parser::Type)],
    wasvy_path: &proc_macro2::TokenStream,
    is_constructor: bool,
) -> proc_macro2::TokenStream {
    let mut out = Vec::new();
    if !is_constructor {
        out.push(quote!(component: ::wasmtime::component::Resource<#wasvy_path::host::WasmComponent>));
    }
    for (name, ty) in params.iter().filter(|(name, _)| name != "self") {
        let ident = rust_ident(name);
        let ty_tokens = ty_to_tokens(resolve, ty, wasvy_path);
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
    params: &[(String, wit_parser::Type)],
    result: Option<&wit_parser::Type>,
    wasvy_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let arg_idents: Vec<Ident> = params
        .iter()
        .filter(|(name, _)| name != "self")
        .map(|(name, _)| rust_ident(name))
        .collect();
    let args_expr = if arg_idents.is_empty() {
        quote!(())
    } else {
        quote!((#(#arg_idents),*,))
    };
    let method_lit = syn::LitStr::new(method, proc_macro2::Span::call_site());
    match result {
        None => quote! {
            let params = serde_json::to_string(&#args_expr).expect("serialize params");
            let _ = #wasvy_path::host::invoke_component_method(self, component, #method_lit, &params)
                .expect("invoke method");
        },
        Some(_) => quote! {
            let params = serde_json::to_string(&#args_expr).expect("serialize params");
            let output = #wasvy_path::host::invoke_component_method(self, component, #method_lit, &params)
                .expect("invoke method");
            serde_json::from_str(&output).expect("deserialize")
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
                        ))
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
    contents.contains("wasvy::component") || contents.contains("wasvy::methods")
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

fn module_segments(base: &PathBuf, file: &PathBuf) -> Result<Vec<String>, String> {
    let rel = file
        .strip_prefix(base)
        .map_err(|_| "file is not under base path".to_string())?;
    let file_name = rel.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
        format!("invalid utf-8 path: {}", file.to_string_lossy())
    })?;

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

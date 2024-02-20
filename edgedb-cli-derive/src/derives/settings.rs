use proc_macro2::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{Data, DeriveInput};

use crate::types;

pub(crate) fn derive_edb_settings(input: &DeriveInput) -> TokenStream {
    let ident = &input.ident;

    match input.data {
        Data::Enum(e) => {
            let mut subcommands = Vec::new();
            for sub in e.variants {
                let ty = match sub.fields {
                    syn::Fields::Unit => None,
                    syn::Fields::Named(_) => {
                        abort!(sub,
                            "named fields are not supported for EdbClap");
                    },
                    syn::Fields::Unnamed(mut unn) => {
                        if unn.unnamed.len() != 1 {
                            abort!(unn, "single field required");
                        }
                        Some(unn.unnamed.pop().unwrap().into_value().ty)
                    }
                };
                subcommands.push(types::Subcommand {
                    attrs: attrib::SubcommandAttrs::from_syn(&sub.attrs),
                    ident: sub.ident,
                    ty,
                });
            };

            let e = &types::Enum {
                attrs,
                vis: e.vis,
                ident: e.ident,
                generics: e.generics,
                subcommands,
            };

            let setting = mk_setting_impl(&e);
            quote! {
                #setting
            }
        }
        _ => abort!(input.data, "`#[derive(EdbSettings]` only supports enums"),
    }
}

fn mk_setting_impl(e: &types::Enum) -> TokenStream {
    let ref ident = e.ident;
    let (impl_gen, ty_gen, where_cl) = e.generics.split_for_impl();
    let to_string = e.subcommands.iter().map(|sub| {
        let variant = &sub.ident;
        let name = ::heck::ToKebabCase::to_kebab_case(&variant.to_string()[..]);
        quote! {
            #ident::#variant(..) => #name
        }
    });
    let is_show = e.subcommands.iter().map(|sub| {
        let variant = &sub.ident;
        quote! {
            #ident::#variant(val) => val.value.is_none()
        }
    });
    let all_items = e.subcommands.iter().map(|sub| {
        let variant = &sub.ident;
        quote! {
            #ident::#variant(::std::default::Default::default())
        }
    });
    quote! {
        impl #impl_gen #ident #ty_gen #where_cl
        {
            pub fn name(&self) -> &'static str {
                match self {
                    #( #to_string ),*
                }
            }
            pub fn is_show(&self) -> bool {
                use Setting::*;

                match self {
                    #( #is_show ),*
                }
            }
            pub fn all_items() -> &'static [#ident] {
                static SETTINGS: ::once_cell::sync::OnceCell<Vec<#ident>>
                    = ::once_cell::sync::OnceCell::new();
                return &SETTINGS.get_or_init(|| {
                    vec![#( #all_items ),*]
                })[..];
            }
        }
    }
}

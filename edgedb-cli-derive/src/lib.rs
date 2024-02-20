extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use syn::{self, parse_macro_input};

mod attrib;
mod derives;
mod into_args;
mod kw;
mod types;

#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(EdbSettings)]
pub fn edgedb_edb_settings(input: TokenStream) -> TokenStream {
    let inp = parse_macro_input!(input as syn::Item);
    derives::derive_edb_settings(inp).into()
}

#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(IntoArgs, attributes(arg, command))]
pub fn edgedb_into_args(input: TokenStream) -> TokenStream {
    let inp = parse_macro_input!(input as syn::Item);
    derive_args(inp).into()
}

fn derive_args(item: syn::Item) -> proc_macro2::TokenStream {
    let attrs = match item {
        syn::Item::Struct(ref s) => &s.attrs,
        syn::Item::Enum(ref e) => &e.attrs,
        _ => abort!(item, "can only derive EdbClap for structs and enums"),
    };
    let attrs = attrib::ContainerAttrs::from_syn(&attrs);
    match item {
        syn::Item::Struct(s) => {
            let fields = match s.fields {
                syn::Fields::Named(f) => f.named.into_iter()
                    .map(|f| types::Field::new(
                        attrib::FieldAttrs::from_syn(&f.attrs),
                        f,
                    ))
                    .collect::<Vec<_>>(),
                _ => abort!(s, "only named fields are supported for EdbClap"),
            };
            into_args::structure(&types::Struct {
                attrs,
                vis: s.vis,
                ident: s.ident,
                generics: s.generics,
                fields,
            })
        }
        _ => abort!(item, "can only derive EdbClap for structs"),
    }
}

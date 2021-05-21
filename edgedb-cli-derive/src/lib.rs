extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::{abort};
use syn::{self, parse_macro_input};

mod attrib;
mod into_app;
mod kw;
mod types;

#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(EdbClap, attributes(edb, clap))]
pub fn edgedb_edb_clap(input: TokenStream) -> TokenStream {
    let inp = parse_macro_input!(input as syn::Item);
    derive(inp).into()
}

fn derive(item: syn::Item) -> proc_macro2::TokenStream {
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
            into_app::structure(&types::Struct {
                attrs,
                vis: s.vis,
                ident: s.ident,
                generics: s.generics,
                fields,
            })
        }
        syn::Item::Enum(e) => {
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
            into_app::subcommands(&types::Enum {
                attrs,
                vis: e.vis,
                ident: e.ident,
                generics: e.generics,
                subcommands,
            })
        }
        _ => abort!(item, "can only derive EdbClap for structs and enums"),
    }
}

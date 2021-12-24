use proc_macro2::{TokenStream};
use proc_macro_error::{abort};
use quote::{quote};

use crate::attrib::{ParserKind};
use crate::types;


pub fn structure(s: &types::Struct) -> TokenStream {
    use ParserKind::*;

    let ref ident = s.ident;
    let (impl_gen, ty_gen, where_cl) = s.generics.split_for_impl();

    let mut args = Vec::new();
    for field in &s.fields {
        let ident = &field.ident;
        if field.attrs.flatten {
            abort!(field.ident, "flatten is not implemented");
        } else if field.attrs.subcommand {
            abort!(field.ident, "subcommand is not implemented");
        } else if let Some(long) = &field.attrs.long {
            let long = String::from("--") +
                &long.as_ref()
                .map(|s| s.value().to_string())
                .unwrap_or_else(
                    || s.attrs.rename_all.convert(&field.ident.to_string()));
            if field.multiple {
                abort!(field.ident, "multiple is not implemented");
            }
            match field.parse.kind {
                FromOccurrences => {
                    abort!(field.ident, "occurrendes are not implemented");
                }
                FromStr | FromOsStr | TryFromStr | TryFromOsStr => {
                    if field.optional {
                        args.push(quote! {
                            if let Some(value) = &self.#ident {
                                process.arg(#long).args([value]);
                            }
                        });
                    // TODO(tailhook) maybe done, but the problem is that
                    //   default_value is a string
                    //} else if let Some(val) = &field.attrs.default_value {
                    //    args.push(quote! {
                    //        if self.#ident != #val {
                    //            process.arg(#long).arg(&self.#ident);
                    //        }
                    //    });
                    } else {
                        args.push(quote! {
                            process.arg(#long).args([&self.#ident]);
                        });
                    }
                }
                FromFlag => {
                    args.push(quote! {
                        if self.#ident {
                            process.arg(#long);
                        }
                    });
                }
            }
        } else if field.attrs.short.is_some() {
            abort!(field.ident,
                   "only long options and positionals are implemented");
        } else {
            if field.multiple {
                abort!(field.ident, "multiple is not implemented");
            }
            match field.parse.kind {
                FromOccurrences => {
                    abort!(field.ident, "occurrendes are not implemented");
                }
                FromStr | FromOsStr | TryFromStr | TryFromOsStr => {
                    if field.optional {
                        args.push(quote! {
                            if let Some(value) = self.#ident {
                                process.args([value]);
                            }
                        });
                    } else if let Some(val) = &field.attrs.default_value {
                        args.push(quote! {
                            if self.#ident != #val {
                                process.args([&self.#ident]);
                            }
                        });
                    } else {
                        args.push(quote! {
                            process.args([&self.#ident]);
                        });
                    }
                }
                FromFlag => {
                    abort!(field.ident,
                           "positional from_flag are not implemented");
                }
            }
        }
    }

    quote! {
        impl #impl_gen crate::process::IntoArgs
            for &'_ #ident #ty_gen #where_cl
        {
            fn add_args(self, process: &mut crate::process::Native) {
                #(#args)*
            }
        }
    }
}


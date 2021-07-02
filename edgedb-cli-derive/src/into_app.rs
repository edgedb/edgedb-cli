use std::env;

use proc_macro2::{Span, TokenStream};
use proc_macro_error::emit_error;
use quote::{quote, quote_spanned};

use crate::attrib::{Case, ParserKind};
use crate::types;


pub fn structure(s: &types::Struct) -> TokenStream {
    let name = env::var("CARGO_PKG_NAME").unwrap();

    let ref ident = s.ident;
    let (impl_gen, ty_gen, where_cl) = s.generics.split_for_impl();

    let app = syn::Ident::new("app", Span::call_site());
    let matches = syn::Ident::new("matches", Span::call_site());
    let dest = syn::Ident::new("dest", Span::call_site());

    let augment = mk_struct(&s, &app, false, true);
    let augment_for_update = mk_struct(&s, &app, true, true);
    let augment_no_inheritance = mk_struct(&s, &app, false, false);
    let from_matches = mk_struct_matches(&s, &matches);
    let update_matches = mk_struct_update_matches(&s, &matches);
    let propagate_args = mk_struct_propagate(&s, &dest, &matches);

    let help = s.attrs.help.as_ref().or(s.attrs.doc.as_ref())
        .map(|text| text.clap_text().value()).unwrap_or_else(String::new);
    let help_title = s.attrs.help.as_ref().or(s.attrs.doc.as_ref())
        .map(|text| text.formatted_title().value())
        .unwrap_or_else(String::new);
    let subcmds = if let Some(sub) =
        s.fields.iter().find(|s| s.attrs.subcommand)
    {
        let ty = &sub.ty;
        quote!(<#ty as crate::options::describe::DescribeEnum>::subcommands)
    } else {
        quote!(crate::options::describe::empty_subcommands)
    };

    quote! {
        impl #impl_gen clap::Clap for #ident #ty_gen #where_cl { }
        impl #impl_gen clap::IntoApp for #ident #ty_gen #where_cl {
            fn into_app<'help>() -> clap::App<'help> {
                <Self as clap::IntoApp>::augment_clap(clap::App::new(#name))
            }
            fn augment_clap(mut #app: clap::App<'_>) -> clap::App<'_> {
                #augment
                return #app;
            }
            fn into_app_for_update<'help>() -> clap::App<'help> {
                Self::augment_clap_for_update(clap::App::new(#name))
            }
            fn augment_clap_for_update(mut #app: clap::App<'_>)
                -> clap::App<'_>
            {
                #augment_for_update
                return #app;
            }
        }
        impl #impl_gen crate::commands::backslash::IntoApp
            for #ident #ty_gen #where_cl
        {
            fn into_app<'help>() -> clap::App<'help> {
                <Self as crate::commands::backslash::IntoApp>
                    ::augment_clap(clap::App::new(#name))
            }
            fn augment_clap(mut #app: clap::App<'_>) -> clap::App<'_> {
                #augment_no_inheritance
                return #app;
            }
        }
        impl #impl_gen clap::FromArgMatches for #ident #ty_gen #where_cl {
            fn from_arg_matches(#matches: &clap::ArgMatches) -> Self {
                #from_matches
            }
            fn update_from_arg_matches(&mut self, #matches: &clap::ArgMatches)
            {
                #update_matches
            }
        }
        impl #impl_gen crate::options::PropagateArgs
            for #ident #ty_gen #where_cl
        {
            fn propagate_args(&self, #dest: &mut ::anymap::AnyMap,
                #matches: &clap::ArgMatches)
            {
                #propagate_args
            }
        }
        impl #impl_gen crate::options::describe::Describe
            for #ident #ty_gen #where_cl
        {
            fn describe() -> crate::options::describe::Command {
                static COMMAND: crate::options::describe::Command =
                    crate::options::describe::Command {
                        help: #help,
                        help_title: #help_title,
                        describe_subcommands: #subcmds,
                    };
                return COMMAND.clone();
            }
        }
    }
}

pub fn subcommands(e: &types::Enum) -> TokenStream {
    let ref ident = e.ident;
    let (impl_gen, ty_gen, where_cl) = e.generics.split_for_impl();
    let app = syn::Ident::new("app", Span::call_site());
    let sub = syn::Ident::new("sub", Span::call_site());
    let augment = mk_subcommands(&e, &app, false, true);
    let augment_for_update = mk_subcommands(&e, &app, true, true);
    let augment_no_inheritance = mk_subcommands(&e, &app, false, false);
    let from_sub = mk_match_subcommand(&e, &sub);
    let propagation = mk_subcommand_propagation(&e);
    let describe_subcommands = mk_subcommand_describe(&e);
    let setting = if e.attrs.setting { mk_setting_impl(&e) } else { quote!() };
    quote! {
        impl #impl_gen clap::Subcommand for #ident #ty_gen #where_cl {
            fn from_subcommand(subcommand: Option<(&str, &clap::ArgMatches)>)
                -> Option<Self>
            {
                if let Some(#sub) = subcommand {
                    #from_sub
                } else {
                    None
                }
            }
            fn update_from_subcommand(&mut self,
                _: std::option::Option<(&str, &clap::ArgMatches)>)
            {
                todo!("update from subcommand")
            }
            fn augment_subcommands(mut #app: clap::App<'_>) -> clap::App<'_> {
                #augment
                return #app;
            }
            fn augment_subcommands_for_update(mut #app: clap::App<'_>)
                -> clap::App<'_>
            {
                #augment_for_update
                return #app;
            }
        }
        impl #impl_gen crate::options::describe::DescribeEnum
            for #ident #ty_gen #where_cl
        {
            fn subcommands() -> &'static [crate::options::describe::Subcommand]
            {
                #describe_subcommands
            }
        }

        impl #impl_gen crate::commands::backslash::Subcommand
            for #ident #ty_gen #where_cl
        {
            fn augment_subcommands(mut #app: clap::App<'_>) -> clap::App<'_> {
                #augment_no_inheritance
                return #app;
            }
        }
        #propagation
        #setting
    }
}

fn mk_arg(field: &types::Field, case: &Case) -> TokenStream {

    let arg = syn::Ident::new("arg", Span::call_site());
    let mut modifiers = TokenStream::new();
    let ident = &field.ident;
    let name = case.convert(&ident.to_string());

    if let Some(ch) = &field.attrs.short {
        modifiers.extend(quote! {
            #arg = #arg.short(#ch);
        });
    }
    match &field.attrs.long {
        Some(Some(long_name)) => {
            modifiers.extend(quote! {
                #arg = #arg.long(#long_name);
            });
        }
        Some(None) => {
            modifiers.extend(quote! {
                #arg = #arg.long(#name);
            });
        }
        None => {}
    }

    if field.parse.has_arg() {
        modifiers.extend(quote! {
            #arg = #arg.takes_value(true);
        });

        if !field.optional && field.attrs.default_value.is_none() {
            modifiers.extend(quote! {
                #arg = #arg.required(true);
            });
        }
    }
    if let Some(val) = &field.attrs.default_value {
        modifiers.extend(quote! {
            #arg = #arg.default_value(#val);
        });
    }

    if field.multiple {
        if field.attrs.long.is_some() || field.attrs.short.is_some() {
            modifiers.extend(quote! {
                #arg = #arg.multiple_occurrences(true);
            });
        } else {
            modifiers.extend(quote! {
                #arg = #arg.multiple_values(true);
            });
        }
    }
    if field.parse.kind == ParserKind::FromOccurrences {
        modifiers.extend(quote! {
            #arg = #arg.multiple_occurrences(true);
        });
    }

    if let Some(text) = field.attrs.help.as_ref().or(field.attrs.doc.as_ref()) {
        let formatted = text.clap_text();
        modifiers.extend(quote! {
            #arg = #arg.about(#formatted);
        });
    }
    if let Some(name) = field.attrs.name.as_ref() {
        modifiers.extend(quote! {
            #arg = #arg.value_name(#name);
        });
    }
    for (name, value) in &field.attrs.options {
        modifiers.extend(quote! {
            #arg = #arg.#name(#value);
        });
    }
    match field.parse.kind {
        ParserKind::TryFromStr => {
            let ty = &field.ty;
            let func = if let Some(func) = &field.parse.parser {
                quote! { #func }
            } else {
                quote! { std::str::FromStr::from_str }
            };
            modifiers.extend(quote! {
                #arg = #arg.validator(|v| {
                    #func(v).map(|_: #ty| ())
                });
            });
        }
        ParserKind::TryFromOsStr => {
            let ty = &field.ty;
            let func = if let Some(func) = &field.parse.parser {
                quote! { #func }
            } else {
                quote! { std::convert::From::from }
            };
            modifiers.extend(quote! {
                #arg = #arg.validator_os(|v| {
                    #func(v).map(|_: #ty| ())
                });
            });
        }
        _ => {}
    }

    return quote! {
        {
            let mut #arg = clap::Arg::new(stringify!(#ident));
            #modifiers
            #arg
        }
    }
}

fn mk_struct(s: &types::Struct, app: &syn::Ident,
    for_update: bool, inheritance: bool)
    -> TokenStream
{
    let mut output = TokenStream::new();
    // suppress version for subcommands
    for (name, value) in &s.attrs.options {
        output.extend(quote! {
            #app = #app.#name(#value);
        });
    }
    let (subcmd_interface, flat_interface) = if inheritance {
        (quote!(clap::Subcommand), quote!(clap::IntoApp))
    } else {
        (
            quote!(crate::commands::backslash::Subcommand),
            quote!(crate::commands::backslash::IntoApp),
        )
    };
    let subcommand_visited = false;
    for field in &s.fields {
        let arg = mk_arg(field, &s.attrs.rename_all);
        if field.attrs.flatten {
            let ty = &field.ty;
            if for_update {
                output.extend(quote! {
                    #app = <#ty as #flat_interface>
                        ::augment_clap_for_update(#app);
                });
            } else {
                output.extend(quote! {
                    #app = <#ty as #flat_interface>
                        ::augment_clap(#app);
                });
            }
        } else if field.attrs.subcommand {
            if subcommand_visited {
                emit_error!(field.ident, "only single subcommand allowed");
            }
            let ty = &field.ty;
            if !field.optional {
                output.extend(quote! {
                    #app = #app.setting(
                        clap::AppSettings::SubcommandRequiredElseHelp
                    );
                });
            }
            output.extend(quote! {
                #app = <#ty as #subcmd_interface>::augment_subcommands(#app);
            });
        } else {
            output.extend(quote! {
                #app = #app.arg(#arg);
            });
        }
    }
    return output;
}

fn mk_subcommands(s: &types::Enum, app: &syn::Ident,
    for_update: bool, inheritance: bool)
    -> TokenStream
{
    let mut output = TokenStream::new();
    // suppress version for subcommands
    for (name, value) in &s.attrs.options {
        output.extend(quote! {
            #app = #app.#name(#value);
        });
    }
    let (flat_interface, cmd_interface) = if inheritance {
        (quote!(clap::Subcommand), quote!(clap::IntoApp))
    } else {
        (
            quote!(crate::commands::backslash::Subcommand),
            quote!(crate::commands::backslash::IntoApp),
        )
    };
    for sub in &s.subcommands {
        if sub.attrs.flatten {
            let ty = &sub.ty;
            if for_update {
                output.extend(quote! {
                    #app = <#ty as #flat_interface>
                        ::augment_subcommands_for_update(#app);
                });
            } else {
                output.extend(quote! {
                    #app = <#ty as #flat_interface>::augment_subcommands(#app);
                });
            }
        } else {
            let isub = syn::Ident::new("sub", Span::call_site());
            let name = sub.attrs.name.clone()
                .unwrap_or_else(|| {
                    s.attrs.rename_all.convert(&sub.ident.to_string())
                });
            let cmd_def = mk_subcommand(sub, &isub);
            let opts = if let Some(ty) = &sub.ty {
                if for_update {
                    quote! {
                        #isub = <#ty as #cmd_interface>
                            ::augment_clap_for_update(#isub);
                    }
                } else {
                    quote! {
                        #isub = <#ty as #cmd_interface>::augment_clap(#isub);
                    }
                }
            } else {
                quote! {}
            };
            let inherit = if inheritance {
                s.attrs.inherit.iter().chain(&sub.attrs.inherit)
                .map(|ty| quote! {
                    #isub = <#ty as #cmd_interface>
                        ::augment_clap_for_update(#isub);
                })
                .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            output.extend(quote! {
                let mut #isub = clap::App::new(#name);
                #isub = #isub.setting(clap::AppSettings::DisableVersionFlag);
                #cmd_def
                #opts
                #( #inherit )*
                #app = #app.subcommand(#isub);
            });
        }
    }
    return output;
}

fn mk_subcommand(s: &types::Subcommand, sub: &syn::Ident)
    -> TokenStream
{
    let mut modifiers = TokenStream::new();

    if let Some(text) = s.attrs.about.as_ref().or(s.attrs.doc.as_ref()) {
        let formatted = text.clap_text();
        modifiers.extend(quote! {
            #sub = #sub.about(#formatted);
        });
    }
    for (name, value) in &s.attrs.options {
        modifiers.extend(quote! {
            #sub = #sub.#name(#value);
        });
    }
    if s.attrs.hidden {
        modifiers.extend(quote! {
            #sub = #sub.setting(clap::AppSettings::Hidden);
        });
    }

    return modifiers;
}

fn get_parser(fld: &types::Field, matches: &syn::Ident) -> TokenStream {
    use crate::attrib::ParserKind::*;

    let ref field_name = fld.ident;
    let func = match fld.parse.parser.as_ref() {
        Some(f) => quote!(#f),
        None => match fld.parse.kind {
            FromStr => quote!(std::convert::From::from),
            TryFromStr => quote!(std::str::FromStr::from_str),
            FromOsStr => quote!(std::convert::From::from),
            TryFromOsStr => quote!(std::convert::TryFrom::try_from),
            FromOccurrences | FromFlag
            => syn::Error::new(fld.parse.span,
                "parser is incompatible with field type"
               ).to_compile_error(),
        },
    };
    let parser = if fld.multiple {
        let inner = match fld.parse.kind {
            FromStr => quote! {
                #matches.values_of(stringify!(#field_name))
                .map(|v| v.into_iter().map(#func).collect())
                .unwrap_or_else(Vec::new)
            },
            FromOsStr => quote! {
                #matches.values_of_os(stringify!(#field_name))
                .map(|v| v.into_iter().map(#func).collect())
                .unwrap_or_else(Vec::new)
            },
            TryFromStr => quote! {
                #matches.values_of(stringify!(#field_name))
                .map(|v| {
                    v.into_iter().map(|v| #func(v).unwrap()).collect()
                })
                .unwrap_or_else(Vec::new)
            },
            TryFromOsStr => quote! {
                #matches.values_of_os(stringify!(#field_name))
                .map(|v| {
                    v.into_iter().map(|v| #func(v).unwrap()).collect()
                })
                .unwrap_or_else(Vec::new)
            },
            FromOccurrences | FromFlag
            => syn::Error::new(fld.parse.span,
                "parser is incompatible with field type"
               ).to_compile_error(),
        };
        if fld.optional {
            quote! {
                if #matches.is_present(stringify!(#field_name)) {
                    Some(#inner)
                } else {
                    None
                }
            }
        } else {
            inner
        }
    } else {
        let parser = match fld.parse.kind {
            FromStr => quote! {
                #matches.value_of(stringify!(#field_name))
                .map(#func)
            },
            FromOsStr => quote! {
                #matches.value_of_os(stringify!(#field_name))
                .map(#func)
            },
            TryFromStr => quote! {
                #matches.value_of(stringify!(#field_name))
                .map(|v| #func(v).expect("already validated"))
            },
            TryFromOsStr => quote! {
                #matches.value_of_os(stringify!(#field_name))
                .map(|v| #func(v).expect("already validated"))
            },
            FromOccurrences => {
                todo!("from_occurrences");
            }
            FromFlag => quote! {
                Some(#matches.is_present(stringify!(#field_name)))
            },
        };
        if fld.optional {
            parser
        } else {
            quote! {
                #parser.expect(concat!(
                    stringify!(#field_name),
                    " is non-optional",
                ))
            }
        }
    };
    return parser;
}

fn mk_struct_propagate(s: &types::Struct,
    dest: &syn::Ident, matches: &syn::Ident)
    -> TokenStream
{
    if let Some(subcmd) = s.fields.iter().find(|s| s.attrs.subcommand) {
        let subcmd_name = &subcmd.ident;
        if subcmd.optional {
            quote! {
                self.#subcmd_name.as_ref().zip(#matches.subcommand())
                .map(|(subcmd, (_name, args))| {
                    crate::options::PropagateArgs::propagate_args(
                        subcmd,
                        #dest,
                        args,
                    )
                });
            }
        } else {
            quote! {
                #matches.subcommand().map(|(_name, args)| {
                    crate::options::PropagateArgs::propagate_args(
                        &self.#subcmd_name,
                        #dest,
                        args,
                    )
                });
            }
        }
    } else {
        quote!()
    }
}
fn mk_struct_matches(s: &types::Struct, matches: &syn::Ident) -> TokenStream {
    let struct_name = &s.ident;
    let mut fields = Vec::with_capacity(s.fields.len());
    let mut inheritable = Vec::new();
    let mut subcmd = None;
    for fld in &s.fields {
        let field_name = &fld.ident;
        let ty = &fld.ty;
        if fld.attrs.subcommand {
            let parser = quote! {
                <#ty as clap::Subcommand>::from_subcommand(
                    #matches.subcommand()
                )
            };
            subcmd = Some(fld);
            if fld.optional {
                fields.push(quote! { let #field_name = #parser; });
            } else {
                fields.push(quote! { let #field_name = #parser.unwrap(); });
            }
        } else if fld.attrs.flatten {
            fields.push(quote! {
                let #field_name: #ty = clap::FromArgMatches::from_arg_matches(
                    #matches
                );
            });
            if fld.attrs.inheritable {
                inheritable.push((field_name, ty));
            }
        } else {
            let parser = get_parser(fld, matches);
            fields.push(quote_spanned! { fld.span =>
                let #field_name = #parser;
            });
        }
    }
    let propagate = if let Some(subcmd) = subcmd {
        let subcmd_name = &subcmd.ident;
        let tmap = syn::Ident::new("tmap", Span::call_site());
        let mut compose = Vec::with_capacity(inheritable.len()+1);
        let mut extract = Vec::with_capacity(inheritable.len());
        compose.push(quote! {
            let mut #tmap = ::anymap::AnyMap::new();
        });
        for (inh_name, ty) in inheritable {
            compose.push(quote! {
                #tmap.insert(#inh_name);
            });
            extract.push(quote! {
                let #inh_name = #tmap.remove::<#ty>().unwrap();
            });
        }
        if extract.is_empty() {
            quote!()
        } else if subcmd.optional {
            quote! {
                #( #compose; )*
                #subcmd_name.as_ref().zip(#matches.subcommand())
                .map(|(subcmd, (_name, args))| {
                    crate::options::PropagateArgs::propagate_args(
                        subcmd,
                        &mut #tmap,
                        args,
                    )
                });
                #( #extract; )*
            }
        } else {
            quote! {
                #( #compose; )*
                #matches.subcommand().map(|(_name, args)| {
                    crate::options::PropagateArgs::propagate_args(
                        &#subcmd_name,
                        &mut #tmap,
                        args,
                    )
                });
                #( #extract; )*
            }
        }
    } else {
        quote!()
    };
    let field_names = s.fields.iter().map(|f| &f.ident);
    quote! {
        #( #fields )*
        #propagate
        return #struct_name {
            #( #field_names ),*
        };
    }
}

fn mk_struct_update_matches(s: &types::Struct, matches: &syn::Ident)
    -> TokenStream
{
    let mut fields = Vec::with_capacity(s.fields.len());
    for fld in &s.fields {
        let field_name = &fld.ident;
        let ty = &fld.ty;
        if fld.attrs.subcommand {
            // TODO(tailhook) also add propagation of inherited options
            fields.push(quote!(todo!("update matches subcommand")));
        } else if fld.attrs.flatten {
            fields.push(quote_spanned! { fld.span =>
                <#ty as clap::FromArgMatches>::update_from_arg_matches(
                    &mut self.#field_name,
                    #matches,
                )
            });
        } else {
            let parser = get_parser(fld, matches);
            fields.push(quote_spanned! { fld.span =>
                if #matches.is_present(stringify!(#field_name)) {
                    self.#field_name = #parser;
                }
            });
        }
    };
    // TODO(tailhook) implement "inherits" when implementing subcommands
    quote! {
        #( #fields; )*
    }
}

fn mk_match_subcommand(s: &types::Enum, sub: &syn::Ident) -> TokenStream {
    let values = syn::Ident::new("values", Span::call_site());
    let mut branches = Vec::new();
    let mut flatten = Vec::new();
    let type_name = &s.ident;
    for subcmd in &s.subcommands {
        let ident = &subcmd.ident;
        if subcmd.attrs.flatten {
            let ty = &subcmd.ty;
            flatten.push(quote! {
                let flattened = <#ty as clap::Subcommand>::from_subcommand(
                    Some(#sub)
                );
                if let Some(fl_sub) = flattened {
                    return Some(#type_name::#ident(fl_sub));
                };
            });
        } else {
            let name = subcmd.attrs.name.clone()
                .unwrap_or_else(|| {
                    s.attrs.rename_all.convert(&ident.to_string())
                });
            match &subcmd.ty {
                Some(ty) => {
                    branches.push(quote! {
                        #name => {
                            Some(#type_name::#ident(
                                <#ty as clap::FromArgMatches>
                                ::from_arg_matches(#values)
                            ))
                        }
                    });
                }
                None => {
                    branches.push(quote! {
                        #name => Some(#type_name::#ident),
                    });
                }
            }
        }
    }
    quote! {
        let (name, #values) = #sub;
        match name {
            #(#branches)*
            _ => {
                #(#flatten)*
                return None;
            }
        }
    }
}

fn mk_subcommand_propagation(e: &types::Enum) -> TokenStream {
    let ref enum_name = e.ident;
    let gen = e.generics.clone();
    let (impl_gen, ty_gen, where_cl) = gen.split_for_impl();

    let dest = syn::Ident::new("dest", Span::call_site());
    let matches = syn::Ident::new("matches", Span::call_site());

    let propagate_global = e.attrs.inherit.iter().map(|ty| {
        quote! {
            if let Some(val) = #dest.get_mut::<#ty>() {
                ::clap::FromArgMatches::update_from_arg_matches(
                    val,
                    #matches,
                );
            };
        }
    });
    let match_branches = e.subcommands.iter().map(|sub| {
        let ident = &sub.ident;
        let (pattern, propagate) = if sub.ty.is_some() {
            let inner = syn::Ident::new("inner", Span::call_site());
            (
                quote! { #enum_name::#ident(#inner) },
                quote! {
                    crate::options::PropagateArgs::propagate_args(
                        #inner,
                        #dest,
                        #matches,
                    )
                }
            )
        } else {
            (
                quote! { #enum_name::#ident },
                quote!(),
            )
        };
        let inherit = sub.attrs.inherit.iter().map(|ty| {
            quote! {
                if let Some(val) = #dest.get_mut::<#ty>() {
                    ::clap::FromArgMatches::update_from_arg_matches(
                        val,
                        #matches,
                    );
                }
            }
        });
        quote! {
            #pattern => {
                #( #inherit; )*
                #propagate
            }
        }
    });

    return quote! {
        impl #impl_gen crate::options::PropagateArgs
            for #enum_name #ty_gen #where_cl
        {
            fn propagate_args(&self, #dest: &mut ::anymap::AnyMap,
                #matches: &clap::ArgMatches)
            {
                #( #propagate_global )*
                match self {
                    #( #match_branches ),*
                }
            }
        }
    };
}

fn subcmd_to_desc(sub: &types::Subcommand, e: &types::Enum) -> TokenStream {
    let name = sub.attrs.name.clone()
        .unwrap_or_else(|| {
            e.attrs.rename_all.convert(&sub.ident.to_string())
        });
    let about = sub.attrs.about.as_ref()
        .or(sub.attrs.doc.as_ref())
        .map(|a| a.clap_text())
        .map(|v| quote!(Some(#v)))
        .unwrap_or_else(|| quote!(None));
    let title = sub.attrs.about.as_ref()
        .or(sub.attrs.doc.as_ref())
        .map(|a| a.formatted_title())
        .map(|v| quote!(Some(#v)))
        .unwrap_or_else(|| quote!(None));
    let hidden = sub.attrs.hidden;
    let expand_help = sub.attrs.expand_help;
    let describe_inner = if let Some(ty) = &sub.ty {
        quote!(<#ty as crate::options::describe::Describe>::describe)
    } else {
        quote!(crate::options::describe::empty_command)
    };
    quote! {
        crate::options::describe::Subcommand {
            name: #name,
            override_about: #about,
            override_title: #title,
            hidden: #hidden,
            expand_help: #expand_help,
            describe_inner: #describe_inner,
        }
    }
}

fn mk_subcommand_describe(e: &types::Enum) -> TokenStream {
    if e.subcommands.iter().any(|s| s.attrs.flatten) {
        let capacity = e.subcommands.len();
        let vec = syn::Ident::new("vec", Span::call_site());
        let mut items = Vec::with_capacity(e.subcommands.len());
        for sub in &e.subcommands {
            if sub.attrs.flatten {
                let ty = &sub.ty;
                items.push(quote! {
                    #vec.extend(
                        <#ty as crate::options::describe::DescribeEnum>
                        ::subcommands()
                        .iter().cloned()
                    );
                });
            } else {
                let cmd = subcmd_to_desc(sub, e);
                items.push(quote! {
                    #vec.push(#cmd);
                });
            }
        }
        quote! {
            static ALL: ::once_cell::sync::OnceCell<
                Vec<crate::options::describe::Subcommand>
            > = ::once_cell::sync::OnceCell::new();
            return ALL.get_or_init(|| {
                let mut #vec = Vec::with_capacity(#capacity);
                #( #items )*
                return #vec;
            });
        }
    } else {
        let direct = e.subcommands.iter().map(|s| subcmd_to_desc(s, e));
        quote! {
            static SUBCOMMANDS: &[crate::options::describe::Subcommand] = &[
                #( #direct ),*
            ];
            return SUBCOMMANDS;
        }
    }
}

fn mk_setting_impl(e: &types::Enum) -> TokenStream {
    let ref ident = e.ident;
    let (impl_gen, ty_gen, where_cl) = e.generics.split_for_impl();
    let to_string = e.subcommands.iter().map(|sub| {
        let variant = &sub.ident;
        let name = ::heck::KebabCase::to_kebab_case(&variant.to_string()[..]);
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

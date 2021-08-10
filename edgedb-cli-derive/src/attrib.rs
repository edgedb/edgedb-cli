use std::convert::TryFrom;


use linked_hash_map::LinkedHashMap;
use proc_macro2::Span;
use proc_macro_error::{emit_error, ResultExt};
use syn::parse::{Parse, Parser, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Paren;

use crate::kw;


#[derive(Debug)]
pub enum FieldAttr {
    Default(syn::Ident),
    DefaultValue(syn::Expr),
    Parse(CliParse),
    Name(syn::LitStr),
    Flatten,
    Inheritable,
    Value {
        name: syn::Ident,
        value: syn::Expr,
    },
}

#[derive(Debug)]
pub enum ContainerAttr {
    Inherit(syn::Type),
    Default(syn::Ident),
    Setting,
    Value {
        name: syn::Ident,
        value: syn::Expr,
    },
}

#[derive(Debug)]
pub enum SubcommandAttr {
    Inherit(syn::Type),
    Hidden,
    ExpandHelp,
    Name(syn::LitStr),
    Default(syn::Ident),
    Value {
        name: syn::Ident,
        value: syn::Expr,
    },
}

pub struct Markdown {
    pub source: syn::LitStr,
}

pub enum Case {
    CamelCase,
    SnakeCase,
    KebabCase,
    ShoutySnakeCase,
    MixedCase,
    TitleCase,
    ShoutyKebabCase,
}

pub struct ContainerAttrs {
    pub doc: Option<Markdown>,
    pub before_help: Option<Markdown>,
    pub after_help: Option<Markdown>,
    pub help: Option<Markdown>,
    pub main: bool,
    pub setting: bool,
    pub rename_all: Case,
    pub inherit: Vec<syn::Type>,
    pub options: LinkedHashMap<syn::Ident, syn::Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserKind {
    FromStr,
    TryFromStr,
    FromOsStr,
    TryFromOsStr,
    FromOccurrences,
    FromFlag,
}

#[derive(Debug, Clone)]
pub struct CliParse {
    pub kind: ParserKind,
    pub parser: Option<syn::Expr>,
    pub span: Span,
}

pub struct FieldAttrs {
    pub name: Option<syn::LitStr>,
    pub doc: Option<Markdown>,
    pub long: Option<Option<syn::LitStr>>,
    pub short: Option<syn::LitChar>,
    pub help: Option<Markdown>,
    pub subcommand: bool,
    pub flatten: bool,
    pub inheritable: bool,
    pub parse: Option<CliParse>,
    pub default_value: Option<syn::Expr>,
    pub options: LinkedHashMap<syn::Ident, syn::Expr>,
}

pub struct SubcommandAttrs {
    pub name: Option<String>,
    pub doc: Option<Markdown>,
    pub about: Option<Markdown>,
    pub flatten: bool,
    pub hidden: bool,
    pub expand_help: bool,
    pub inherit: Vec<syn::Type>,
    pub options: LinkedHashMap<syn::Ident, syn::Expr>,
}

struct ContainerAttrList(pub Punctuated<ContainerAttr, syn::Token![,]>);
struct FieldAttrList(pub Punctuated<FieldAttr, syn::Token![,]>);
struct SubcommandAttrList(pub Punctuated<SubcommandAttr, syn::Token![,]>);

fn try_set<T, I>(dest: &mut T, value: I)
    where T: TryFrom<I>,
          <T as TryFrom<I>>::Error: Into<proc_macro_error::Diagnostic>,
{
    T::try_from(value)
    .map(|val| *dest = val)
    .map_err(|e| emit_error!(e.into())).ok();
}

fn try_set_opt<T, I>(dest: &mut Option<T>, value: I)
    where T: TryFrom<I>,
          <T as TryFrom<I>>::Error: Into<proc_macro_error::Diagnostic>,
{
    T::try_from(value)
    .map(|val| *dest = Some(val))
    .map_err(|e| emit_error!(e.into())).ok();
}

impl Parse for ContainerAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        use ContainerAttr::*;
        let lookahead = input.lookahead1();
        if lookahead.peek(kw::inherit) {
            let _kw: kw::inherit = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            let ty = content.parse()?;
            Ok(Inherit(ty))
        } else if lookahead.peek(kw::setting_impl) {
            let _kw: kw::setting_impl = input.parse()?;
            Ok(Setting)
        } else {
            let name: syn::Ident = input.parse()?;
            let lookahead = input.lookahead1();
            if lookahead.peek(Paren) {
                let content;
                syn::parenthesized!(content in input);
                let value = content.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let value: syn::Expr = input.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![,]) {
                Ok(Default(name))
            } else if input.cursor().eof() {
                Ok(Default(name))
            } else {
                Err(lookahead.error())
            }
        }
    }
}

impl Parse for FieldAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        use FieldAttr::*;

        let lookahead = input.lookahead1();
        if lookahead.peek(kw::parse) {
            let _parse: kw::parse = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            let kind = content.parse()?;
            let lookahead = content.lookahead1();
            if content.cursor().eof() {
                Ok(Parse(CliParse {
                    kind,
                    parser: None,
                    span: input.span(),
                }))
            } else if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = content.parse()?;
                let parser = content.parse()?;
                Ok(Parse(CliParse {
                    kind,
                    parser: Some(parser),
                    span: content.span(),
                }))
            } else {
                Err(lookahead.error())
            }
        } else if lookahead.peek(kw::name) {
            let _kw: kw::name = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let name = input.parse()?;
            Ok(Name(name))
        } else if lookahead.peek(kw::default_value) {
            let _kw: kw::default_value = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let value = input.parse()?;
            Ok(DefaultValue(value))
        } else if lookahead.peek(kw::flatten) {
            let _kw: kw::flatten = input.parse()?;
            Ok(Flatten)
        } else if lookahead.peek(kw::inheritable) {
            let _kw: kw::inheritable = input.parse()?;
            Ok(Inheritable)
        } else if lookahead.peek(syn::Ident) {
            let name: syn::Ident = input.parse()?;
            let lookahead = input.lookahead1();
            if lookahead.peek(Paren) {
                let content;
                syn::parenthesized!(content in input);
                let value = content.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let value: syn::Expr = input.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![,]) {
                Ok(Default(name))
            } else if input.cursor().eof() {
                Ok(Default(name))
            } else {
                Err(lookahead.error())
            }
        } else {
            Err(lookahead.error())
        }
    }
}

impl Parse for SubcommandAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        use SubcommandAttr::*;

        let lookahead = input.lookahead1();
        if lookahead.peek(kw::inherit) {
            let _kw: kw::inherit = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            let ty = content.parse()?;
            Ok(Inherit(ty))
        } else if lookahead.peek(kw::name) {
            let _kw: kw::name = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let val = input.parse()?;
            Ok(Name(val))
        } else if lookahead.peek(kw::hidden) {
            let _kw: kw::hidden = input.parse()?;
            Ok(Hidden)
        } else if lookahead.peek(kw::expand_help) {
            let _kw: kw::expand_help = input.parse()?;
            Ok(ExpandHelp)
        } else {
            let name: syn::Ident = input.parse()?;
            let lookahead = input.lookahead1();
            if lookahead.peek(Paren) {
                let content;
                syn::parenthesized!(content in input);
                let value = content.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let value: syn::Expr = input.parse()?;
                Ok(Value { name, value })
            } else if lookahead.peek(syn::Token![,]) {
                Ok(Default(name))
            } else if input.cursor().eof() {
                Ok(Default(name))
            } else {
                Err(lookahead.error())
            }
        }
    }
}

impl Parse for ContainerAttrList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::parse_terminated(input).map(ContainerAttrList)
    }
}

impl Parse for FieldAttrList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::parse_terminated(input).map(FieldAttrList)
    }
}

impl Parse for SubcommandAttrList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::parse_terminated(input).map(SubcommandAttrList)
    }
}

impl ContainerAttrs {
    pub fn from_syn(attrs: &[syn::Attribute]) -> ContainerAttrs {
        use ContainerAttr::*;

        let mut res = ContainerAttrs {
            doc: None,
            before_help: None,
            after_help: None,
            help: None,
            main: false,
            setting: false,
            inherit: Vec::new(),
            rename_all: Case::KebabCase,
            options: LinkedHashMap::new(),
        };
        for attr in attrs {
            if matches!(attr.style, syn::AttrStyle::Outer) &&
                (attr.path.is_ident("clap") || attr.path.is_ident("edb"))
            {
                let chunk: ContainerAttrList = match attr.parse_args() {
                    Ok(attr) => attr,
                    Err(e) => {
                        emit_error!(e);
                        continue;
                    }
                };
                for item in chunk.0 {
                    match item {
                        Inherit(ty) => {
                            res.inherit.push(ty);
                        }
                        Setting => {
                            res.setting = true;
                        }
                        Value { name, value } if name == "before_help" => {
                            try_set_opt(&mut res.before_help, value);
                        }
                        Value { name, value } if name == "after_help" => {
                            try_set_opt(&mut res.after_help, value);
                        }
                        Value { name, value } if name == "help" => {
                            try_set_opt(&mut res.help, value);
                        }
                        Value { name, value } if name == "rename_all" => {
                            try_set(&mut res.rename_all, value);
                        }
                        Value { name, value } => {
                            res.options.insert(name, value);
                        }
                        Default(name) if name == "main" => {
                            res.main = true;
                        }
                        Default(name) => {
                            emit_error!(&name, "expected `{}=value`", name);
                        }
                    }
                }
            } else if matches!(attr.style, syn::AttrStyle::Outer) &&
                attr.path.is_ident("doc")
            {
                if let Some(doc) = &mut res.doc {
                    doc.append_from_attr(attr);
                } else {
                    try_set_opt(&mut res.doc, attr);
                }
            }
        }
        res
    }
}

impl FieldAttrs {
    pub fn from_syn(attrs: &[syn::Attribute]) -> FieldAttrs {
        use FieldAttr::*;

        let mut res = FieldAttrs {
            name: None,
            doc: None,
            help: None,
            short: None,
            long: None,
            subcommand: false,
            flatten: false,
            inheritable: false,
            parse: None,
            default_value: None,
            options: LinkedHashMap::new(),
        };
        for attr in attrs {
            if matches!(attr.style, syn::AttrStyle::Outer) &&
                (attr.path.is_ident("clap") || attr.path.is_ident("edb"))
            {
                let chunk: FieldAttrList = match attr.parse_args() {
                    Ok(attr) => attr,
                    Err(e) => {
                        emit_error!(e);
                        continue;
                    }
                };
                for item in chunk.0 {
                    match item {
                        Value { name, value } if name == "help" => {
                            try_set_opt(&mut res.help, value);
                        }
                        Value { name, value } if name == "long" => {
                            match value {
                                syn::Expr::Lit(syn::ExprLit {
                                    lit: syn::Lit::Str(s),
                                    ..
                                }) => {
                                    res.long = Some(Some(s))
                                }
                                _ => emit_error!(value, "expected string"),
                            };
                        }
                        Value { name, value } if name == "short" => {
                            match value {
                                syn::Expr::Lit(syn::ExprLit {
                                    lit: syn::Lit::Char(s),
                                    ..
                                }) => {
                                    res.short = Some(s)
                                }
                                _ => emit_error!(value, "expected character"),
                            };
                        }
                        Value { name, value } => {
                            res.options.insert(name, value);
                        }
                        Default(name) if name == "long" => {
                            res.long = Some(None);
                        }
                        Flatten => {
                            res.flatten = true;
                        }
                        Inheritable => {
                            res.flatten = true;
                            res.inheritable = true;
                        }
                        Default(name) if name == "subcommand" => {
                            res.subcommand = true;
                        }
                        Default(name) => {
                            emit_error!(&name, "expected `{}=value`", name);
                        }
                        DefaultValue(val) => {
                            res.default_value = Some(val);
                        }
                        Parse(parse) => {
                            res.parse = Some(parse);
                        }
                        Name(name) => {
                            res.name = Some(name);
                        }
                    }
                }
            } else if matches!(attr.style, syn::AttrStyle::Outer) &&
                attr.path.is_ident("doc")
            {
                if let Some(doc) = &mut res.doc {
                    doc.append_from_attr(attr);
                } else {
                    try_set_opt(&mut res.doc, attr);
                }
            }
        }
        res
    }
}

impl SubcommandAttrs {
    pub fn from_syn(attrs: &[syn::Attribute]) -> SubcommandAttrs {
        use SubcommandAttr::*;

        let mut res = SubcommandAttrs {
            name: None,
            doc: None,
            about: None,
            flatten: false,
            expand_help: false,
            hidden: false,
            inherit: Vec::new(),
            options: LinkedHashMap::new(),
        };
        for attr in attrs {
            if matches!(attr.style, syn::AttrStyle::Outer) &&
                (attr.path.is_ident("clap") || attr.path.is_ident("edb"))
            {
                let chunk: SubcommandAttrList = match attr.parse_args() {
                    Ok(attr) => attr,
                    Err(e) => {
                        emit_error!(e);
                        continue;
                    }
                };
                for item in chunk.0 {
                    match item {
                        Inherit(ty) => res.inherit.push(ty),
                        Name(name) => res.name = Some(name.value()),
                        Hidden => res.hidden = true,
                        ExpandHelp => res.expand_help = true,
                        Value { name, value } if name == "about" => {
                            try_set_opt(&mut res.about, value);
                        }
                        Value { name, value } => {
                            res.options.insert(name, value);
                        }
                        Default(name) if name == "flatten" => {
                            res.flatten = true;
                        }
                        Default(name) => {
                            emit_error!(&name, "expected `{}=value`", name);
                        }
                    }
                }
            } else if matches!(attr.style, syn::AttrStyle::Outer) &&
                attr.path.is_ident("doc")
            {
                if let Some(doc) = &mut res.doc {
                    doc.append_from_attr(attr);
                } else {
                    try_set_opt(&mut res.doc, attr);
                }
            }
        }
        res
    }
}

impl Parse for Markdown {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Markdown { source: input.parse()? })
    }
}

impl TryFrom<syn::Expr> for Markdown {
    type Error = syn::Error;
    fn try_from(val: syn::Expr) -> syn::Result<Markdown> {
        match val {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), ..}) => {
                Ok(Markdown { source: s })
            }
            _ => {
                Err(syn::Error::new_spanned(val, "literal expected"))
            }
        }
    }
}
impl TryFrom<&'_ syn::Attribute> for Markdown {
    type Error = syn::Error;
    fn try_from(attr: &syn::Attribute) -> syn::Result<Markdown> {
        let parser = |input: ParseStream| {
            let lookahead = input.lookahead1();
            if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let source: syn::LitStr = input.parse()?;
                Ok(Markdown { source })
            } else {
                Err(syn::Error::new_spanned(attr, "`doc=` expected"))
            }
        };
        parser.parse2(attr.tokens.clone())
    }
}

impl Markdown {
    fn append_from_attr(&mut self, attr: &syn::Attribute) {
        let parser = |input: ParseStream| {
            let lookahead = input.lookahead1();
            if lookahead.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                let source: syn::LitStr = input.parse()?;
                self.source = syn::LitStr::new(
                    &(self.source.value() + "\n" + &source.value()),
                    self.source.span(),
                );
            } else {
                emit_error!(attr, "`doc=` expected")
            }
            Ok(())
        };
        parser.parse2(attr.tokens.clone()).unwrap_or_abort();
    }
}

impl TryFrom<syn::Expr> for Case {
    type Error = syn::Error;
    fn try_from(val: syn::Expr) -> syn::Result<Case> {
        match val {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), ..}) => {
                let case = match &s.value()[..] {
                    "CamelCase" => Case::CamelCase,
                    "snake_case" => Case::SnakeCase,
                    "kebab-case" => Case::KebabCase,
                    "SHOUTY_SNAKE_CASE" => Case::ShoutySnakeCase,
                    "mixedCase" => Case::MixedCase,
                    "Title Case" => Case::TitleCase,
                    "SHOUTY-KEBAB-CASE" => Case::ShoutyKebabCase,
                    _ => {
                        return Err(syn::Error::new_spanned(s,
                            &format!("undefined case conversion")));
                    }
                };
                Ok(case)
            }
            _ => {
                Err(syn::Error::new_spanned(val, "literal expected"))
            }
        }
    }
}

impl Case {
    pub fn convert(&self, s: &str) -> String {
        use Case::*;

        match self {
            CamelCase => heck::CamelCase::to_camel_case(s),
            SnakeCase => heck::SnakeCase::to_snake_case(s),
            KebabCase => heck::KebabCase::to_kebab_case(s),
            ShoutySnakeCase => heck::ShoutySnakeCase::to_shouty_snake_case(s),
            MixedCase => heck::MixedCase::to_mixed_case(s),
            TitleCase => heck::TitleCase::to_title_case(s),
            ShoutyKebabCase => heck::ShoutyKebabCase::to_shouty_kebab_case(s),
        }
    }
}

impl Parse for ParserKind {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(kw::from_str) {
            let _kw: kw::from_str = input.parse()?;
            Ok(ParserKind::FromStr)
        } else if lookahead.peek(kw::try_from_str) {
            let _kw: kw::try_from_str = input.parse()?;
            Ok(ParserKind::TryFromStr)
        } else if lookahead.peek(kw::from_os_str) {
            let _kw: kw::from_os_str = input.parse()?;
            Ok(ParserKind::FromOsStr)
        } else if lookahead.peek(kw::try_from_os_str) {
            let _kw: kw::try_from_os_str = input.parse()?;
            Ok(ParserKind::TryFromOsStr)
        } else if lookahead.peek(kw::from_occurrences) {
            let _kw: kw::from_occurrences = input.parse()?;
            Ok(ParserKind::FromOccurrences)
        } else if lookahead.peek(kw::from_flag) {
            let _kw: kw::from_flag = input.parse()?;
            Ok(ParserKind::FromFlag)
        } else {
            Err(lookahead.error())
        }
    }
}

impl CliParse {
    pub fn has_arg(&self) -> bool {
        use ParserKind::*;
        !matches!(self.kind, FromOccurrences | FromFlag)
    }
}

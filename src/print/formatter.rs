use crate::print::stream::Output;
use crate::print::Printer;

use colorful::core::color_string::CString;

use crate::print::buffer::Result;

use crate::print::style::Style;

pub(in crate::print) trait ColorfulExt {
    fn clear(&self) -> CString;
}

impl<'a> ColorfulExt for &'a str {
    fn clear(&self) -> CString {
        CString::new(*self)
    }
}


pub trait Formatter {
    type Error;
    fn const_number<T: ToString>(&mut self, s: T) -> Result<Self::Error>;
    fn const_string<T: ToString>(&mut self, s: T) -> Result<Self::Error>;
    fn const_uuid<T: ToString>(&mut self, s: T) -> Result<Self::Error>;
    fn const_bool<T: ToString>(&mut self, s: T) -> Result<Self::Error>;
    fn const_enum<T: ToString>(&mut self, s: T) -> Result<Self::Error>;
    fn nil(&mut self) -> Result<Self::Error>;
    fn typed<S: ToString>(&mut self, typ: &str, s: S) -> Result<Self::Error>;
    fn error<S: ToString>(&mut self, typ: &str, s: S) -> Result<Self::Error>;
    fn set<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn tuple<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn array<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn object<F>(&mut self, type_id: Option<&str>, f: F)
        -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn json_object<F>(&mut self, f: F)
        -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn named_tuple<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn call<F>(&mut self, name: &str, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn comma(&mut self) -> Result<Self::Error>;
    fn ellipsis(&mut self) -> Result<Self::Error>;
    fn object_field(&mut self, f: &str, linkprop: bool) -> Result<Self::Error>;
    fn tuple_field(&mut self, f: &str) -> Result<Self::Error>;

    fn implicit_properties(&self) -> bool;
    fn expand_strings(&self) -> bool;
    fn max_items(&self) -> Option<usize>;
}

impl<T: Output> Formatter for Printer<T> {
    type Error = T::Error;
    fn const_number<S: ToString>(&mut self, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::Number, &s.to_string()))
    }
    fn const_string<S: ToString>(&mut self, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::String, &s.to_string()))
    }
    fn const_uuid<S: ToString>(&mut self, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::UUID, &s.to_string()))
    }
    fn const_bool<S: ToString>(&mut self, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::Boolean, &s.to_string()))
    }
    fn const_enum<S: ToString>(&mut self, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::Enum, &s.to_string()))
    }
    fn nil(&mut self) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::SetLiteral, "{}"))
    }
    fn typed<S: ToString>(&mut self, typ: &str, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::Cast, &format!("<{}>", typ)))?;
        self.write(self.styler.apply(
            Style::String, &format!("'{}'", s.to_string().escape_default())
        ))
    }
    fn error<S: ToString>(&mut self, typ: &str, s: S) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(
            Style::Error, &format!("<err-{}>", typ)
        ))?;
        self.write(self.styler.apply(
            Style::Error, &format!("'{}'", s.to_string().escape_default())
        ))
    }
    fn set<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::SetLiteral, "{"),
            f,
            self.styler.apply(Style::SetLiteral, "}"),
        )?;
        Ok(())
    }
    fn comma(&mut self) -> Result<Self::Error> {
        Printer::comma(self)
    }
    fn ellipsis(&mut self) -> Result<Self::Error> {
        Printer::ellipsis(self)
    }
    fn object<F>(&mut self, type_name: Option<&str>, f: F)
        -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        match type_name {
            Some(type_name) => {
                if type_name == "std::FreeObject" {
                    self.block(
                        self.styler.apply(Style::ObjectLiteral, "{"),
                        f,
                        self.styler.apply(Style::ObjectLiteral, "}"),
                    )?;
                } else {
                    self.block(
                        self.styler.apply(Style::ObjectLiteral,
                                          &(String::from(type_name) + " {")),
                        f,
                        self.styler.apply(Style::ObjectLiteral, "}"),
                    )?;
                }
            }
            _ => {
                self.block(
                    self.styler.apply(Style::ObjectLiteral, "Object {"),
                    f,
                    self.styler.apply(Style::ObjectLiteral, "}"),
                )?;
            }
        }
        Ok(())
    }
    fn json_object<F>(&mut self, f: F)
        -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::ObjectLiteral, "{"),
            f,
            self.styler.apply(Style::ObjectLiteral, "}"),
        )?;
        Ok(())
    }
    fn object_field(&mut self, f: &str, linkprop: bool) -> Result<Self::Error> {
        self.delimit()?;
        if linkprop {
            self.write(self.styler.apply(Style::ObjectLinkProperty, f))?;
        } else {
            self.write(self.styler.apply(Style::ObjectPointer, f))?;
        }
        self.field()?;
        Ok(())
    }
    fn tuple<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::TupleLiteral, "("),
            f,
            self.styler.apply(Style::TupleLiteral, ")"),
        )?;
        Ok(())
    }
    fn named_tuple<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::TupleLiteral, "("),
            f,
            self.styler.apply(Style::TupleLiteral, ")"),
        )?;
        Ok(())
    }
    fn call<F>(&mut self, name: &str, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::TupleLiteral, &format!("{}(", name)),
            f,
            self.styler.apply(Style::TupleLiteral, ")"),
        )?;
        Ok(())
    }
    fn tuple_field(&mut self, f: &str) -> Result<Self::Error> {
        self.delimit()?;
        self.write(self.styler.apply(Style::TupleField, f))?;
        self.write(self.styler.apply(Style::TupleLiteral, " := "))?;
        Ok(())
    }
    fn array<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        self.block(
            self.styler.apply(Style::ArrayLiteral, "["),
            f,
            self.styler.apply(Style::ArrayLiteral, "]"),
        )?;
        Ok(())
    }

    fn implicit_properties(&self) -> bool {
        self.implicit_properties
    }

    fn expand_strings(&self) -> bool {
        self.expand_strings
    }

    fn max_items(&self) -> Option<usize> {
        self.max_items
    }
}

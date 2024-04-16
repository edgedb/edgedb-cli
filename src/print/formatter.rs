use crate::print::stream::Output;
use crate::print::Printer;

use colorful::core::color_string::CString;

use crate::print::buffer::{Result, Exception};

use crate::print::style::Style;
use crate::repl::VectorLimit;

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
    #[allow(dead_code)]
    fn error<S: ToString>(&mut self, typ: &str, s: S) -> Result<Self::Error>;
    fn set<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn tuple<F>(&mut self, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn array<F>(&mut self, type_name: Option<&str>, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>;
    fn auto_sized_vector<'x>(&mut self,
                             iter: impl IntoIterator<Item=&'x f32> + Copy)
        -> Result<Self::Error>;
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
    fn max_vector_length(&self) -> VectorLimit;

}

impl<T: Output> Formatter for Printer<T>
    where T::Error: std::fmt::Debug,
{
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
    fn array<F>(&mut self, type_name: Option<&str>, f: F) -> Result<Self::Error>
        where F: FnMut(&mut Self) -> Result<Self::Error>
    {
        self.delimit()?;
        if let Some(type_name) = type_name {
            self.block(
                self.styler.apply(Style::ArrayLiteral,
                                  &format!("<{type_name}>[")),
                f,
                self.styler.apply(Style::ArrayLiteral, "]"),
            )?;
        } else {
            self.block(
                self.styler.apply(Style::ArrayLiteral, "["),
                f,
                self.styler.apply(Style::ArrayLiteral, "]"),
            )?;
        }
        Ok(())
    }
    fn auto_sized_vector<'x>(&mut self,
                             iter: impl IntoIterator<Item=&'x f32> + Copy)
        -> Result<Self::Error>
    {
        self.delimit()?;
        let flag = self.open_block(
            self.styler.apply(Style::ArrayLiteral, "<ext::pgvector::vector>[")
        )?;
        let close = self.styler.apply(Style::ArrayLiteral, "]");
        if self.flow {
            let mut printed = 0;
            let mut savepoint = (self.buffer.len(), self.column);
            let mut first_try = || {
                for item in iter {
                    self.const_number(item)?;
                    self.comma()?;
                    let col_left = self.max_width.saturating_sub(self.column);
                    if col_left > ", ...],".len() {
                        savepoint = (self.buffer.len(), self.column);
                    } else {
                        return Err(Exception::DisableFlow);
                    }
                    printed += 1;
                }
                Ok(())
            };
            match first_try().and_then(|()| self.close_block(&close, flag)) {
                Ok(()) => {}
                Err(Exception::DisableFlow) if flag => {
                    if printed >= 3 {
                        self.buffer.truncate(savepoint.0);
                        self.column = savepoint.1;
                        let tmp_res = self.delimit()
                            .and_then(|()| self.write("...".clear()))
                            .and_then(|()| self.close_block(&close, flag));
                        match tmp_res {
                            Ok(()) => return Ok(()),
                            Err(Exception::DisableFlow) if flag => {}
                            Err(e) => return Err(e)?,
                        }
                    }
                    self.reopen_block()?;
                    let mut iter = iter.into_iter();
                    for item in iter.by_ref().take(3) {
                        self.const_number(item)?;
                        self.comma()?;
                    }
                    if iter.next().is_some() {
                        self.delimit()?;
                        self.write("...\n".clear())?;
                    }
                    self.close_block(&close, flag)?;
                }
                Err(e) => return Err(e)?,
            }
        } else {
            let mut iter = iter.into_iter();
            for item in iter.by_ref().take(3) {
                self.const_number(item)?;
                self.comma()?;
            }
            if iter.next().is_some() {
                self.delimit()?;
                self.write("...".clear())?;
            }
            self.close_block(&close, flag)?;
        }
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

    fn max_vector_length(&self) -> VectorLimit {
        self.max_vector_length
    }
}

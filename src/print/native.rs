use std::cmp::min;
use std::convert::TryInto;

use bigdecimal::BigDecimal;
use colorful::Colorful;
use chrono::format::{Item, Numeric, Pad, Fixed};
use chrono::{NaiveDateTime, NaiveDate, NaiveTime};
use humantime::format_rfc3339;
use num_bigint::BigInt;

use edgedb_protocol::value::Value;
use crate::print::formatter::Formatter;
use crate::print::buffer::Result;


static DATETIME_FORMAT: &[Item<'static>] = &[
    Item::Numeric(Numeric::Year, Pad::Zero),
    Item::Literal("-"),
    Item::Numeric(Numeric::Month, Pad::Zero),
    Item::Literal("-"),
    Item::Numeric(Numeric::Day, Pad::Zero),
    Item::Literal("T"),
    Item::Numeric(Numeric::Hour, Pad::Zero),
    Item::Literal(":"),
    Item::Numeric(Numeric::Minute, Pad::Zero),
    Item::Literal(":"),
    Item::Numeric(Numeric::Second, Pad::Zero),
    Item::Fixed(Fixed::Nanosecond),
];
static DATE_FORMAT: &[Item<'static>] = &[
    Item::Numeric(Numeric::Year, Pad::Zero),
    Item::Literal("-"),
    Item::Numeric(Numeric::Month, Pad::Zero),
    Item::Literal("-"),
    Item::Numeric(Numeric::Day, Pad::Zero),
];
static TIME_FORMAT: &[Item<'static>] = &[
    Item::Numeric(Numeric::Hour, Pad::Zero),
    Item::Literal(":"),
    Item::Numeric(Numeric::Minute, Pad::Zero),
    Item::Literal(":"),
    Item::Numeric(Numeric::Second, Pad::Zero),
    Item::Fixed(Fixed::Nanosecond),
];

pub trait FormatExt {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error>;
}

fn escape_string(s: &str, expanded: bool) -> String {
    let mut buf = String::with_capacity(s.len()+2);
    buf.push('\'');
    for c in s.chars() {
        match c {
            '\x00'..='\x08' | '\x0B' | '\x0C'
            | '\x0E'..='\x1F' | '\x7F'
            => buf.push_str(&format!("\\x{:02x}", c as u32)),
            '\u{0080}'..='\u{009F}'
            => buf.push_str(&format!("\\u{:04x}", c as u32)),
            '\'' => buf.push_str("\\'"),
            '\r' if !expanded => buf.push_str("\\r"),
            '\n' if !expanded => buf.push_str("\\n"),
            '\t' if !expanded => buf.push_str("\\t"),
            _ => buf.push(c),
        }
    }
    buf.push('\'');
    return buf;
}

fn format_bigint(bint: BigInt) -> String {
    let txt = bint.to_string();
    let no_zeros = txt.trim_end_matches('0');
    let zeros = txt.len() - no_zeros.len();
    if zeros > 5 {
        return format!("{}e{}n", no_zeros, zeros);
    } else {
        return format!("{}n", txt);
    }
}

fn format_decimal(value: BigDecimal) -> String {
    let txt = value.to_string();
    if txt.contains('.') {
        if txt.starts_with("0.00000") {
            let no_zeros = txt[2..].trim_start_matches('0');
            let zeros = txt.len()-2 - no_zeros.len();
            return format!("0.{}e-{}", no_zeros, zeros);
        } else {
            return format!("{}n", txt);
        }
    } else {
        let no_zeros = txt.trim_end_matches('0');
        let zeros = txt.len() - no_zeros.len();
        if zeros > 5 {
            return format!("{}.0e{}n", no_zeros, zeros);
        } else {
            return format!("{}.0n", txt);
        }
    }
}

impl FormatExt for Value {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error> {
        use Value as V;
        match self {
            V::Nothing => prn.const_scalar("Nothing"),
            V::Uuid(u) => prn.const_scalar(u),
            V::Str(s) => {
                prn.const_scalar(escape_string(s, prn.expand_strings()))
            }
            V::Bytes(b) => prn.const_scalar(format_args!("{:?}", b)),
            V::Int16(v) => prn.const_scalar(v),
            V::Int32(v) => prn.const_scalar(v),
            V::Int64(v) => prn.const_scalar(v),
            V::Float32(v) => prn.const_scalar(v),
            V::Float64(v) => prn.const_scalar(v),
            V::BigInt(v) => prn.const_scalar(format_bigint(v.into())),
            V::Decimal(v) => prn.const_scalar(format_decimal(v.into())),
            V::Bool(v) => prn.const_scalar(v),
            V::Datetime(t) => prn.typed("datetime", format_rfc3339(*t)),
            V::LocalDatetime(dt) => {
                match TryInto::<NaiveDateTime>::try_into(dt) {
                    Ok(naive) => prn.typed("cal::local_datetime",
                        naive.format_with_items(DATETIME_FORMAT.iter())),
                    Err(e) => prn.error("cal::local_datetime", e),
                }
            }
            V::LocalDate(d) => {
                match TryInto::<NaiveDate>::try_into(d) {
                    Ok(naive) => prn.typed("cal::local_date",
                        naive.format_with_items(DATE_FORMAT.iter())),
                    Err(e) => prn.error("cal::local_date", e),
                }
            }
            V::LocalTime(t) => {
                prn.typed("cal::local_time",
                    Into::<NaiveTime>::into(t)
                        .format_with_items(TIME_FORMAT.iter()))
            }
            V::Duration(d) => {
                // TODO(tailhook) implement more DB-like duration display
                prn.const_scalar(format_args!("{}{:?}",
                    if d.is_negative() { "-" } else { "" }, d.abs_duration()))
            }
            V::Json(d) => prn.const_scalar(format!("{:?}", d)),
            V::Set(items) => {
                prn.set(|prn| {
                    if let Some(limit) = prn.max_items() {
                        for item in &items[..min(limit, items.len())] {
                            item.format(prn)?;
                            prn.comma()?;
                        }
                        if items.len() > limit {
                            prn.ellipsis()?;
                        }
                    } else {
                        for item in items {
                            item.format(prn)?;
                            prn.comma()?;
                        }
                    }
                    Ok(())
                })
            },
            V::Object { shape, fields } => {
                // TODO(tailhook) optimize it on no-implicit-types
                //                or just cache typeid index on shape
                let type_id = shape.elements
                    .iter().zip(fields)
                    .find(|(f, _) | f.name == "__tid__")
                    .and_then(|(_, v)| if let Some(Value::Uuid(type_id)) = v {
                        Some(type_id)
                    } else {
                        None
                    });
                prn.object(type_id, |prn| {
                    let mut n = 0;
                    for (fld, value) in shape.elements.iter().zip(fields) {
                        if !fld.flag_implicit || prn.implicit_properties() {
                            if fld.flag_link_property {
                                prn.object_field(
                                    ("@".to_owned() + &fld.name)
                                    .rgb(0, 0xa5, 0xcb).bold()
                                )?;
                            } else {
                                prn.object_field(
                                    fld.name.clone().light_blue().bold())?;
                            };
                            value.format(prn)?;
                            prn.comma()?;
                            n += 1;
                        }
                    }
                    if n == 0 {
                        if let Some((fld, value)) = shape.elements
                            .iter().zip(fields)
                            .find(|(f, _) | f.name == "id")
                        {
                            prn.object_field(
                                fld.name.clone().light_blue().bold())?;
                            value.format(prn)?;
                            prn.comma()?;
                        }
                    }
                    Ok(())
                })
            }
            V::Tuple(items) => {
                prn.tuple(|prn| {
                    for item in items {
                        item.format(prn)?;
                        prn.comma()?;
                    }
                    Ok(())
                })
            }
            V::NamedTuple { shape, fields } => {
                prn.named_tuple(|prn| {
                    for (fld, value) in shape.elements.iter().zip(fields) {
                        prn.tuple_field(&fld.name)?;
                        value.format(prn)?;
                        prn.comma()?;
                    }
                    Ok(())
                })
            }
            V::Array(items) => {
                prn.array(|prn| {
                    if let Some(limit) = prn.max_items() {
                        for item in &items[..min(limit, items.len())] {
                            item.format(prn)?;
                            prn.comma()?;
                        }
                        if items.len() > limit {
                            prn.ellipsis()?;
                        }
                    } else {
                        for item in items {
                            item.format(prn)?;
                            prn.comma()?;
                        }
                    }
                    Ok(())
                })
            }
            V::Enum(v) => prn.const_scalar(&**v),
        }
    }
}

impl FormatExt for Option<Value> {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error> {
        match self {
            Some(v) => v.format(prn),
            None => prn.nil(),
        }
    }
}

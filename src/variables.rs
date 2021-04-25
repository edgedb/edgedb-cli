use std::error::Error;
use std::fmt;

use anyhow::Context;

use crate::prompt;
use crate::repl;
use edgedb_protocol::codec;
use edgedb_protocol::descriptors::{Descriptor, InputTypedesc};
use edgedb_protocol::value::Value;

#[derive(Debug)]
pub struct Canceled;

pub async fn input_variables(
    desc: &InputTypedesc,
    state: &mut repl::PromptRpc,
) -> Result<Value, anyhow::Error> {
    if desc.is_empty_tuple() {
        return Ok(Value::Tuple(Vec::new()));
    }
    match desc.root() {
        Descriptor::Tuple(tuple) => {
            let mut val = Vec::with_capacity(tuple.element_types.len());
            for (idx, el) in tuple.element_types.iter().enumerate() {
                val.push(input_item(&format!("{}", idx), desc.get(*el)?, desc, state).await?);
            }
            return Ok(Value::Tuple(val));
        }
        Descriptor::NamedTuple(tuple) => {
            let mut fields = Vec::with_capacity(tuple.elements.len());
            let shape = tuple.elements[..].into();
            for el in tuple.elements.iter() {
                fields.push(input_item(&el.name, desc.get(el.type_pos)?, desc, state).await?);
            }
            return Ok(Value::NamedTuple { shape, fields });
        }
        root => {
            return Err(anyhow::anyhow!("Unknown input type descriptor: {:?}", root));
        }
    }
}

async fn input_item(
    name: &str,
    mut item: &Descriptor,
    all: &InputTypedesc,
    state: &mut repl::PromptRpc,
) -> Result<Value, anyhow::Error> {
    match item {
        Descriptor::Scalar(s) => {
            item = all.get(s.base_type_pos)?;
        }
        _ => {}
    }
    match item {
        Descriptor::BaseScalar(s) => {
            let type_name = match s.id {
                codec::STD_STR => "str",
                codec::STD_UUID => "uuid",
                codec::STD_INT16 => "int16",
                codec::STD_INT32 => "int32",
                codec::STD_INT64 => "int64",
                _ => return Err(anyhow::anyhow!("Unimplemented input type {}", s.id)),
            };

            let val = match state.variable_input(name, type_name, "").await? {
                prompt::Input::Text(val) => val,
                prompt::Input::Interrupt | prompt::Input::Eof => Err(Canceled)?,
            };

            match s.id {
                codec::STD_STR => Ok(Value::Str(val)),
                codec::STD_UUID => {
                    let v = val.parse().context("invalid uuid value")?;
                    Ok(Value::Uuid(v))
                }
                codec::STD_INT16 => {
                    let v = val.parse::<i16>().context("invalid int16 value")?;
                    Ok(Value::Int16(v))
                }
                codec::STD_INT32 => {
                    let v = val.parse::<i32>().context("invalid int32 value")?;
                    Ok(Value::Int32(v))
                }
                codec::STD_INT64 => {
                    let v = val.parse::<i64>().context("invalid int64 value")?;
                    Ok(Value::Int64(v))
                }
                _ => Err(anyhow::anyhow!("Unimplemented input type {}", s.id)),
            }
        }
        _ => Err(anyhow::anyhow!(
            "Unimplemented input type descriptor: {:?}",
            item
        )),
    }
}

impl Error for Canceled {}

impl fmt::Display for Canceled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        "Operation canceled".fmt(f)
    }
}

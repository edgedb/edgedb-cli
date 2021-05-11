use std::fmt;
use std::error::Error;
use std::sync::Arc;

use edgedb_protocol::value::Value;
use edgedb_protocol::codec;
use edgedb_protocol::descriptors::{InputTypedesc, Descriptor};
use crate::repl;
use crate::prompt;
use crate::prompt::variable::{self, VariableInput};


#[derive(Debug)]
pub struct Canceled;


pub async fn input_variables(desc: &InputTypedesc, state: &mut repl::PromptRpc)
    -> Result<Value, anyhow::Error>
{
    if desc.is_empty_tuple() {
        return Ok(Value::Tuple(Vec::new()));
    }
    match desc.root() {
        Descriptor::Tuple(tuple) => {
            let mut val = Vec::with_capacity(tuple.element_types.len());
            for (idx, el) in tuple.element_types.iter().enumerate() {
                val.push(input_item(&format!("{}", idx),
                    desc.get(*el)?, desc, state).await?);
            }
            return Ok(Value::Tuple(val));
        }
        Descriptor::NamedTuple(tuple) => {
            let mut fields = Vec::with_capacity(tuple.elements.len());
            let shape = tuple.elements[..].into();
            for el in tuple.elements.iter() {
                fields.push(input_item(&el.name,
                    desc.get(el.type_pos)?, desc, state).await?);
            }
            return Ok(Value::NamedTuple { shape, fields });
        }
        root => {
            return Err(anyhow::anyhow!(
                "Unknown input type descriptor: {:?}", root));
        }
    }
}

async fn input_item(name: &str, mut item: &Descriptor, all: &InputTypedesc,
    state: &mut repl::PromptRpc)
    -> Result<Value, anyhow::Error>
{
    match item {
        Descriptor::Scalar(s) => {
            item = all.get(s.base_type_pos)?;
        }
        _ => {},
    }
    match item {
        Descriptor::BaseScalar(s) => {
            let var_type: Arc<dyn VariableInput> = match s.id {
                codec::STD_STR => Arc::new(variable::Str),
                codec::STD_UUID => Arc::new(variable::Uuid),
                codec::STD_INT16 => Arc::new(variable::Int16),
                codec::STD_INT32 => Arc::new(variable::Int32),
                codec::STD_INT64 => Arc::new(variable::Int64),
                _ => return Err(anyhow::anyhow!(
                        "Unimplemented input type {}", s.id))
            };

            let val = match state.variable_input(name, var_type, "").await? {
                | prompt::Input::Value(val) => val,
                | prompt::Input::Text(_) => unreachable!(),
                | prompt::Input::Interrupt
                | prompt::Input::Eof => Err(Canceled)?,
            };
            Ok(val)
        }
        _ => Err(anyhow::anyhow!(
                "Unimplemented input type descriptor: {:?}", item)),
    }
}

impl Error for Canceled {
}

impl fmt::Display for Canceled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        "Operation canceled".fmt(f)
    }
}

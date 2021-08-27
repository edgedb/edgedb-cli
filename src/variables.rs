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
        Descriptor::Tuple(tuple) if desc.proto().is_at_most(0, 11) => {
            let mut val = Vec::with_capacity(tuple.element_types.len());
            for (idx, el) in tuple.element_types.iter().enumerate() {
                val.push(input_item(
                    &format!("{}", idx), desc.get(*el)?, desc, state, false,
                ).await?.expect("no optional"));
            }
            return Ok(Value::Tuple(val));
        }
        Descriptor::NamedTuple(tuple) if desc.proto().is_at_most(0, 11) => {
            let mut fields = Vec::with_capacity(tuple.elements.len());
            let shape = tuple.elements[..].into();
            for el in tuple.elements.iter() {
                fields.push(input_item(
                    &el.name, desc.get(el.type_pos)?, desc, state, false
                ).await?.expect("no optional"));
            }
            return Ok(Value::NamedTuple { shape, fields });
        }
        Descriptor::ObjectShape(obj) if desc.proto().is_at_least(0, 12) => {
            let mut fields = Vec::with_capacity(obj.elements.len());
            let shape = obj.elements[..].into();
            for el in obj.elements.iter() {
                let optional = el.cardinality
                    .map(|c| c.is_optional()).unwrap_or(false);
                fields.push(input_item(
                    &el.name, desc.get(el.type_pos)?, desc, state, optional,
                ).await?);
            }
            return Ok(Value::Object { shape, fields });
        }
        root => {
            return Err(anyhow::anyhow!(
                "Unknown input type descriptor: {:?}", root));
        }
    }
}

async fn input_item(name: &str, mut item: &Descriptor, all: &InputTypedesc,
    state: &mut repl::PromptRpc, optional: bool)
    -> Result<Option<Value>, anyhow::Error>
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
                codec::STD_FLOAT32 => Arc::new(variable::Float32),
                codec::STD_FLOAT64 => Arc::new(variable::Float64),
                codec::STD_DECIMAL => Arc::new(variable::Decimal),
                codec::STD_BOOL => Arc::new(variable::Bool),
                codec::STD_JSON => Arc::new(variable::Json),
                codec::STD_BIGINT => Arc::new(variable::BigInt),
                _ => return Err(anyhow::anyhow!(
                        "Unimplemented input type {}", s.id))
            };

            let val = match
                state.variable_input(name, var_type, optional, "").await?
            {
                | prompt::Input::Value(val) => Some(val),
                | prompt::Input::Text(_) => unreachable!(),
                | prompt::Input::Interrupt => Err(Canceled)?,
                | prompt::Input::Eof => None,
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

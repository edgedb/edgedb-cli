use edgedb_protocol::value::Value::{self, *};


pub fn format_row(v: &Value) -> Result<String, anyhow::Error> {
    match v {
        Object { shape, fields } => {
            Ok(shape.elements.iter().zip(fields)
                .filter(|(s, _)| !s.flag_implicit)
                .map(|(_, v)| match v {
                    Some(v) => value_to_string(v),
                    None => Ok(String::new()),
                })
                .collect::<Result<Vec<_>,_>>()?.join("\t"))
        }
        _ => value_to_string(v),
    }
}

fn value_to_string(v: &Value) -> Result<String, anyhow::Error> {
    use edgedb_protocol::value::Value::*;
    match v {
        Nothing => Ok(String::new()),
        Uuid(uuid) => Ok(uuid.to_string()),
        Str(s) => Ok(s.clone()),
        Int16(v) => Ok(v.to_string()),
        Int32(v) => Ok(v.to_string()),
        Int64(v) => Ok(v.to_string()),
        Float32(v) => Ok(v.to_string()),
        Float64(v) => Ok(v.to_string()),
        Bool(v) => Ok(v.to_string()),
        Json(v) => Ok(v.to_string()),
        Enum(v) => Ok(v.to_string()),
        Duration(v) => Ok(v.to_string()),
        ConfigMemory(v) => Ok(v.to_string()),
        RelativeDuration(v) => Ok(v.to_string()),
        | Datetime(_) // TODO(tailhook)
        | BigInt(_) // TODO(tailhook)
        | Decimal(_) // TODO(tailhook)
        | LocalDatetime(_) // TODO(tailhook)
        | LocalDate(_) // TODO(tailhook)
        | LocalTime(_) // TODO(tailhook)
        | Bytes(_)
        | Object {..}
        | NamedTuple {..}
        | Array(_)
        | Set(_)
        | Tuple(_)
        => {
            Err(anyhow::anyhow!(
                "Complex objects like {:?} cannot be printed tab-separated",
                v))
        }
    }
}

use serde_json::Value;

use crate::print::buffer::Result;
use crate::print::{FormatExt, Formatter};

impl FormatExt for Value {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error> {
        use Value as V;
        match self {
            V::Null => prn.const_bool("null"),
            V::Bool(v) => prn.const_bool(v),
            s @ V::String(_) => prn.const_string(s),
            s @ V::Number(_) => prn.const_number(s),
            V::Array(items) => prn.array(None, |prn| {
                for item in items {
                    item.format(prn)?;
                    prn.comma()?;
                }
                Ok(())
            }),
            V::Object(dict) => prn.json_object(|prn| {
                for (key, value) in dict {
                    let json_str = serde_json::to_string(key).expect("cannot serialize string");
                    prn.object_field(json_str.as_str(), false)?;

                    value.format(prn)?;
                    prn.comma()?;
                }
                Ok(())
            }),
        }
    }
}

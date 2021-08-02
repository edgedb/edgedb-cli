use serde_json::Value;

use crate::print::{FormatExt, Formatter};
use crate::print::buffer::Result;


impl FormatExt for Value {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error> {
        use Value as V;
        match self {
            V::Null => prn.const_bool("null"),
            V::Bool(v) => prn.const_bool(v),
            s@V::String(_) => prn.const_string(s),
            s@V::Number(_) => prn.const_number(s),
            V::Array(items) => {
                prn.array(|prn| {
                    for item in items {
                        item.format(prn)?;
                        prn.comma()?;
                    }
                    Ok(())
                })
            },
            V::Object(dict) => {
                prn.json_object(|prn| {
                    for (key, value) in dict {
                        if key.starts_with('@') {
                            prn.object_field(
                                serde_json::to_string(key)
                                .expect("cannot serialize string")
                                .as_ref(),
                            false)?;
                        } else {
                            prn.object_field(
                                serde_json::to_string(key)
                                .expect("cannot serialize string")
                                .as_ref(),
                                false)?;
                        }
                        value.format(prn)?;
                        prn.comma()?;
                    }
                    Ok(())
                })
            }
        }
    }
}

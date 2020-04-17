use serde_json::Value;

use crate::print::{FormatExt, Formatter};
use crate::print::buffer::Result;


impl FormatExt for Value {
    fn format<F: Formatter>(&self, prn: &mut F) -> Result<F::Error> {
        use Value as V;
        match self {
            V::Null => prn.const_scalar("null"),
            V::Bool(v) => prn.const_scalar(v),
            s@V::String(_)|s@V::Number(_) => prn.const_scalar(s),
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
                        prn.object_field(&serde_json::to_string(key)
                                         .expect("can serialize string"))?;
                        value.format(prn)?;
                        prn.comma()?;
                    }
                    Ok(())
                })
            }
        }
    }
}

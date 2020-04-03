use std::borrow::Cow;
use bytes::Bytes;

pub use edgeql_parser::helpers::quote_name;


pub fn print_result(res: Bytes) {
    eprintln!("  -> {}: Ok", String::from_utf8_lossy(&res[..]));
}


pub fn quote_namespaced<'x>(name: &'x str) -> Cow<'x, str> {
    if name.contains("::") {
        let mut buf = String::with_capacity(name.len());
        let mut iter = name.split("::");
        buf.push_str(&quote_name(iter.next().unwrap()));
        for chunk in iter {
            buf.push_str("::");
            buf.push_str(&quote_name(chunk));
        }
        return buf.into();
    } else {
        return quote_name(name);
    }
}

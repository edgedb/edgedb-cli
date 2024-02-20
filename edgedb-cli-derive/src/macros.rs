macro_rules! format_err {
    ($obj:expr, $($format:tt)+) => {{
        let msg = format!($($format)+);
        syn::Error::new($obj, msg)
    }};
}

macro_rules! abort {
    ($obj:expr, $($format:tt)+) => {{
        return Err(format_err!($obj, $($format)+));
    }};
}

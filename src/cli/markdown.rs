#[macro_export]
macro_rules! _md_set_items {
    ($expander:expr, $key: ident = $value: expr $(, $($tail:tt)*)?) => {
        $expander.set(stringify!($key), $value);
        $crate::_md_set_items!($expander $(, $($tail)*)?);
    };
    ($expander:expr, $key: ident : if $condition:expr => {
            $($subkey: ident = $subvalue: expr),* $(,)?
    } $(, $($tail:tt)*)?) => {
        if $condition {
            let exp = $expander.sub(stringify!($key));
            $(exp.set(stringify!($subkey), $subvalue);)*
        }
        $crate::_md_set_items!($expander $(, $($tail)*)?);
    };
    ($expander:expr, $key: ident : if $condition:expr $(, $($tail:tt)*)?) => {
        if $condition {
            $expander.sub(stringify!($key));
        }
        $crate::_md_set_items!($expander $(, $($tail)*)?);
    };
    ($expander:expr $(,)*) => {};
}

#[macro_export]
macro_rules! print_markdown {
    ($template: expr$(, $($item:tt)*)?) => {
        #[allow(unused_mut)]
        {
            static TEMPLATE: once_cell::sync::OnceCell<minimad::TextTemplate> =
                once_cell::sync::OnceCell::new();
            let template = TEMPLATE.get_or_init(move || {
                minimad::TextTemplate::from($template)
            });
            let mut expander = minimad::OwningTemplateExpander::new();
            $crate::_md_set_items!(expander $(, $($item)*)?);
            let skin = termimad::get_default_skin();
            let (width, _) = termimad::terminal_size();
            let fmt = termimad::FmtText::from_text(
                skin,
                expander.expand(&template),
                Some(width as usize),
            );
            print!("{}", fmt);
        }
    }
}

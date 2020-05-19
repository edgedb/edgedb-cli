use once_cell::sync::Lazy;
use prettytable::format::{FormatBuilder, LinePosition, LineSeparator};
use prettytable::format::{Alignment, TableFormat};
use prettytable::{Cell, Attr};


pub static FORMAT: Lazy<TableFormat> = Lazy::new(|| {
    FormatBuilder::new()
    .column_separator('│')
    .borders('│')
    .separators(&[LinePosition::Top],
                LineSeparator::new('─',
                                   '┬',
                                   '┌',
                                   '┐'))
    .separators(&[LinePosition::Title],
                LineSeparator::new('─',
                                   '┼',
                                   '├',
                                   '┤'))
    .separators(&[LinePosition::Bottom],
                LineSeparator::new('─',
                                   '┴',
                                   '└',
                                   '┘'))
    .padding(1, 1)
    .build()
});

pub fn header_cell(title: &str) -> Cell {
    Cell::new_align(title, Alignment::CENTER)
        .with_style(Attr::Dim)
}

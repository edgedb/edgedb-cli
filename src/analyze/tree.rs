use std::fmt::{self, Write};

use crate::analyze::contexts;
use crate::analyze::model::{Analysis, Plan, Stage, Arguments};
use crate::analyze::model::{Shape, ChildName, DebugNode, Cost};
use crate::analyze::table;
use crate::print::Highlight;



struct Opt<'a, T>(&'a Option<T>);
struct Border;

#[derive(Clone, Copy, Debug)]
struct Wide;

#[derive(Clone, Copy, Debug)]
struct Narrow;

#[derive(Debug, Clone)]
pub struct WideMarker {
    columns: bitvec::vec::BitVec,
}

#[derive(Debug, Clone)]
pub struct NarrowMarker {
    columns: bitvec::vec::BitVec,
    has_children: bool,
    has_head: bool,
}

#[derive(Debug, Clone)]
pub struct ShapeNode<'a> {
    marker: WideMarker,
    context: contexts::OptNumber,
    attribute: Option<&'a str>,
}

#[derive(Debug)]
pub struct Relations<'a>(&'a [String]);
pub struct NodeTitle<'a>(contexts::OptNumber, &'a str);
pub struct Property<'a, T: fmt::Display>(&'a str, T);
pub struct Comma<T>(T, bool);
pub struct CommaSeparated<T: Iterator>(std::iter::Peekable<T>);

#[derive(Debug)]
pub struct StageInfo<'a> {
    context: contexts::OptNumber,
    stage: &'a Stage,
}

#[derive(Debug)]
pub struct DebugPlanInfo<'a> {
    context: contexts::OptNumber,
    node: &'a DebugNode,
}

pub trait HasChildren {
    fn has_children(&self) -> bool;
}

pub fn print_debug_plan(explain: &Analysis) {
    if let Some(debug) = &explain.debug_info {
        if let Some(node) = &debug.full_plan {

            let mut header = Vec::with_capacity(9);
            header.push(Box::new("") as Box<_>);
            cost_header(&mut header, &explain.arguments);
            header.push(Box::new("Plan Info".emphasize()) as Box<_>);

            let marker = NarrowMarker::new(node);

            let mut row = Vec::with_capacity(9);
            row.push(Box::new(marker.clone()) as Box<_>);
            cost_columns(&mut row, &node.cost, &explain.arguments);
            row.push(Box::new(table::WordList(DebugPlanInfo {
                context: explain.context(&node.contexts),
                node,
            })));

            let mut rows = vec![header, row];

            for (child, ch) in marker.children(&node.plans) {
                visit_debug_tree(&mut rows, &explain, child, ch);
            }

            table::render(Some("Debug Plan"), &rows);
        }
    }
}

pub fn print_shape(explain: &Analysis) {
    if let Some(shape) = &explain.coarse_grained {
        let mut header = Vec::with_capacity(8);
        header.push(Box::new("") as Box<_>);
        cost_header(&mut header, &explain.arguments);
        header.push(Box::new("Relations".emphasize()) as Box<_>);

        let mut root = Vec::with_capacity(3);
        let context = explain.context(&shape.contexts);
        root.push(Box::new(format!("{context}{}", "root".fade())) as Box<_>);
        cost_columns(&mut root, &shape.cost, &explain.arguments);
        root.push(Box::new(table::WordList(Relations(&shape.relations))));

        let mut rows = vec![header, root];

        for (child, ch) in WideMarker::new().children(&shape.children) {
            match &ch.name {
                ChildName::Pointer { name } => {
                    visit_subshape(&mut rows, &explain,
                                   child, Some(name), &ch.node);
                }
                _ => {
                    visit_subshape(&mut rows, &explain,
                                   child, None, &ch.node);
                }
            }
        }

        table::render(Some("Coarse-grained Query Plan"), &rows);
    }
}

fn visit_subshape<'x>(
    result: &mut Vec<Vec<Box<dyn table::Contents + 'x>>>,
    explain: &Analysis,
    marker: WideMarker,
    attribute: Option<&'x str>,
    node: &'x Shape,
) {
    let mut row = Vec::with_capacity(8);
    row.push(Box::new(ShapeNode {
        marker: marker.clone(),
        context: explain.context(&node.contexts),
        attribute,
    }) as Box<_>);
    cost_columns(&mut row, &node.cost, &explain.arguments);
    row.push(Box::new(table::WordList(Relations(&node.relations))));
    result.push(row);

    for (child, ch) in marker.children(&node.children) {
        match &ch.name {
            ChildName::Pointer { name } => {
                visit_subshape(result, explain, child, Some(name), &ch.node);
            }
            _ => {
                visit_subshape(result, explain, child, None, &ch.node);
            }
        }
    }
}


pub fn print_expanded_tree(explain: &Analysis) {
    if let Some(node) = &explain.fine_grained {
        let mut header = Vec::with_capacity(9);
        header.push(Box::new("") as Box<_>);
        cost_header(&mut header, &explain.arguments);
        header.push(Box::new("Plan Info".emphasize()) as Box<_>);

        let marker = NarrowMarker::new(node);
        let mut rows = vec![header];

        for (i, (m, item)) in marker.subsequent(&node.pipeline).enumerate() {
            let mut row = Vec::with_capacity(9);
            row.push(Box::new(m) as Box<_>);
            cost_columns(&mut row, &item.cost, &explain.arguments);
            row.push(Box::new(table::WordList(StageInfo {
                context: if i == 0 {
                    explain.context(&node.contexts)
                } else {
                    explain.context(&[])
                },
                stage: item,
            })));
            rows.push(row);
        }

        for (child, ch) in marker.children(&node.subplans) {
            visit_expanded_tree(&mut rows, &explain, child, ch);
        }

        table::render(Some("Fine-grained Query Plan"), &rows);
    }
}

fn visit_expanded_tree<'x>(
    result: &mut Vec<Vec<Box<dyn table::Contents + 'x>>>,
    explain: &Analysis,
    marker: NarrowMarker,
    node: &'x Plan,
) {
    for (i, (marker, item)) in marker.subsequent(&node.pipeline).enumerate() {
        let mut row = Vec::with_capacity(9);
        row.push(Box::new(marker) as Box<_>);
        cost_columns(&mut row, &item.cost, &explain.arguments);
        row.push(Box::new(table::WordList(StageInfo {
            context: if i == 0 {
                explain.context(&node.contexts)
            } else {
                explain.context(&[])
            },
            stage: item,
        })));
        result.push(row);
    }

    for (child, ch) in marker.children(&node.subplans) {
        visit_expanded_tree(result, &explain, child, ch);
    }
}

fn visit_debug_tree<'x>(
    result: &mut Vec<Vec<Box<dyn table::Contents + 'x>>>,
    explain: &Analysis,
    marker: NarrowMarker,
    node: &'x DebugNode,
) {
    let mut row = Vec::with_capacity(9);
    row.push(Box::new(marker.clone()) as Box<_>);
    cost_columns(&mut row, &node.cost, &explain.arguments);
    row.push(Box::new(table::WordList(DebugPlanInfo {
        context: explain.context(&node.contexts),
        node,
    })));
    result.push(row);

    for (child, ch) in marker.children(&node.plans) {
        visit_debug_tree(result, &explain, child, ch);
    }
}

impl<'a, T: fmt::Display> fmt::Display for Opt<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => write!(f, "∅"),
            Some(v) => v.fmt(f),
        }
    }
}

impl WideMarker {
    pub fn new() -> WideMarker {
        WideMarker {
            columns: bitvec::vec::BitVec::new(),
        }
    }
    pub fn children<'x, I: IntoIterator>(&'x self, children: I)
        -> impl Iterator<Item=(WideMarker, I::Item)> + 'x
        where I::IntoIter: ExactSizeIterator + 'x,
    {
        let children = children.into_iter();
        let last_idx = children.len().saturating_sub(1);
        children.enumerate().map(move |(idx, child)| {
            let mut columns = self.columns.clone();
            columns.push(last_idx == idx);
            (WideMarker { columns }, child)
        })
    }
}

impl<T: HasChildren> HasChildren for &T {
    fn has_children(&self) -> bool {
        (*self).has_children()
    }
}

impl HasChildren for Plan {
    fn has_children(&self) -> bool {
        return !self.subplans.is_empty()
    }
}

impl HasChildren for DebugNode {
    fn has_children(&self) -> bool {
        return !self.plans.is_empty()
    }
}

impl NarrowMarker {
    pub fn new<T: HasChildren>(item: &T) -> NarrowMarker {
        NarrowMarker {
            columns: bitvec::vec::BitVec::new(),
            has_children: item.has_children(),
            has_head: true,
        }
    }
    pub fn children<'x, I: IntoIterator>(&'x self, children: I)
        -> impl Iterator<Item=(NarrowMarker, I::Item)> + 'x
        where I::IntoIter: ExactSizeIterator + 'x,
              <I::IntoIter as Iterator>::Item: HasChildren,
    {
        let children = children.into_iter();
        let last_idx = children.len().saturating_sub(1);
        children.enumerate().map(move |(idx, child)| {
            let mut columns = self.columns.clone();
            columns.push(last_idx == idx);
            let marker = NarrowMarker {
                columns,
                has_children: child.has_children(),
                has_head: true,
            };
            (marker, child)
        })
    }
    pub fn subsequent<'x, I: IntoIterator>(&'x self, items: I)
        -> impl Iterator<Item=(NarrowMarker, I::Item)> + 'x
        where I::IntoIter: ExactSizeIterator + 'x,
    {
        let items = items.into_iter();
        items.enumerate().map(move |(idx, child)| {
            let marker = NarrowMarker {
                columns: self.columns.clone(),
                has_children: self.has_children,
                has_head: idx == 0,
            };
            (marker, child)
        })
    }
}

impl WideMarker {
    fn width(&self) -> usize {
        self.columns.len() * 3
    }
    fn render_head(&self, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        let cols = &self.columns;
        if cols.len() <= 1 {
            if let Some(last) = cols.last() {
                if *last {
                    write!(f, "╰──")?;
                } else {
                    write!(f, "├──")?;
                }
            }
        } else {
            let mut iter = cols.iter().take(self.columns.len() - 1);
            if let Some(first) = iter.next() {
                if *first {
                    write!(f, "   ")?;
                } else {
                    write!(f, "│  ")?;
                }
            }
            for i in iter {
                if *i {
                    write!(f, "   ")?;
                } else {
                    write!(f, "│  ")?;
                }
            }
            if let Some(last) = cols.last() {
                if *last {
                    write!(f, "╰──")?;
                } else {
                    write!(f, "├──")?;
                }
            }
        }
        Ok(())
    }
    fn render_tail(&self, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        for _ in 1..height {
            write!(f, "\n")?;
            let mut iter = self.columns.iter();
            if let Some(first) = iter.next() {
                if *first {
                    write!(f, "   ")?;
                } else {
                    write!(f, "│  ")?;
                }
            }
            for i in iter {
                if *i {
                    write!(f, "   ")?;
                } else {
                    write!(f, "│  ")?;
                }
            }
        }
        Ok(())
    }
}

impl table::Contents for NarrowMarker {
    fn width_bounds(&self) -> (usize, usize) {
        (self.columns.len(), self.columns.len())
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, width: usize, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        let cols = &self.columns;
        if self.has_head {
            for i in cols.iter().take(self.columns.len().saturating_sub(1)) {
                f.write_char(if *i { ' ' } else { '│' })?;
            }
            if let Some(last) = cols.last() {
                f.write_char(if *last { '╰' } else { '├' })?;
            }
            let mut fill = cols.len()..width;
            if fill.next().is_some() {
                f.write_char(if self.has_children { '┬' } else { '─' })?;
            }
            for _ in fill {
                f.write_char('─')?;
            }
            f.write_char('\n')?;
        }
        for _ in (if self.has_head { 1 } else { 0 })..height {
            for i in &self.columns {
                f.write_char(if *i { ' ' } else { '│' })?;
            }
            if width > cols.len() && self.has_children {
                f.write_char('│')?;
            }
            f.write_char('\n')?;
        }
        Ok(())
    }
}

impl table::Contents for ShapeNode<'_> {
    fn width_bounds(&self) -> (usize, usize) {
        let mwidth = self.marker.width() + table::display_width(&self.context);
        let alen = if let Some(attr) = self.attribute {
            " .".len() + attr.len()
        } else {
            0
        };
        (mwidth + alen, mwidth + alen)
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, _width: usize, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        self.marker.render_head(f)?;
        write!(f, "{}", self.context)?;
        if let Some(attr) = self.attribute {
            write!(f, ".{}", attr)?;
        }
        self.marker.render_tail(height, f)?;
        Ok(())
    }
}

impl table::Contents for Border {
    fn width_bounds(&self) -> (usize, usize) {
        (1, 1)
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, _width: usize, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        for _ in 0..height {
            write!(f, "{}\n", "│".emphasize())?;
        }
        Ok(())
    }
}

fn cost_header(
    header: &mut Vec<Box<dyn table::Contents + '_>>,
    args: &Arguments
) {
    header.push(Box::new(Border) as Box<_>);
    if args.execute {
        header.push(Box::new(table::Right("Time".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Cost".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Loops".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Rows".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Width".emphasize())) as Box<_>);
    } else {
        header.push(Box::new(table::Right("Cost".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Plan Rows".emphasize())) as Box<_>);
        header.push(Box::new(table::Right("Width".emphasize())) as Box<_>);
    }
    header.push(Box::new(Border) as Box<_>);
}

fn cost_columns(
    row: &mut Vec<Box<dyn table::Contents + '_>>,
    cost: &Cost,
    args: &Arguments
) {
    row.push(Box::new(Border) as Box<_>);
    if args.execute {
        // TODO(tailhook) use actual time
        row.push(Box::new(table::Float(cost.actual_total_time.unwrap_or(0.))));
        row.push(Box::new(table::Right(cost.total_cost)));
        row.push(Box::new(table::Float(cost.actual_loops.unwrap_or(0.))));
        row.push(Box::new(table::Float(cost.actual_rows.unwrap_or(0.))));
        row.push(Box::new(table::Right(cost.plan_width)));
    } else {
        row.push(Box::new(table::Float(cost.total_cost)));
        row.push(Box::new(table::Right(cost.plan_rows)));
        row.push(Box::new(table::Right(cost.plan_width)));
    }
    row.push(Box::new(Border) as Box<_>);
}

impl table::Words for Relations<'_> {
    fn print<T: table::Printer>(&self, p: &mut T) -> fmt::Result {
        p.words(add_commas(self.0))
    }
}

impl table::Words for StageInfo<'_> {
    fn print<T: table::Printer>(&self, p: &mut T) -> fmt::Result {
        p.word(NodeTitle(self.context, &self.stage.plan_type))?;
        let props = self.stage.properties.iter()
            .filter(|i| i.important)
            .map(|p| Property(&p.title, &p.value));
        p.words(add_commas(props))?;
        Ok(())
    }
}

impl table::Words for DebugPlanInfo<'_> {
    fn print<T: table::Printer>(&self, p: &mut T) -> fmt::Result {
        p.word(NodeTitle(self.context, &self.node.node_type))?;
        if let Some(value) = &self.node.parent_relationship {
            p.word(Property("parent_rel", value))?;
        }
        if let Some(value) = &self.node.alias {
            p.word(Property("alias", value))?;
        }
        if let Some(value) = &self.node.index_name {
            p.word(Property("index_name", value))?;
        }
        if let Some(value) = &self.node.relation_name {
            p.word(Property("relation_name", value))?;
        }
        p.words(self.node.properties.iter().map(|(k, v)| Property(k, v)))?;
        Ok(())
    }
}

impl<T: Iterator> Iterator for CommaSeparated<T> {
    type Item = Comma<T::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|element| {
            Comma(element, self.0.peek().is_some())
        })
    }
}

fn add_commas<T>(input: impl IntoIterator<Item=T>)
    -> impl Iterator<Item=Comma<T>>
{
    CommaSeparated(input.into_iter().peekable())
}

impl<T: fmt::Display> fmt::Display for Comma<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Comma(contents, comma) = self;
        contents.fmt(f)?;
        if *comma {
            f.write_char(',')?;
        }
        Ok(())
    }
}

impl fmt::Display for NodeTitle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.0, self.1)
    }
}

impl<T: fmt::Display> fmt::Display for Property<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}={}", self.0.fade(), self.1)
    }
}

use std::cmp::max;
use std::fmt;

use crate::analyze::contexts;
use crate::analyze::model::{Analysis, Plan, Arguments};
use crate::analyze::model::{Shape, ChildName, DebugNode, Cost};
use crate::analyze::table;
use crate::print::Highlight;



struct Opt<'a, T>(&'a Option<T>);
struct Border;

#[derive(Debug, Clone)]
pub struct NodeMarker {
    columns: bitvec::vec::BitVec,
}

#[derive(Debug, Clone)]
pub struct ShapeNode<'a> {
    marker: NodeMarker,
    context: contexts::OptNumber,
    attribute: Option<&'a str>,
}

#[derive(Debug)]
pub struct Relations<'a>(&'a [String]);

pub fn print_debug_plan(explain: &Analysis) {
    if let Some(debug) = &explain.debug_info {
        if let Some(node) = &debug.full_plan {
            println!("Debug Plan");
            print_debug_node(explain, "", node, true);
        }
    }
}

fn print_debug_node(explain: &Analysis, prefix: &str, node: &DebugNode,
                    last: bool)
{
    let marker = if last { "└──" } else { "├──" };
    println!("{prefix}{marker}{shape}{ctx} {node_type}:{parent} {alias}/{rel_name} [{simplified_path}] / {index} \
             (cost={cost1}..{cost2} actual_time={atime} rows={rows} width={width})",
             node_type=node.node_type,
             parent=node.parent_relationship.as_deref().unwrap_or("root"),
             index=node.index_name.as_deref().unwrap_or("-"),
             rows=node.plan_rows,
             width=node.plan_width,
             cost1=node.startup_cost,
             cost2=node.total_cost,
             atime=Opt(&node.actual_total_time),
             alias=node.alias.as_deref().unwrap_or("-"),
             rel_name=node.relation_name.as_deref().unwrap_or("-"),
             simplified_path=node.simplified_paths.iter()
                 .map(|(alias, attr)| format!("{alias}.{attr}"))
                 .collect::<Vec<_>>().join(","),
             shape=node.is_shape.then(|| "+").unwrap_or(""),
             ctx=explain.context(&node.contexts),
    );
    let prefix = prefix.to_string() + if last { "    " } else { "│   " };
    let last_idx = node.plans.len().saturating_sub(1);
    for (idx, node) in node.plans.iter().enumerate() {
        print_debug_node(explain, &prefix, node, idx == last_idx);
    }
}

pub fn print_shape(explain: &Analysis) {
    if let Some(shape) = &explain.coarse_grained {
        let mut header = Vec::with_capacity(3);
        header.push(Box::new("") as Box<_>);
        cost_header(&mut header, &explain.arguments);
        header.push(Box::new("Relations".emphasize()) as Box<_>);

        let mut root = Vec::with_capacity(3);
        let context = explain.context(&shape.contexts);
        root.push(Box::new(format!("{context} {}", "root".fade())) as Box<_>);
        cost_columns(&mut root, &shape.cost, &explain.arguments);
        root.push(Box::new(Relations(&shape.relations)));

        let mut rows = vec![header, root];

        for (child, ch) in NodeMarker::new().children(&shape.children) {
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
    marker: NodeMarker,
    attribute: Option<&'x str>,
    node: &'x Shape,
) {
    let mut row = Vec::with_capacity(3);
    row.push(Box::new(ShapeNode {
        marker: marker.clone(),
        context: explain.context(&node.contexts),
        attribute,
    }) as Box<_>);
    cost_columns(&mut row, &node.cost, &explain.arguments);
    row.push(Box::new(Relations(&node.relations)));
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


pub fn print_tree(explain: &Analysis) {
    println!("Full Plan Tree");
    if let Some(tree) = &explain.fine_grained {
        print_tree_node(explain, "", tree, true);
    }
}

fn print_tree_node(explain: &Analysis, prefix: &str, node: &Plan, last: bool) {
    let children = !node.subplans.is_empty();
    let pipe = node.pipeline.len() > 1;
    let (m1, m2, m3, m4) = match (last, children, pipe) {
        (true, true, false) =>   ("╰┬───", "     ", "     ", " │   "), //
        (false, true, false) =>  ("├┬───", "     ", "     ", "││   "), //
        (true, false, false) =>  ("╰────", "     ", "     ", "     "), //
        (false, false, false) => ("├────", "     ", "     ", "│    "),

        (true, true, true) =>    ("╰┬┄┄┄", " ┊   ", " ├───", " │   "), //
        (false, true, true) =>   ("├┬┄┄┄", "│┊   ", "│├───", "││   "), //
        (true, false, true) =>   ("╰┬┄┄┄", " ┊   ", " ╰───", " │   "),
        (false, false, true) =>  ("├┬┄┄┄", "│┊   ", "│╰───", "│    "), //
    };
    let last_idx = node.pipeline.len().saturating_sub(1);
    for (idx, stage) in node.pipeline.iter().enumerate() {
        let pipelast = idx == last_idx;
        println!("{prefix}{marker}{ctx} {type} (cost={cost1}..{cost2} rows={rows} width={width})",
                 marker=if idx == 0 { m1 } else if pipelast { m3 } else { m2 },
                 type=stage.plan_type,
                 rows=stage.cost.plan_rows,
                 width=stage.cost.plan_width,
                 cost1=stage.cost.startup_cost,
                 cost2=stage.cost.total_cost,
                 ctx=if pipelast {
                     explain.context(&node.contexts)
                 } else {
                     explain.context(&[])
                 },

        );
        for prop in &stage.properties {
            if prop.important {
                println!("{prefix}{marker}    {name}={value}",
                         marker=if pipelast { m4 } else { m2 },
                         name=prop.title,
                         value=prop.value,
                );
            }
        }
    }
    let prefix = prefix.to_string() + if last { " " } else { "│" };
    let last_idx = node.subplans.len().saturating_sub(1);
    for (idx, node) in node.subplans.iter().enumerate() {
        print_tree_node(explain, &prefix, node, idx == last_idx);
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

impl NodeMarker {
    pub fn new() -> NodeMarker {
        NodeMarker { columns: bitvec::vec::BitVec::new() }
    }
    pub fn children<'x, I: IntoIterator>(&'x self, children: I)
        -> impl Iterator<Item=(NodeMarker, I::Item)> + 'x
        where I::IntoIter: ExactSizeIterator + 'x,
    {
        let children = children.into_iter();
        let last_idx = children.len().saturating_sub(1);
        children.enumerate().map(move |(idx, child)| {
            let mut columns = self.columns.clone();
            columns.push(last_idx == idx);
            (NodeMarker { columns }, child)
        })
    }
}

impl NodeMarker {
    fn width(&self) -> usize {
        self.columns.len().saturating_sub(1)*3 + 3
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
                    write!(f, "    ")?;
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
            write!(f, " .{}", attr)?;
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

impl table::Contents for Relations<'_> {
    fn width_bounds(&self) -> (usize, usize) {
        let mut min_width = 0;
        let mut width = 0;
        let last_idx = self.0.len().saturating_sub(1);
        for (idx, item) in self.0.iter().enumerate() {
            let item_width = table::display_width(item);
            if last_idx == idx {
                min_width = max(min_width, item_width);
                width += item_width
            } else {
                min_width = max(min_width, item_width + ",".len());
                width += item_width + ", ".len();
            }
        }
        (min_width, width)
    }
    fn height(&self, width: usize) -> usize {
        let mut height = 1;
        let mut col = 0;
        let last_idx = self.0.len().saturating_sub(1);
        for (idx, item) in self.0.iter().enumerate() {
            let item_width = table::display_width(item);
            let comma = if last_idx == idx { "" } else { "," };
            let mut space = if col > 0 { " " } else { "" };
            if col + space.len() + item_width + comma.len() > width {
                height += 1;
                col = 0;
                space = "";
            }
            col += space.len() + item_width + comma.len();
        }
        return height;
    }
    fn render(&self, width: usize, _height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        let mut col = 0;
        let last_idx = self.0.len().saturating_sub(1);
        for (idx, item) in self.0.iter().enumerate() {
            let item_width = table::display_width(item);
            let comma = if last_idx == idx { "" } else { "," };
            let mut space = if col > 0 { " " } else { "" };
            if col + space.len() + item_width + comma.len() > width {
                write!(f, "\n")?;
                col = 0;
                space = "";
            }
            write!(f, "{space}{item}{comma}")?;
            col += space.len() + item_width + comma.len();
        }
        Ok(())
    }
}

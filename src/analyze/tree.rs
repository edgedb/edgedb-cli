use std::collections::BTreeMap;
use std::env;
use std::fmt;

use crate::analyze::model::{Analysis, Plan, IndexCell, Arguments};
use crate::analyze::model::{Shape, ChildName, DebugNode, Cost};
use crate::analyze::table;
use crate::print::Highlight;

static NUMBERS: [char; 10] = ['➊', '➋', '➌', '➍', '➎', '➏', '➐', '➑', '➒', '➓'];


struct Opt<'a, T>(&'a Option<T>);

#[derive(Debug, Clone)]
pub struct NodeMarker {
    columns: bitvec::vec::BitVec,
}

#[derive(Debug, Clone)]
pub struct ShapeNode<'a> {
    marker: NodeMarker,
    attribute: Option<&'a str>,
}

pub fn print_debug_plan(explain: &Analysis) {
    if let Some(debug) = &explain.debug_info {
        if let Some(node) = &debug.full_plan {
            println!("Debug Plan");
            print_debug_node("", node, true);
        }
    }
}

fn print_debug_node(prefix: &str, node: &DebugNode, last: bool) {
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
             ctx=node.contexts.iter()
                 .flat_map(|c| c.index_cell.load())
                 .next().map(|idx| NUMBERS[idx as usize]) // TODO 10 > nums
                 .unwrap_or(if node.contexts.is_empty() {' '} else {'*'}),
    );
    let prefix = prefix.to_string() + if last { "    " } else { "│   " };
    let last_idx = node.plans.len().saturating_sub(1);
    for (idx, node) in node.plans.iter().enumerate() {
        print_debug_node(&prefix, node, idx == last_idx);
    }
}

pub fn print_shape(explain: &Analysis) {
    println!("Shape");
    if let Some(shape) = &explain.coarse_grained {

        let mut header = Vec::with_capacity(3);
        header.push(Box::new("") as Box<_>);
        // TODO(tailhook) column splitter
        header.push(Box::new(table::Right("Cost".emphasize())) as Box<_>);
        // TODO(tailhook) column splitter

        let mut total = Vec::with_capacity(3);
        total.push(Box::new("root") as Box<_>);
        // TODO(tailhook) column splitter
        cost_columns(&mut total, &shape.cost, &explain.arguments);
        // TODO(tailhook) column splitter

        let mut rows = vec![header, total];

        for (child, ch) in NodeMarker::new().children(&shape.children) {
            match &ch.name {
                ChildName::Pointer { name } => {
                    visit_subshape(&mut rows, &explain.arguments,
                                   child, Some(name), &ch.node);
                }
                _ => {
                    visit_subshape(&mut rows, &explain.arguments,
                                   child, None, &ch.node);
                }
            }
        }

        table::render(&rows);
    }
}

fn visit_subshape<'x>(
    result: &mut Vec<Vec<Box<dyn table::Contents + 'x>>>,
    arguments: &Arguments,
    marker: NodeMarker,
    attribute: Option<&'x str>,
    node: &'x Shape,
) {
    let mut row = Vec::with_capacity(3);
    row.push(Box::new(ShapeNode {
        marker: marker.clone(),
        attribute,
    }) as Box<_>);
    // TODO(tailhook) column splitter
    cost_columns(&mut row, &node.cost, arguments);
    // TODO(tailhook) column splitter
    // TODO: row.push(node.relations);
    result.push(row);

    for (child, ch) in marker.children(&node.children) {
        match &ch.name {
            ChildName::Pointer { name } => {
                visit_subshape(result, arguments, child, Some(name), &ch.node);
            }
            _ => {
                visit_subshape(result, arguments, child, None, &ch.node);
            }
        }
    }
}

fn find_indexes(plan: &Plan,
                buf: &mut BTreeMap<(usize, usize), Vec<IndexCell>>) {
    if let Some(ctx) = plan.contexts.last() {
        if ctx.buffer_idx == 0 {
            buf.entry((ctx.start, ctx.end))
                .or_insert_with(Vec::new)
                .push(ctx.index_cell.clone());
        }
    }
    for sub_plan in &plan.subplans {
        find_indexes(sub_plan, buf);
    }
}

fn find_shape_indexes(shape: &Shape,
                      buf: &mut BTreeMap<(usize, usize), Vec<IndexCell>>) {
    if let Some(ctx) = shape.contexts.last() {
        if ctx.buffer_idx == 0 {
            buf.entry((ctx.start, ctx.end))
                .or_insert_with(Vec::new)
                .push(ctx.index_cell.clone());
        }
    }
    for sub in &shape.children {
        find_shape_indexes(&sub.node, buf);
    }
}

fn find_debug_indexes(plan: &DebugNode,
                      buf: &mut BTreeMap<(usize, usize), Vec<IndexCell>>) {
    if let Some(ctx) = plan.contexts.last() {
        if ctx.buffer_idx == 0 {
            buf.entry((ctx.start, ctx.end))
                .or_insert_with(Vec::new)
                .push(ctx.index_cell.clone());
        }
    }
    for sub_plan in &plan.plans {
        find_debug_indexes(sub_plan, buf);
    }
}

pub fn print_contexts(explain: &Analysis) {
    println!("Contexts");
    let mut context_indexes = BTreeMap::new();
    if let Some(plan) = &explain.fine_grained {
        find_indexes(plan, &mut context_indexes);
    }
    if let Some(shape) = &explain.coarse_grained {
        find_shape_indexes(shape, &mut context_indexes);
    }
    if env::var_os("_EDGEDB_ANALYZE_DEBUG_PLAN")
        .map(|x| !x.is_empty()).unwrap_or(false)
    {
        if let Some(debug) = &explain.debug_info {
            if let Some(node) = &debug.full_plan {
                find_debug_indexes(node, &mut context_indexes);
            }
        }
    }

    let text = &explain.buffers[0];
    let mut out = String::with_capacity(text.len() + context_indexes.len());
    let mut offset = 0;
    let mut iter = context_indexes.iter().enumerate().peekable();
    while let Some((idx, (&(start, end), cells))) = iter.next() {
        if start < offset {
            continue; // TODO(tailhook)
        }
        out.push_str(&text[offset..start]);
        for c in cells {
            c.store(Some(idx as u32));
        }
        if iter.peek().map(|(_, (&(next, _), _))| next > end).unwrap_or(true)
           && !text[start..end].trim().contains('\n')
        {
            out.push_str("\x1B[4m");  // underline
            out.push(NUMBERS[idx]);  // TODO(tailhook) more than 10
            out.push(' ');  // some terminals are bad with wide chars
            out.push_str(&text[start..end]);
            out.push_str("\x1B[0m");  // reset
            offset = end;
        } else {
            out.push(NUMBERS[idx]);  // TODO(tailhook) more than 10
            out.push(' ');  // some terminals are bad with wide chars
            offset = start;
        }
    }
    out.push_str(&text[offset..]);
    println!("{}", out);
}

pub fn print_tree(explain: &Analysis) {
    println!("Full Plan Tree");
    if let Some(tree) = &explain.fine_grained {
        print_tree_node("", tree, true);
    }
}

fn print_tree_node(prefix: &str, node: &Plan, last: bool) {
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
                     node.contexts.iter()
                     .flat_map(|c| c.index_cell.load())
                     .next().map(|idx| NUMBERS[idx as usize]) // TODO 10 > nums
                     .unwrap_or(if node.contexts.is_empty() {' '} else {'*'})
                 } else {
                     ' '
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
        print_tree_node(&prefix, node, idx == last_idx);
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
        self.columns.len().saturating_sub(1)*4 + 2
    }
    fn render_head(&self, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        let cols = &self.columns;
        if cols.len() <= 1 {
            if let Some(last) = cols.last() {
                if *last {
                    write!(f, "╰─")?;
                } else {
                    write!(f, "├─")?;
                }
            }
        } else {
            let mut iter = cols.iter().take(self.columns.len() - 1);
            if let Some(first) = iter.next() {
                if *first {
                    write!(f, "  ")?;
                } else {
                    write!(f, "│ ")?;
                }
            }
            for i in iter {
                if *i {
                    write!(f, "    ")?;
                } else {
                    write!(f, "  │ ")?;
                }
            }
            if let Some(last) = cols.last() {
                if *last {
                    write!(f, "  ╰─")?;
                } else {
                    write!(f, "  ├─")?;
                }
            }
        }
        Ok(())
    }
    fn render_tail(&self, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        for _ in 1..height {
            for i in &self.columns {
                if *i {
                    write!(f, "  ")?;
                } else {
                    write!(f, "│ ")?;
                }
            }
        }
        Ok(())
    }
}

impl table::Contents for ShapeNode<'_> {
    fn width_bounds(&self) -> (usize, usize) {
        let mwidth = self.marker.width();
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
        if let Some(attr) = self.attribute {
            write!(f, " .{}", attr)?;
        }
        self.marker.render_tail(height, f)?;
        Ok(())
    }
}

fn cost_columns(
    row: &mut Vec<Box<dyn table::Contents + '_>>,
    cost: &Cost,
    args: &Arguments
) {
    if args.execute {
        // TODO(tailhook) use actual time
        row.push(Box::new(table::Float(cost.total_cost as f64)));
    } else {
        row.push(Box::new(table::Float(cost.total_cost as f64)));
    }
}

use std::collections::BTreeMap;
use std::env;
use std::fmt;

use crate::analyze::model::{Analysis, Plan, IndexCell};
use crate::analyze::model::{Shape, ChildName, DebugNode};

static NUMBERS: [char; 10] = ['➊', '➋', '➌', '➍', '➎', '➏', '➐', '➑', '➒', '➓'];


struct Opt<'a, T>(&'a Option<T>);

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
        print_subshape("", None, shape, true);
    }
}

fn print_subshape(prefix: &str, attribute: Option<&str>, node: &Shape, last: bool)
{
    let marker = if last { "╰──" } else { "├──" };
    let mut title = Vec::with_capacity(4);
    if let Some(attribute) = attribute {
        title.push(".");
        title.push(attribute);
    }
    for (idx, relation) in node.relations.iter().enumerate() {
        if idx == 0 && !title.is_empty() {
            title.push(": ");
        } else if idx > 0 {
            title.push(", ");
        }
        title.push(relation);
    }
    let cost = format!("(cost={cost})",
         cost=node.cost.total_cost,
    );
    println!("{prefix}{marker}{ctx} {title} {cost}",
             title=title.join(""),
             ctx=node.contexts.iter()
                 .flat_map(|c| c.index_cell.load())
                 .next().map(|idx| NUMBERS[idx as usize]) // TODO 10 > nums
                 .unwrap_or(if node.contexts.is_empty() {' '} else {'*'}),
    );

    let prefix = prefix.to_string() + if last { "   " } else { "│  " };

    let last_idx = node.children.len().saturating_sub(1);
    for (idx, ch) in node.children.iter().enumerate() {
        match &ch.name {
            ChildName::Pointer { name } => {
                print_subshape(&prefix, Some(name), &ch.node, idx == last_idx);
            }
            _ => {
                print_subshape(&prefix, None, &ch.node, idx == last_idx);
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

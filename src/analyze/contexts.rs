use std::fmt;
use std::collections::HashMap;
use std::env;

use crate::analyze::model::Analysis;
use crate::highlight;


static NUMBERS: [char; 10] = ['➊', '➋', '➌', '➍', '➎', '➏', '➐', '➑', '➒', '➓'];

pub struct Number(pub u32);

pub struct OptNumber(Option<Number>);

pub fn print_contexts(explain: &Analysis) {
    let mut context_indexes = HashMap::new();
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

    for text in &explain.buffers {
        print_buffer(&text);
    }
}

fn print_buffer(text: &str) {
    let mut out = String::with_capacity(text.len());
    highlight::edgeql(&mut out, &text, styler);

    println!("{}", out);

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
            write!(&mut out, "{}", Number(idx)).expect("write succeeds");
            out.push(' ');  // some terminals are bad with wide chars
            out.push_str(&text[start..end]);
            out.push_str("\x1B[0m");  // reset
            offset = end;
        } else {
            write!(&mut out, "{}", Number(idx)).expect("write succeeds");
            out.push(' ');  // some terminals are bad with wide chars
            offset = start;
        }
    }
    out.push_str(&text[offset..]);
    println!("{}", out);
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            0..=9 => NUMBERS[self.0 as usize].fmt(f),
            _ => write!(f, "({})", self.0+1),
        }
    }
}

impl fmt::Display for OptNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(x) => x.fmt(f),
            None => Ok(()),
        }
    }
}

fn find_indexes<'a>(plan: &'a Plan,
                buf: &mut HashMap<ContextId, &'a Context>) {
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

fn find_shape_indexes<'a>(shape: &'a Shape,
                      buf: &mut HashMap<ContextId, &'a Context>) {
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

fn find_debug_indexes<'a>(plan: &'a DebugNode,
                      buf: &mut BTreeMap<ContextId, &'a Context>) {
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

impl Analysis {
    fn context_num(&self, contexts: &[Context]) -> OptNumber {
        for ctx in contexts {
            if let Some(number) = self.contexts.get(ctx.id) {
                return OptNumber(Some(number));
            }
        }
        return OptNumber(Nont);
    }
}

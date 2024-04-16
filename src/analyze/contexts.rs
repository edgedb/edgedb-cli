use std::fmt::{self, Write};
use std::collections::{BTreeMap, HashMap};

use crate::analyze::model::{Analysis, AnalysisData, Context, ContextId};
use crate::analyze::model::{Shape, DebugNode, Plan, Buffer, ContextSpan};
use crate::analyze::table;
use crate::highlight;
use crate::print::style::Styler;

static NUMBERS: [char; 10] = ['➊', '➋', '➌', '➍', '➎', '➏', '➐', '➑', '➒', '➓'];

#[derive(Debug, Clone, Copy)]
pub struct Number(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct OptNumber(Option<Number>);

type BufferIdx = usize;
type Offset = usize;
type Length = usize;

struct Gather<'a> {
    by_location: BTreeMap<(BufferIdx, Offset), (Length, Vec<ContextId>)>,
    siblings: HashMap<ContextId, &'a [Context]>,
}

impl<'a> Gather<'a> {
    fn scan(explain: &AnalysisData) -> Gather {
        let mut me = Gather {
            by_location: BTreeMap::new(),
            siblings: HashMap::new(),
        };
        if let Some(shape) = &explain.coarse_grained {
            me.scan_shape(shape);
        }
        if let Some(plan) = &explain.fine_grained {
            me.scan_expanded(plan);
        }
        if let Some(debug) = &explain.debug_info {
            if let Some(node) = &debug.full_plan {
                me.scan_debug(node);
            }
        }
        me
    }
    fn scan_shape(&mut self, shape: &'a Shape) {
        for ctx in &shape.contexts {
            let len = ctx.end.saturating_sub(ctx.start);
            self.by_location.entry((ctx.buffer_idx, ctx.start))
                .or_insert_with(|| (len, Vec::new()))
                .1.push(ctx.context_id);
            self.siblings.insert(ctx.context_id, &shape.contexts);
        }
        for sub in &shape.children {
            self.scan_shape(&sub.node);
        }
    }
    fn scan_expanded(&mut self, plan: &'a Plan) {
        for ctx in &plan.contexts {
            let len = ctx.end.saturating_sub(ctx.start);
            self.by_location.entry((ctx.buffer_idx, ctx.start))
                .or_insert_with(|| (len, Vec::new()))
                .1.push(ctx.context_id);
            self.siblings.insert(ctx.context_id, &plan.contexts);
        }
        for sub_plan in &plan.subplans {
           self.scan_expanded(sub_plan);
        }
    }
    fn scan_debug(&mut self, plan: &'a DebugNode) {
        for ctx in &plan.contexts {
            let len = ctx.end.saturating_sub(ctx.start);
            self.by_location.entry((ctx.buffer_idx, ctx.start))
                .or_insert_with(|| (len, Vec::new()))
                .1.push(ctx.context_id);
            self.siblings.insert(ctx.context_id, &plan.contexts);
        }
        for sub_plan in &plan.plans {
            self.scan_debug(sub_plan);
        }
    }
}

pub fn preprocess(input: AnalysisData) -> Analysis {

    let Gather { by_location, siblings } = Gather::scan(&input);

    let mut contexts = HashMap::new();
    let mut index = 0;
    let mut buffers_ctx = input.buffers.iter()
        .map(|_| Vec::new() )
        .collect::<Vec<_>>();
    for (&(buf, offset), &(len, ref items)) in &by_location {
        if let Some(&num) = items.iter().flat_map(|c| contexts.get(c)).next() {
            if let Some(ctxs) = buffers_ctx.get_mut(buf) { ctxs.push(ContextSpan {offset, len, num}) }
            continue;
        }
        let num = Number(index);
        index += 1;

        for c_id in items {
            if !contexts.contains_key(c_id) {  // TODO(tailhook) use try_insert
                contexts.insert(*c_id, num);
                if let Some(siblings) = siblings.get(c_id) {
                    for sub in *siblings {
                        contexts.entry(sub.context_id).or_insert(num);
                    }
                }
            }
        }
        if let Some(ctxs) = buffers_ctx.get_mut(buf) { ctxs.push(ContextSpan {offset, len, num}) }
    }
    Analysis {
        buffers: input.buffers.into_iter().zip(buffers_ctx)
            .map(|(text, contexts)| Buffer { text, contexts })
            .collect(),
        fine_grained: input.fine_grained,
        coarse_grained: input.coarse_grained,
        debug_info: input.debug_info,
        arguments: input.arguments,
        contexts,
    }
}

pub fn print(explain: &Analysis) {
    if let Some(first) = explain.buffers.first() {
        print_buffer(first, "Query");
    }
    for (n, buf) in explain.buffers[1..].iter().enumerate() {
        if !buf.contexts.is_empty() {
            print_buffer(buf, format_args!("Computable {n}"));
        }
    }
}

fn print_buffer(buffer: &Buffer, title: impl fmt::Display) {
    let mut markup = String::with_capacity(buffer.text.len());
    let styler = Styler::dark_256();
    highlight::edgeql(&mut markup, &buffer.text, &styler);

    let mut out = String::with_capacity(markup.len());
    let mut counter = table::Counter::new();
    let mut iter = buffer.contexts.iter().peekable();
    let mut input = markup.chars();
    while let Some(context) = iter.next() {
        while counter.offset < context.offset {
            let Some(c) = input.next() else { break };
            counter.add_char(c);
            out.push(c);
        }
        let end = context.offset.saturating_add(context.len);
        if iter.peek().map(|next| next.offset > end).unwrap_or(true)
           && !buffer.text[context.offset..end].trim().contains('\n')
        {
            out.push_str("\x1B[4m");  // underline
            write!(&mut out, "{}", context.num).expect("write succeeds");
            out.push(' ');  // some terminals are bad with wide chars
            for c in input.by_ref() {
                // TODO(tailhook) catch reset and restore underline
                counter.add_char(c);
                out.push(c);
                if counter.offset >= end {
                    break;
                }
                if out.ends_with("\x1B[0m") {
                    out.push_str("\x1B[4m");  // restore underline
                }
            }
            out.push_str("\x1B[0m");  // reset
        } else {
            write!(&mut out, "{}", context.num).expect("write succeeds");
            out.push(' ');  // some terminals are bad with wide chars
        }
    }
    out.push_str(input.as_str());

    let width = out.lines()
        .map(table::str_width)
        .max()
        .unwrap_or(80);

    table::print_title(title, width);
    println!("{}", out);
    println!();
}


impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            0..=9 => write!(f, "{} ", NUMBERS[self.0 as usize]),
            _ => write!(f, "({}) ", self.0+1),
        }
    }
}

impl fmt::Display for OptNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            Some(x) => x.fmt(f),
            None => Ok(()),
        }
    }
}

impl Analysis {
    pub fn context(&self, contexts: &[Context]) -> OptNumber {
        for ctx in contexts {
            if let Some(num) = self.contexts.get(&ctx.context_id) {
                return OptNumber(Some(*num));
            }
        }
        OptNumber(None)
    }
}

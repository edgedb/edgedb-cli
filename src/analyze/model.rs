use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

use crate::analyze::contexts::Number;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContextId(u64);

#[derive(Deserialize, Debug)]
pub struct AnalysisData {
    pub buffers: Vec<String>,
    pub fine_grained: Option<Plan>,
    pub coarse_grained: Option<Shape>,
    pub debug_info: Option<DebugInfo>,
    pub arguments: Arguments,
}

#[derive(Debug)]
pub struct Analysis {
    pub buffers: Vec<Buffer>,
    pub fine_grained: Option<Plan>,
    pub coarse_grained: Option<Shape>,
    pub debug_info: Option<DebugInfo>,
    pub arguments: Arguments,
    pub contexts: HashMap<ContextId, Number>,
}

#[derive(Debug)]
pub struct ContextSpan {
    pub offset: usize,
    pub len: usize,
    pub num: Number,
}

#[derive(Debug)]
pub struct Buffer {
    pub text: String,
    pub contexts: Vec<ContextSpan>,
}

#[derive(Deserialize, Debug)]
pub struct Arguments {
    pub execute: bool,
    #[serde(default)]
    pub buffers: bool,
}

#[derive(Deserialize, Debug)]
pub struct DebugInfo {
    pub full_plan: Option<DebugNode>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "value")]
pub enum PropValue {
    #[serde(rename = "kB")]
    Kbytes(u64),
    #[serde(rename = "ms")]
    Millis(f64),
    #[serde(rename = "expr")]
    Expr(String),
    #[serde(rename = "index")]
    Index(String),
    #[serde(rename = "relation")]
    Relation(String),
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "int")]
    Int(i64),
    #[serde(rename = "float")]
    Float(f64),

    #[serde(rename = "list:kB")]
    ListKbytes(Vec<u64>),
    #[serde(rename = "list:ms")]
    ListMillis(Vec<f64>),
    #[serde(rename = "list:expr")]
    ListExpr(Vec<String>),
    #[serde(rename = "list:index")]
    ListIndex(Vec<String>),
    #[serde(rename = "list:relation")]
    ListRelation(Vec<String>),
    #[serde(rename = "list:text")]
    ListText(Vec<String>),
    #[serde(rename = "list:int")]
    ListInt(Vec<i64>),
    #[serde(rename = "list:float")]
    ListFloat(Vec<f64>),
}

#[derive(Deserialize, Debug)]
pub struct Prop {
    pub title: String,
    pub important: bool,
    #[serde(flatten)]
    pub value: PropValue,
}

#[derive(Deserialize, Debug)]
pub struct Stage {
    pub plan_id: uuid::Uuid,
    pub plan_type: String,
    pub properties: Vec<Prop>,
    #[serde(flatten)]
    pub cost: Cost,
}

#[derive(Deserialize, Debug)]
pub struct Plan {
    pub alias: Option<String>,
    #[serde(default)]
    pub contexts: Vec<Context>,
    pub pipeline: Vec<Stage>,
    pub subplans: Vec<Plan>,
}

#[derive(Deserialize, Debug)]
pub struct Cost {
    pub plan_rows: u64,
    pub plan_width: u64,
    pub startup_cost: f64,
    pub total_cost: f64,

    pub actual_startup_time: Option<f64>,
    pub actual_total_time: Option<f64>,
    pub actual_rows: Option<f64>,
    pub actual_loops: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum ChildName {
    #[serde(rename = "pointer")]
    Pointer { name: String },
    #[serde(rename = "filter")]
    Filter,
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug)]
pub struct Child {
    #[serde(flatten)]
    pub name: ChildName,
    pub node: Shape,
}

#[derive(Deserialize, Debug)]
pub struct Shape {
    pub plan_id: uuid::Uuid,
    #[serde(default)]
    pub children: Vec<Child>,
    #[serde(default)]
    pub contexts: Vec<Context>,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(flatten)]
    pub cost: Cost,
}

#[derive(Deserialize, Debug)]
pub struct Context {
    pub start: usize,
    pub end: usize,
    pub buffer_idx: usize,
    pub text: String,
    #[serde(skip, default)]
    pub context_id: ContextId,
}

#[derive(Deserialize, Debug)]
pub struct DebugNode {
    pub node_type: String,
    pub parent_relationship: Option<String>,
    #[serde(default)]
    pub plans: Vec<DebugNode>,
    pub alias: Option<String>,
    pub index_name: Option<String>,
    pub relation_name: Option<String>,
    #[serde(default)]
    pub contexts: Vec<Context>,

    #[serde(flatten)]
    pub cost: Cost,
    #[serde(flatten)]
    pub properties: HashMap<String, serde_json::Value>,
}

impl fmt::Display for PropValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use PropValue::*;

        match self {
            Kbytes(val) => {
                // TODO(tailhook) use mega/gigabytes
                val.fmt(f)?;
                write!(f, "kB")?;
            }
            Millis(val) => {
                // TODO(tailhook) use seconds, minutes, hours?
                val.fmt(f)?;
                write!(f, "ms")?;
            }
            Expr(val) => {
                write!(f, "{:?}", val)?;
            }
            Index(val) => {
                val.fmt(f)?;
            }
            Relation(val) => {
                val.fmt(f)?;
            }
            Text(val) => {
                val.fmt(f)?;
            }
            Int(val) => {
                val.fmt(f)?;
            }
            Float(val) => {
                // TODO(tailhook) figure out the best format
                val.fmt(f)?;
            }

            ListKbytes(_) => todo!(),
            ListMillis(_) => todo!(),
            ListExpr(_) => todo!(),
            ListIndex(_) => todo!(),
            ListRelation(_) => todo!(),
            ListText(_) => todo!(),
            ListInt(_) => todo!(),
            ListFloat(_) => todo!(),
        }
        Ok(())
    }
}

impl Default for ContextId {
    fn default() -> ContextId {
        ContextId(NEXT_ID.fetch_add(1, Ordering::SeqCst))
    }
}

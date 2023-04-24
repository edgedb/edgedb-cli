use std::fmt;
use std::sync::Arc;

use serde::Deserialize;


pub type IndexCell = Arc<crossbeam_utils::atomic::AtomicCell<Option<u32>>>;


#[derive(Deserialize, Debug)]
pub struct Analysis {
    pub buffers: Vec<String>,
    pub fine_grained: Option<Plan>,
    pub coarse_grained: Option<Shape>,
    pub debug_info: Option<DebugInfo>,
}

#[derive(Deserialize, Debug)]
pub struct DebugInfo {
    pub full_plan: Option<DebugNode>,
}

#[derive(Deserialize, Debug)]
#[serde(tag="type", content="value")]
pub enum PropValue {
    #[serde(rename="kB")]
    Kbytes(u64),
    #[serde(rename="ms")]
    Millis(f64),
    #[serde(rename="expr")]
    Expr(String),
    #[serde(rename="index")]
    Index(String),
    #[serde(rename="relation")]
    Relation(String),
    #[serde(rename="text")]
    Text(String),
    #[serde(rename="int")]
    Int(i64),
    #[serde(rename="float")]
    Float(f64),

    #[serde(rename="list:kB")]
    ListKbytes(Vec<u64>),
    #[serde(rename="list:ms")]
    ListMillis(Vec<f64>),
    #[serde(rename="list:expr")]
    ListExpr(Vec<String>),
    #[serde(rename="list:index")]
    ListIndex(Vec<String>),
    #[serde(rename="list:relation")]
    ListRelation(Vec<String>),
    #[serde(rename="list:text")]
    ListText(Vec<String>),
    #[serde(rename="list:int")]
    ListInt(Vec<i64>),
    #[serde(rename="list:float")]
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
    pub startup_cost: f32,
    pub total_cost: f32,

    pub actual_startup_time: Option<f32>,
    pub actual_total_times: Option<f32>,
    pub actual_rows: Option<f32>,
    pub actual_loops: Option<f32>,
}

#[derive(Deserialize, Debug)]
#[serde(tag="kind")]
pub enum ChildName {
    #[serde(rename="pointer")]
    Pointer { name: String },
    #[serde(rename="filter")]
    Filter,
    #[serde(other)]
    Other
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
    pub index_cell: IndexCell,
}

#[derive(Deserialize, Debug)]
pub struct DebugNode {
    pub node_type: String,
    pub parent_relationship: Option<String>,
    #[serde(default)]
    pub plans: Vec<DebugNode>,
    #[serde(default)]
    pub is_shape: bool,
    pub alias: Option<String>,
    pub index_name: Option<String>,
    pub relation_name: Option<String>,
    #[serde(default)]
    pub simplified_paths: Vec<(String, String)>,
    #[serde(default)]
    pub contexts: Vec<Context>,

    pub plan_width: u64,
    pub plan_rows: u64,
    pub startup_cost: f64,
    pub total_cost: f64,
    pub actual_startup_time: Option<f64>,
    pub actual_total_time: Option<f64>,
    pub actual_rows: Option<f64>,
    pub actual_loops: Option<f64>,
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

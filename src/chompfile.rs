use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Chompfile {
    pub version: f32,
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub task: Vec<ChompTaskMaybeTemplated>,
    #[serde(default)]
    pub template: Vec<ChompTemplate>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
pub struct ChompTaskMaybeTemplated {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub deps: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub run: Option<String>,
    pub engine: Option<String>,
    pub template: Option<String>,
    #[serde(default)]
    pub args: BTreeMap<String, toml::value::Value>,
}

// Pending https://github.com/denoland/deno/issues/13185
#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
pub struct ChompTaskMaybeTemplatedNoDefault {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub deps: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub run: Option<String>,
    pub engine: Option<String>,
    pub template: Option<String>,
    pub args: Option<BTreeMap<String, toml::value::Value>>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
pub struct ChompTemplate {
    pub name: String,
    pub definition: String,
}

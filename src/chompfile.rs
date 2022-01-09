use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChompEngine {
    Cmd,
    Node,
}

impl Default for ChompEngine {
    fn default () -> Self {
        ChompEngine::Cmd
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chompfile {
    pub version: f32,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub task: Vec<ChompTaskMaybeTemplated>,
    #[serde(default)]
    pub template: Vec<ChompTemplate>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TargetCheck {
    Mtime,
    Exists,
}

impl Default for TargetCheck {
    fn default () -> Self {
        TargetCheck::Mtime
    }
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
pub struct ChompTaskMaybeTemplated {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub target_check: Option<TargetCheck>,
    #[serde(default)]
    pub deps: Vec<String>,
    pub serial: Option<bool>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub run: Option<String>,
    pub engine: Option<ChompEngine>,
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
    pub target_check: Option<TargetCheck>,
    pub deps: Option<Vec<String>>,
    pub serial: Option<bool>,
    pub env: Option<BTreeMap<String, String>>,
    pub run: Option<String>,
    pub engine: Option<ChompEngine>,
    pub template: Option<String>,
    pub args: Option<BTreeMap<String, toml::value::Value>>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
pub struct ChompTemplate {
    pub name: String,
    pub definition: String,
}

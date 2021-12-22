use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Chompfile {
    pub version: f32,
    pub env: Option<BTreeMap<String, String>>,
    pub task: Option<Vec<ChompTaskMaybeTemplated>>,
    pub template: Option<Vec<ChompTemplate>>,
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
    pub args: Option<BTreeMap<String, String>>
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
pub struct TemplateOption {
    pub name: String,
    #[serde(rename="type")]
    pub type_: Option<String>,
    pub default: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
pub struct ChompTemplate {
    pub name: String,
    pub option: Option<Vec<TemplateOption>>,
    pub definition: TemplateDefinition,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
pub struct TemplateDefinition {
    pub task: String
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ChompEngine {
    Cmd,
    Node,
    Deno,
}

impl Default for ChompEngine {
    fn default () -> Self {
        ChompEngine::Cmd
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Chompfile {
    pub version: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub debug: bool,
    pub default_task: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub server: ServerOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    pub task: Vec<ChompTaskMaybeTemplated>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub template_options: HashMap<String, HashMap<String, toml::value::Value>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub extensions: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
pub struct ServerOptions {
    #[serde(default, skip_serializing_if = "is_default")]
    pub root: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub port: u16,
}

impl Default for ServerOptions {
    fn default () -> Self {
        ServerOptions {
            root: ".".to_string(),
            port: 8080
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum InvalidationCheck {
    NotFound,
    Mtime,
    Always,
}

impl Default for InvalidationCheck {
    fn default () -> Self {
        InvalidationCheck::Mtime
    }
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ChompTaskMaybeTemplated {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub dep: Option<String>,
    pub deps: Option<Vec<String>>,
    pub serial: Option<bool>,
    pub invalidation: Option<InvalidationCheck>,
    pub display: Option<bool>,
    pub engine: Option<ChompEngine>,
    pub run: Option<String>,
    pub cwd: Option<String>,
    pub template: Option<String>,
    pub template_options: Option<HashMap<String, toml::value::Value>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub env: HashMap<String, String>,
}

impl ChompTaskMaybeTemplated {
    pub fn targets_vec (&self) -> Vec<String> {
        if let Some(ref target) = self.target {
            vec![target.to_string()]
        }
        else if let Some(ref targets) = self.targets {
            targets.clone()
        }
        else {
            vec![]
        }
    }
    pub fn deps_vec (&self) -> Vec<String> {
        if let Some(ref dep) = self.dep {
            vec![dep.to_string()]
        }
        else if let Some(ref deps) = self.deps {
            deps.clone()
        }
        else {
            vec![]
        }
    }
}

impl ChompTaskMaybeTemplatedNoDefault {
    pub fn targets_vec (&self) -> Vec<String> {
        if let Some(ref target) = self.target {
            vec![target.to_string()]
        }
        else if let Some(ref targets) = self.targets {
            targets.clone()
        }
        else {
            vec![]
        }
    }
    pub fn deps_vec (&self) -> Vec<String> {
        if let Some(ref dep) = self.dep {
            vec![dep.to_string()]
        }
        else if let Some(ref deps) = self.deps {
            deps.clone()
        }
        else {
            vec![]
        }
    }
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

// Pending https://github.com/denoland/deno/issues/13185
#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChompTaskMaybeTemplatedNoDefault {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub dep: Option<String>,
    pub deps: Option<Vec<String>>,
    pub serial: Option<bool>,
    pub invalidation: Option<InvalidationCheck>,
    pub display: Option<bool>,
    pub engine: Option<ChompEngine>,
    pub run: Option<String>,
    pub cwd: Option<String>,
    pub template: Option<String>,
    pub template_options: Option<HashMap<String, toml::value::Value>>,
    pub env: Option<HashMap<String, String>>,
}

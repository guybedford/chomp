// Chomp Task Runner
// Copyright (C) 2022  Guy Bedford

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ChompEngine {
    Shell,
    Node,
    Deno,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskDisplay {
    None,
    Dot,
    InitStatus,
    StatusOnly,
    InitOnly,
}

impl Default for TaskDisplay {
    fn default() -> Self {
        TaskDisplay::InitStatus
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStdio {
    All,
    NoStdin,
    StdoutOnly,
    StderrOnly,
    None,
}

impl Default for TaskStdio {
    fn default() -> Self {
        TaskStdio::All
    }
}

impl Default for ChompEngine {
    fn default() -> Self {
        ChompEngine::Shell
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Chompfile {
    pub version: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub echo: bool,
    pub default_task: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub env_default: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub server: ServerOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    pub task: Vec<ChompTaskMaybeTemplated>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub template_options: HashMap<String, HashMap<String, toml::value::Value>>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
pub struct ServerOptions {
    #[serde(default = "default_root", skip_serializing_if = "is_default")]
    pub root: String,
    #[serde(default = "default_port", skip_serializing_if = "is_default")]
    pub port: u16,
}

fn default_root() -> String {
    ".".to_string()
}

fn default_port() -> u16 {
    5776
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions {
            root: ".".to_string(),
            port: default_port(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum InvalidationCheck {
    NotFound,
    Mtime,
    Always,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationCheck {
    OkTargets,
    TargetsOnly,
    OkOnly,
    None,
}

impl Default for ValidationCheck {
    fn default() -> Self {
        ValidationCheck::OkTargets
    }
}

impl Default for InvalidationCheck {
    fn default() -> Self {
        InvalidationCheck::Mtime
    }
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChompTaskMaybeTemplated {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub dep: Option<String>,
    pub deps: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub serial: Option<bool>,
    pub invalidation: Option<InvalidationCheck>,
    pub validation: Option<ValidationCheck>,
    pub display: Option<TaskDisplay>,
    pub stdio: Option<TaskStdio>,
    pub engine: Option<ChompEngine>,
    pub run: Option<String>,
    pub cwd: Option<String>,
    pub env_replace: Option<bool>,
    pub template: Option<String>,
    pub echo: Option<bool>,
    pub template_options: Option<HashMap<String, toml::value::Value>>,
    pub env: Option<HashMap<String, String>>,
    pub env_default: Option<HashMap<String, String>>,
}

impl ChompTaskMaybeTemplated {
    pub fn new() -> Self {
        ChompTaskMaybeTemplated {
            name: None,
            run: None,
            args: None,
            cwd: None,
            deps: None,
            dep: None,
            targets: None,
            target: None,
            display: None,
            engine: None,
            env_replace: None,
            env: None,
            env_default: None,
            echo: None,
            invalidation: None,
            validation: None,
            serial: None,
            stdio: None,
            template: None,
            template_options: None,
        }
    }
    pub fn targets_vec(&self) -> Vec<String> {
        if let Some(ref target) = self.target {
            vec![target.to_string()]
        } else if let Some(ref targets) = self.targets {
            targets.clone()
        } else {
            vec![]
        }
    }
    pub fn deps_vec(&self) -> Vec<String> {
        if let Some(ref dep) = self.dep {
            vec![dep.to_string()]
        } else if let Some(ref deps) = self.deps {
            deps.clone()
        } else {
            vec![]
        }
    }
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

#[derive(Debug, Serialize, PartialEq, Deserialize, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ChompTaskMaybeTemplatedJs {
    pub name: Option<String>,
    pub target: Option<String>,
    pub targets: Option<Vec<String>>,
    pub dep: Option<String>,
    pub deps: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub serial: Option<bool>,
    pub invalidation: Option<InvalidationCheck>,
    pub validation: Option<ValidationCheck>,
    pub display: Option<TaskDisplay>,
    pub stdio: Option<TaskStdio>,
    pub engine: Option<ChompEngine>,
    pub run: Option<String>,
    pub cwd: Option<String>,
    pub echo: Option<bool>,
    pub env_replace: Option<bool>,
    pub template: Option<String>,
    pub template_options: Option<HashMap<String, toml::value::Value>>,
    pub env: Option<HashMap<String, String>>,
    pub env_default: Option<HashMap<String, String>>,
}

impl Into<ChompTaskMaybeTemplated> for ChompTaskMaybeTemplatedJs {
    fn into(self) -> ChompTaskMaybeTemplated {
        ChompTaskMaybeTemplated {
            cwd: self.cwd,
            name: self.name,
            args: self.args,
            target: self.target,
            targets: self.targets,
            display: self.display,
            stdio: self.stdio,
            invalidation: self.invalidation,
            validation: self.validation,
            dep: self.dep,
            deps: self.deps,
            echo: self.echo,
            serial: self.serial,
            env_replace: self.env_replace,
            env: self.env,
            env_default: self.env_default,
            run: self.run,
            engine: self.engine,
            template: self.template,
            template_options: self.template_options,
        }
    }
}

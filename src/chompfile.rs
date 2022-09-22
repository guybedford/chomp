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

use anyhow::Result;
use directories::UserDirs;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env::current_dir,
    path::{Component, Path, PathBuf},
};

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
    NotOk,
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum WatchInvalidation {
    RestartRunning,
    SkipRunning,
}

impl Default for WatchInvalidation {
    fn default() -> Self {
        WatchInvalidation::RestartRunning
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
    pub watch_invalidation: Option<WatchInvalidation>,
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
            watch_invalidation: None,
        }
    }
    pub fn targets_vec(&self) -> Result<Vec<String>> {
        if let Some(ref target) = self.target {
            let target_str = resolve_path(target);
            Ok(vec![target_str])
        } else if let Some(ref targets) = self.targets {
            let targets = targets.iter().map(|t| resolve_path(&t)).collect();
            Ok(targets)
        } else {
            Ok(vec![])
        }
    }
    pub fn deps_vec(&self, chompfile: &Chompfile) -> Result<Vec<String>> {
        let names = chompfile
            .task
            .iter()
            .filter(|&t| t.name.is_some())
            .map(|t| t.name.as_ref().unwrap())
            .collect::<Vec<_>>();

        if let Some(ref dep) = self.dep {
            let dep_str = if names.contains(&dep) || skip_special_chars(dep) {
                dep.to_string()
            } else {
                resolve_path(dep)
            };
            Ok(vec![dep_str])
        } else if let Some(ref deps) = self.deps {
            let deps = deps
                .iter()
                .map(|dep| {
                    if names.contains(&dep) || skip_special_chars(dep) {
                        dep.to_owned()
                    } else {
                        resolve_path(dep)
                    }
                })
                .collect();
            Ok(deps)
        } else {
            Ok(vec![])
        }
    }
}

fn skip_special_chars(s: &String) -> bool {
    s.contains(':') || s.contains("&prev") || s.contains("&next")
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
    pub watch_invalidation: Option<WatchInvalidation>,
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
            watch_invalidation: self.watch_invalidation,
        }
    }
}

fn resolve_path(target: &String) -> String {
    path_from(current_dir().unwrap(), target.as_str())
        .to_str()
        .unwrap()
        .to_string()
}
/// https://stackoverflow.com/questions/68231306/stdfscanonicalize-for-files-that-dont-exist
/// build a usable path from a user input which may be absolute
/// (if it starts with / or ~) or relative to the supplied base_dir.
/// (we might want to try detect windows drives in the future, too)
pub fn path_from<P: AsRef<Path>>(base_dir: P, input: &str) -> PathBuf {
    let tilde = Regex::new(r"^~(/|$)").unwrap();
    if input.starts_with('/') {
        // if the input starts with a `/`, we use it as is
        input.into()
    } else if tilde.is_match(input) {
        // if the input starts with `~` as first token, we replace
        // this `~` with the user home directory
        PathBuf::from(&*tilde.replace(input, |c: &Captures| {
            if let Some(user_dirs) = UserDirs::new() {
                format!("{}{}", user_dirs.home_dir().to_string_lossy(), &c[1],)
            } else {
                // warn!("no user dirs found, no expansion of ~");
                c[0].to_string()
            }
        }))
    } else {
        // we put the input behind the source (the selected directory
        // or its parent) and we normalize so that the user can type
        // paths with `../`
        normalize_path(base_dir.as_ref().join(input))
    }
}

/// Improve the path to try remove and solve .. token.
///
/// This assumes that `a/b/../c` is `a/c` which might be different from
/// what the OS would have chosen when b is a link. This is OK
/// for broot verb arguments but can't be generally used elsewhere
///
/// This function ensures a given path ending with '/' still
/// ends with '/' after normalization.
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let ends_with_slash = path.as_ref().to_str().map_or(false, |s| s.ends_with('/'));
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match &component {
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            _ => {
                normalized.push(component);
            }
        }
    }
    if ends_with_slash {
        normalized.push("");
    }
    normalized
}

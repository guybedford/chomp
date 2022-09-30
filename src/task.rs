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

use crate::chompfile::ChompTaskMaybeTemplated;
use crate::chompfile::TaskDisplay;
use crate::chompfile::ValidationCheck;
use crate::chompfile::WatchInvalidation;
use crate::chompfile::{Chompfile, InvalidationCheck};
use crate::engines::CmdPool;
use crate::server::FileEvent;
use crate::ExtensionEnvironment;
use async_recursion::async_recursion;
use capturing_glob::{glob, Pattern};
use futures::future::Shared;
use futures::future::{select_all, Future, FutureExt};
use notify::DebouncedEvent;
use notify::RecommendedWatcher;
use pathdiff::diff_paths;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env::current_dir;
use std::fs::canonicalize;
use std::io::ErrorKind::NotFound;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
extern crate notify;
use crate::engines::replace_env_vars_static;
use crate::engines::ExecState;
use anyhow::{anyhow, Result};
use derivative::Derivative;
use futures::executor;
use notify::{watcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use tokio::fs;
use tokio::time;

#[derive(Debug)]
pub struct Task<'a> {
    name: Option<String>,
    targets: Vec<String>,
    deps: Vec<String>,
    env: BTreeMap<String, String>,
    chomp_task: &'a ChompTaskMaybeTemplated,
}

pub struct RunOptions {
    pub args: Option<Vec<String>>,
    pub cfg_file: PathBuf,
    pub pool_size: usize,
    pub targets: Vec<String>,
    pub watch: bool,
    pub rerun: bool,
    pub force: bool,
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
enum JobState {
    Sentinel,
    Uninitialized,
    Initialized,
    Checking,
    Pending,
    Running,
    Fresh,
    Failed,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct Job {
    interpolate: Option<String>,
    task: usize,
    deps: Vec<usize>,
    parents: Vec<usize>,
    live: bool,
    state: JobState,
    mtime: Option<Duration>,
    #[derivative(Debug = "ignore")]
    mtime_future: Option<Shared<Pin<Box<dyn Future<Output = Option<Duration>>>>>>,
    targets: Vec<String>,
    cmd_num: Option<usize>,
}

#[derive(Debug)]
enum Node {
    Job(Job),
    File(File),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
enum FileState {
    Uninitialized,
    Initialized,
    Checking,
    Found,
    NotFound,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct File {
    name: String,
    parents: Vec<usize>,
    state: FileState,
    mtime: Option<Duration>,
    #[derivative(Debug = "ignore")]
    mtime_future: Option<Shared<Pin<Box<dyn Future<Output = Option<Duration>>>>>>,
}

impl File {
    fn new(name: String) -> File {
        File {
            name,
            mtime: None,
            parents: Vec::new(),
            state: FileState::Uninitialized,
            mtime_future: None,
        }
    }

    fn init(&mut self, watcher: Option<&mut RecommendedWatcher>) {
        self.state = FileState::Initialized;
        if let Some(watcher) = watcher {
            match watcher.watch(&self.name, RecursiveMode::Recursive) {
                Ok(_) => {}
                Err(_) => {
                    // eprintln!("Unable to watch {}", self.name);
                }
            };
        }
    }
}

fn find_interpolate(s: &str) -> Result<Option<(usize, bool)>> {
    match s.find("##") {
        Some(idx) => {
            if s.find('#').unwrap() != idx || s[idx + 2..].find('#').is_some() {
                return Err(anyhow!("Multiple interpolates in '{}' not supported", s));
            }
            Ok(Some((idx, true)))
        }
        None => match s.find('#') {
            Some(idx) => {
                if s[idx + 1..].find('#').is_some() {
                    return Err(anyhow!("Multiple interpolates in '{}' not supported", s));
                }
                Ok(Some((idx, false)))
            }
            None => Ok(None),
        },
    }
}

fn get_interpolate_match(interpolate: &str, path: &str) -> String {
    let prefix_len = interpolate.find('#').unwrap();
    let suffix_len = interpolate.len() - interpolate.rfind('#').unwrap() - 1;
    path[prefix_len..path.len() - suffix_len].to_string()
}

fn check_interpolate_exclude(task: &Task, path: &str) -> bool {
    // If the interpolated dependency matches its own task's target glob space, then we exclude it
    // We can enable further custom ignores here in future
    if let Some(interpolation_target) = task.targets.iter().find(|&t| t.contains('#')) {
        let target_glob = if interpolation_target.contains("##") {
            interpolation_target.replace("##", "(**/*)")
        } else {
            interpolation_target.replace('#', "(*)")
        };
        if Pattern::new(&target_glob).unwrap().matches(&path) {
            return true;
        }
    }
    return false;
}

fn replace_interpolate(s: &str, replacement: &str) -> String {
    if let Some((_, double)) = find_interpolate(s).unwrap() {
        if double {
            s.replace("##", replacement)
        } else {
            s.replace('#', replacement)
        }
    } else {
        String::from(s)
    }
}

pub struct Runner<'a> {
    // ui: &'a ChompUI,
    cwd: String,
    cmd_pool: CmdPool<'a>,
    chompfile: &'a Chompfile,
    watch: bool,
    tasks: Vec<Task<'a>>,

    nodes: Vec<Node>,

    task_jobs: HashMap<String, usize>,
    file_nodes: HashMap<String, usize>,
    interpolate_nodes: Vec<usize>,
}

impl<'a> Job {
    fn new(task: usize, interpolate: Option<String>) -> Job {
        Job {
            interpolate,
            task,
            deps: Vec::new(),
            live: false,
            parents: Vec::new(),
            state: JobState::Uninitialized,
            targets: Vec::new(),
            mtime: None,
            cmd_num: None,
            mtime_future: None,
        }
    }

    fn display_name(&self, tasks: &Vec<Task<'a>>) -> String {
        let task = &tasks[self.task];
        if self.interpolate.is_some() {
            if task.targets.len() > 0 {
                match task.targets.iter().find(|&t| t.contains('#')) {
                    Some(interpolate_target) => {
                        replace_interpolate(interpolate_target, &self.interpolate.as_ref().unwrap())
                    }
                    None => replace_interpolate(
                        task.deps.iter().find(|&d| d.contains('#')).unwrap(),
                        &self.interpolate.as_ref().unwrap(),
                    ),
                }
            } else {
                replace_interpolate(
                    task.deps.iter().find(|&d| d.contains('#')).unwrap(),
                    &self.interpolate.as_ref().unwrap(),
                )
            }
        } else if self.targets.len() > 0 {
            self.targets.first().unwrap().to_string()
        } else if let Some(name) = &task.name {
            String::from(format!(":{}", name))
        } else if let Some(run) = &task.chomp_task.run {
            String::from(format!("{}", run))
        } else {
            String::from(format!("[task {}]", self.task))
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
enum JobOrFileState {
    Job(JobState),
    File(FileState),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct StateTransition {
    node_num: usize,
    cmd_num: Option<usize>,
    state: JobOrFileState,
}

impl StateTransition {
    fn from_job(node_num: usize, state: JobState, cmd_num: Option<usize>) -> Self {
        StateTransition {
            node_num,
            cmd_num,
            state: JobOrFileState::Job(state),
        }
    }
    fn from_file(node_num: usize, state: FileState, cmd_num: Option<usize>) -> Self {
        StateTransition {
            node_num,
            cmd_num,
            state: JobOrFileState::File(state),
        }
    }
}

#[derive(Debug)]
struct QueuedStateTransitions {
    state_transitions: HashSet<StateTransition>,
}

impl QueuedStateTransitions {
    fn new() -> Self {
        Self {
            state_transitions: HashSet::new(),
        }
    }
    fn insert_job(
        &mut self,
        node_num: usize,
        state: JobState,
        cmd_num: Option<usize>,
    ) -> Option<StateTransition> {
        let transition = StateTransition::from_job(node_num, state, cmd_num);
        if self.state_transitions.insert(transition.clone()) {
            Some(transition)
        } else {
            None
        }
    }
    fn insert_file(
        &mut self,
        node_num: usize,
        state: FileState,
        cmd_num: Option<usize>,
    ) -> Option<StateTransition> {
        let transition = StateTransition::from_file(node_num, state, cmd_num);
        if self.state_transitions.insert(transition.clone()) {
            Some(transition)
        } else {
            None
        }
    }
    fn remove_job(&mut self, node_num: usize, state: JobState, cmd_num: Option<usize>) -> bool {
        let transition = StateTransition::from_job(node_num, state, cmd_num);
        self.state_transitions.remove(&transition)
    }
}

// None = NotFound
pub async fn check_target_mtimes(targets: Vec<String>, default_latest: bool) -> Option<Duration> {
    if targets.len() == 0 {
        if default_latest {
            return Some(now());
        } else {
            return None;
        }
    }
    let mut futures = Vec::new();
    for target in &targets {
        let target_path = Path::new(target);
        futures.push(
            async move {
                match fs::metadata(target_path).await {
                    Ok(n) => Some(
                        n.modified()
                            .expect("No modified implementation")
                            .duration_since(UNIX_EPOCH)
                            .unwrap(),
                    ),
                    Err(e) => match e.kind() {
                        NotFound => None,
                        _ => panic!("Unknown file error"),
                    },
                }
            }
            .boxed_local(),
        );
    }
    let mut has_missing = false;
    let mut last_mtime = None;
    while futures.len() > 0 {
        let (mtime, _, new_futures) = select_all(futures).await;
        futures = new_futures;
        if mtime.is_none() {
            has_missing = true;
            last_mtime = None;
        } else if !has_missing && mtime > last_mtime {
            last_mtime = mtime;
        }
    }
    last_mtime
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('(') || s.contains('[') || s.contains('?') || s.contains('*')
}

fn now() -> std::time::Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
}

// On Windows, we need to explicitly redefine wanted system-defined
// env vars since these are specifically promoted to local variables
// for the powershell exec
#[cfg(target_os = "windows")]
fn create_task_env<'a>(
    task: &ChompTaskMaybeTemplated,
    chompfile: &'a Chompfile,
    replacements: bool,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for (item, value) in &chompfile.env {
        env.insert(
            item.to_uppercase(),
            if replacements {
                replace_env_vars_static(value, &env)
            } else {
                value.to_string()
            },
        );
    }
    for (item, value) in &chompfile.env_default {
        if !env.contains_key(item) {
            if let Some(val) = std::env::var_os(item) {
                env.insert(item.to_uppercase(), String::from(val.to_str().unwrap()));
            } else {
                env.insert(
                    item.to_uppercase(),
                    if replacements {
                        replace_env_vars_static(value, &env)
                    } else {
                        value.to_string()
                    },
                );
            }
        }
    }
    if let Some(ref task_env) = task.env {
        for (item, value) in task_env {
            env.insert(
                item.to_uppercase(),
                if replacements {
                    replace_env_vars_static(value, &env)
                } else {
                    value.to_string()
                },
            );
        }
    }
    if let Some(ref task_env_default) = task.env_default {
        for (item, value) in task_env_default {
            if !env.contains_key(item) {
                if let Some(val) = std::env::var_os(item) {
                    env.insert(item.to_uppercase(), String::from(val.to_str().unwrap()));
                } else {
                    env.insert(
                        item.to_uppercase(),
                        if replacements {
                            replace_env_vars_static(value, &env)
                        } else {
                            value.to_string()
                        },
                    );
                }
            }
        }
    }
    env
}

#[cfg(not(target_os = "windows"))]
fn create_task_env<'a>(
    task: &ChompTaskMaybeTemplated,
    chompfile: &'a Chompfile,
    replacements: bool,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for (item, value) in &chompfile.env {
        env.insert(
            item.to_uppercase(),
            if replacements {
                replace_env_vars_static(value, &env)
            } else {
                value.to_string()
            },
        );
    }
    for (item, value) in &chompfile.env_default {
        if !env.contains_key(item) && std::env::var_os(item).is_none() {
            env.insert(
                item.to_uppercase(),
                if replacements {
                    replace_env_vars_static(value, &env)
                } else {
                    value.to_string()
                },
            );
        }
    }
    if let Some(ref task_env) = task.env {
        for (item, value) in task_env {
            env.insert(
                item.to_uppercase(),
                if replacements {
                    replace_env_vars_static(value, &env)
                } else {
                    value.to_string()
                },
            );
        }
    }
    if let Some(ref task_env_default) = task.env_default {
        for (item, value) in task_env_default {
            if !env.contains_key(item) && std::env::var_os(item).is_none() {
                env.insert(
                    item.to_uppercase(),
                    if replacements {
                        replace_env_vars_static(value, &env)
                    } else {
                        value.to_string()
                    },
                );
            }
        }
    }
    env
}

impl<'a> Runner<'a> {
    pub fn new(
        // ui: &'a ChompUI,
        chompfile: &'a Chompfile,
        extension_env: &'a mut ExtensionEnvironment,
        pool_size: usize,
        watch: bool,
    ) -> Result<Runner<'a>> {
        let cwd_buf = current_dir()?;
        let cwd = cwd_buf.to_str().unwrap();

        let cmd_pool: CmdPool = CmdPool::new(pool_size, String::from(cwd), extension_env);
        let mut runner = Runner {
            watch,
            // ui,
            cwd: String::from(cwd),
            cmd_pool,
            chompfile,
            nodes: Vec::new(),
            tasks: Vec::new(),
            task_jobs: HashMap::new(),
            file_nodes: HashMap::new(),
            interpolate_nodes: Vec::new(),
        };

        for task in &runner.chompfile.task {
            let targets = task.targets_vec()?;
            let deps = task.deps_vec(&chompfile)?;
            let env = create_task_env(&task, &chompfile, task.env_replace.unwrap_or(true));
            let task = Task {
                name: task.name.clone(),
                targets,
                deps,
                chomp_task: &task,
                env,
            };

            runner.tasks.push(task);
            runner.add_job(runner.tasks.len() - 1, None)?;
        }

        Ok(runner)
    }

    fn add_job(&mut self, task_num: usize, interpolate: Option<String>) -> Result<(usize, bool)> {
        let num: usize = self.nodes.len();
        let task = &self.tasks[task_num];

        let is_interpolate_target = task.deps.iter().find(|&d| d.contains('#')).is_some();

        // map target name
        if let Some(ref name) = task.name {
            if interpolate.is_none() {
                let name = if is_interpolate_target && name.contains('#') {
                    // interpolates support "#" in the name as well
                    // which is treated as blank for the all case
                    replace_interpolate(name, "")
                } else {
                    name.to_string()
                };
                if !self.task_jobs.contains_key(&name) {
                    self.task_jobs.insert(name, num);
                }
            } else if name.contains('#') {
                // interpolate individual names only expanded when using "#" in the name
                let name = replace_interpolate(name, interpolate.as_ref().unwrap());
                if !self.task_jobs.contains_key(&name) {
                    self.task_jobs.insert(name, num);
                }
            }
        }

        // map interpolation for primary interpolation job
        if is_interpolate_target && interpolate.is_none() {
            self.interpolate_nodes.push(num);
        }

        let mut job = Job::new(task_num, interpolate.clone());

        // map target file as file node
        let task_targets = task.targets.clone();
        if !is_interpolate_target || interpolate.is_some() {
            for target in task_targets.iter() {
                let file_target = match &interpolate {
                    Some(interpolate) => {
                        if !target.contains('#') {
                            continue;
                        }
                        replace_interpolate(target, interpolate)
                    }
                    None => target.to_string(),
                };
                match self.file_nodes.get(&file_target) {
                    Some(&target_num) => {
                        if self.nodes.get(target_num).is_none() {
                            self.nodes.push(Node::Job(job));
                            return Ok((num, true));
                        }

                        match &self.nodes[target_num] {
                            Node::Job(_) => {
                                // duplicate job for same file -> first wins (skip)
                                return Ok((target_num, false));
                            }
                            Node::File(file) => {
                                // replacing previous file node with interpolate job node -> upgrade the attachments
                                self.file_nodes.insert(file_target, target_num);
                                let parents = file.parents.clone();
                                for parent in parents {
                                    let parent_job = self.get_job_mut(parent).unwrap();
                                    let idx = parent_job
                                        .deps
                                        .iter()
                                        .enumerate()
                                        .find(|(_, &d)| d == target_num)
                                        .unwrap()
                                        .0;
                                    parent_job.deps[idx] = target_num;
                                    job.parents.push(parent);
                                }
                            }
                        }
                    }
                    None => {
                        self.file_nodes.insert(file_target, num);
                    }
                }
            }
        }

        self.nodes.push(Node::Job(job));
        return Ok((num, true));
    }

    fn add_file(&mut self, file: String) -> Result<usize> {
        let file2 = file.to_string();
        Ok(match self.file_nodes.get(&file2) {
            Some(&num) => num,
            None => {
                let num = self.nodes.len();
                self.nodes.push(Node::File(File::new(file)));
                self.file_nodes.insert(file2, num);
                num
            }
        })
    }

    #[inline]
    fn get_job(&self, num: usize) -> Option<&Job> {
        match self.nodes[num] {
            Node::Job(ref job) => Some(job),
            _ => None,
        }
    }

    #[inline]
    fn get_job_mut(&mut self, num: usize) -> Option<&mut Job> {
        match self.nodes[num] {
            Node::Job(ref mut job) => Some(job),
            _ => None,
        }
    }

    #[inline]
    fn get_file_mut(&mut self, num: usize) -> Option<&mut File> {
        match self.nodes[num] {
            Node::File(ref mut file) => Some(file),
            _ => None,
        }
    }

    fn mark_complete(
        &mut self,
        job_num: usize,
        mtime: Option<Duration>,
        cmd_time: Option<Duration>,
        failed: bool,
    ) {
        {
            let job = self.get_job_mut(job_num).unwrap();
            if let Some(mtime) = mtime {
                job.mtime = Some(mtime);
            }
            job.state = if failed {
                JobState::Failed
            } else {
                JobState::Fresh
            };
        }
        let job = self.get_job(job_num).unwrap();
        let task = &self.tasks[job.task];
        if failed
            || matches!(
                task.chomp_task.display,
                Some(TaskDisplay::InitStatus)
                    | Some(TaskDisplay::StatusOnly)
                    | Some(TaskDisplay::Dot)
                    | None
            )
            || self.chompfile.echo
        {
            let mut name = job.display_name(&self.tasks);
            let primary = job.parents.len() == 0;
            if primary {
                let mut name_bold = String::from("\x1b[1m");
                name_bold.push_str(&name);
                name_bold.push_str("\x1b[0m");
                name = name_bold;
            }
            if matches!(task.chomp_task.display, Some(TaskDisplay::Dot)) {
                if failed {
                    print!("\x1b[1;31m.\x1b[0m");
                } else if mtime.is_some() || cmd_time.is_some() {
                    print!("\x1b[1;32m.\x1b[0m");
                } else {
                    print!("\x1b[1m●\x1b[0m");
                }
                std::io::stdout().flush().unwrap();
            } else if let Some(cmd_time) = cmd_time {
                name = relative_path(&name, &self.cwd);
                if failed {
                    println!(
                        "\x1b[1;31mx\x1b[0m {} \x1b[34m[{:?}]\x1b[0m",
                        name, cmd_time
                    );
                } else {
                    println!(
                        "\x1b[1;32m√\x1b[0m {} \x1b[34m[{:?}]\x1b[0m",
                        name, cmd_time
                    );
                }
            } else {
                name = relative_path(&name, &self.cwd);
                if failed {
                    println!("\x1b[1;31mx\x1b[0m {}", name);
                } else if mtime.is_some() {
                    println!("\x1b[1;32m√\x1b[0m {}", name);
                } else if task.deps.len() == 0 {
                    println!("\x1b[1m●\x1b[0m {} \x1b[34m[exists]\x1b[0m", name);
                } else {
                    println!("\x1b[1m●\x1b[0m {} \x1b[34m[cached]\x1b[0m", name);
                }
            }
        }
        {
            let job = self.get_job_mut(job_num).unwrap();
            job.cmd_num = None;
        }
    }

    fn invalidate_job(
        &mut self,
        job_num: usize,
        queued: &mut QueuedStateTransitions,
        redrives: &mut HashSet<usize>,
    ) -> Result<()> {
        let job = self.get_job(job_num).unwrap();
        let job = match job.state {
            JobState::Failed | JobState::Fresh => {
                let task = &self.tasks[job.task];
                if matches!(
                    task.chomp_task.watch_invalidation,
                    Some(WatchInvalidation::SkipRunning)
                ) {
                    if let Some(mtime) = job.mtime {
                        if mtime > now() - Duration::from_secs(1) {
                            return Ok(());
                        }
                    }
                }
                let job = self.get_job_mut(job_num).unwrap();
                job.state = JobState::Pending;
                job
            }
            JobState::Running => {
                if let Some(cmd_num) = job.cmd_num {
                    // Could possibly consider a JobState::MaybeTerminate
                    // as a kind of Pending analog which may or may not rerun
                    queued.remove_job(job_num, JobState::Running, Some(cmd_num));
                    let display_name = job.display_name(&self.tasks);
                    let task = &self.tasks[job.task];
                    if matches!(
                        task.chomp_task.watch_invalidation,
                        Some(WatchInvalidation::SkipRunning)
                    ) {
                        let job = self.get_job_mut(job_num).unwrap();
                        job.state = JobState::Fresh;
                        return Ok(());
                    }
                    self.cmd_pool.terminate(cmd_num, &display_name);
                }
                let job = self.get_job_mut(job_num).unwrap();
                job.mtime = Some(now() - Duration::from_secs(1));
                job.state = JobState::Pending;
                job
            }
            _ => self.get_job_mut(job_num).unwrap(),
        };
        if job.parents.len() > 0 {
            for parent in job.parents.clone() {
                if parent == job_num {
                    continue;
                }
                self.invalidate_job(parent, queued, redrives)?;
            }
        }
        redrives.insert(job_num);
        Ok(())
    }

    fn invalidate_path(
        &mut self,
        path: PathBuf,
        queued: &mut QueuedStateTransitions,
        redrives: &mut HashSet<usize>,
    ) -> Result<bool> {
        let path_str = path.to_str().unwrap();
        if !path_str.starts_with(&self.cwd) {
            return Err(anyhow!("Expected path within cwd"));
        }
        let rel_str = &path_str[self.cwd.len() + 1..];
        let sanitized_path = rel_str.replace("\\", "/");
        match self.file_nodes.get(&sanitized_path) {
            Some(&node_num) => match self.nodes[node_num] {
                Node::Job(_) => {
                    self.invalidate_job(node_num, queued, redrives)?;
                    Ok(true)
                }
                Node::File(ref mut file) => {
                    file.mtime = Some(now());
                    for parent in file.parents.clone() {
                        self.invalidate_job(parent, queued, redrives)?;
                    }
                    Ok(true)
                }
            },
            None => Ok(false),
        }
    }

    fn expand_job_deps(&self, job_num: usize, deps: &mut Vec<String>) {
        let job = self.get_job(job_num).unwrap();
        for &dep in job.deps.iter() {
            match &self.nodes[dep] {
                Node::Job(job) => {
                    if job.interpolate.is_none() {
                        let task = &self.tasks[job.task];
                        let has_interpolation =
                            task.deps.iter().find(|&d| d.contains('#')).is_some();
                        if has_interpolation {
                            self.expand_job_deps(dep, deps);
                        }
                    }
                    for target in job.targets.iter() {
                        if !deps.iter().find(|&dep| dep == target).is_some() {
                            deps.push(target.to_string());
                        }
                    }
                }
                Node::File(file) => {
                    let name = &file.name;
                    if !deps.iter().find(|&dep| dep == name).is_some() {
                        deps.push(name.to_string());
                    }
                }
            };
        }
    }

    fn run_job(
        &mut self,
        job_num: usize,
        force: bool,
    ) -> Option<(usize, Pin<Box<dyn Future<Output = StateTransition> + 'a>>)> {
        let job = self.get_job(job_num).unwrap();
        if job.state != JobState::Pending {
            panic!("Expected pending job");
        }
        let task = &self.tasks[job.task];
        // CMD Exec
        if task.chomp_task.run.is_none() {
            self.mark_complete(job_num, Some(now()), None, false);
            return None;
        }
        // the interpolation template itself is not run
        if job.interpolate.is_none() {
            let has_interpolation = task.deps.iter().find(|&d| d.contains('#')).is_some();
            if has_interpolation {
                self.mark_complete(job_num, Some(now()), None, false);
                return None;
            }
        }
        // If we have an mtime, check if we need to do work
        if let Some(mtime) = job.mtime {
            let can_skip = task.chomp_task.args.is_none()
                && match task.chomp_task.invalidation.unwrap_or_default() {
                    InvalidationCheck::NotFound => true,
                    InvalidationCheck::Always => {
                        if !force
                            && (matches!(
                                task.chomp_task.display,
                                Some(TaskDisplay::InitStatus) | Some(TaskDisplay::InitOnly) | None
                            ) || self.chompfile.echo)
                        {
                            println!(
                                "  \x1b[1m{}\x1b[0m invalidated",
                                job.display_name(&self.tasks),
                            );
                        }
                        false
                    }
                    InvalidationCheck::Mtime => {
                        if force {
                            false
                        } else {
                            let mut dep_change = false;
                            for &dep in job.deps.iter() {
                                dep_change = match &self.nodes[dep] {
                                    Node::Job(dep) => {
                                        let invalidated = match &self.tasks[dep.task]
                                            .chomp_task
                                            .invalidation
                                            .unwrap_or_default()
                                        {
                                            InvalidationCheck::NotFound
                                            | InvalidationCheck::Always
                                            | InvalidationCheck::Mtime => match dep.mtime {
                                                Some(dep_mtime) => dep_mtime > mtime,
                                                None => true,
                                            },
                                        };
                                        if invalidated
                                            && (matches!(
                                                task.chomp_task.display,
                                                Some(TaskDisplay::InitStatus)
                                                    | Some(TaskDisplay::InitOnly)
                                                    | None
                                            ) || self.chompfile.echo)
                                        {
                                            println!(
                                                "  \x1b[1m{}\x1b[0m invalidated by {}",
                                                job.display_name(&self.tasks),
                                                dep.display_name(&self.tasks)
                                            );
                                        }
                                        invalidated
                                    }
                                    Node::File(dep) => {
                                        let invalidated = match dep.mtime {
                                            Some(dep_mtime) => dep_mtime > mtime,
                                            None => true,
                                        };
                                        if invalidated
                                            && (matches!(
                                                task.chomp_task.display,
                                                Some(TaskDisplay::InitStatus)
                                                    | Some(TaskDisplay::InitOnly)
                                                    | None
                                            ) || self.chompfile.echo)
                                        {
                                            println!(
                                                "  \x1b[1m{}\x1b[0m invalidated by {}",
                                                job.display_name(&self.tasks),
                                                dep.name
                                            );
                                        }
                                        invalidated
                                    }
                                };
                                if dep_change {
                                    break;
                                }
                            }
                            !dep_change
                        }
                    }
                };
            if can_skip {
                self.mark_complete(job_num, None, None, false);
                return None;
            }
        }

        let run = task.chomp_task.run.as_ref().unwrap();
        let mut env = task.env.clone();
        if let Some(interpolate) = &job.interpolate {
            env.insert("MATCH".to_string(), interpolate.to_string());
        }
        let target_index = if job.interpolate.is_some() {
            match task
                .targets
                .iter()
                .enumerate()
                .find(|(_, d)| d.contains('#'))
            {
                Some(mtch) => mtch.0,
                None => 0,
            }
        } else {
            0
        };
        let target = if task.targets.len() == 0 {
            "".to_string()
        } else if let Some(interpolate) = &job.interpolate {
            replace_interpolate(&task.targets[target_index], interpolate)
        } else {
            task.targets[target_index].clone()
        };

        let mut targets = String::new();
        for (idx, t) in task.targets.iter().enumerate() {
            if idx > 0 {
                targets.push_str(":");
            }
            if idx == target_index {
                targets.push_str(&target);
            } else {
                targets.push_str(t);
            }
        }

        let mut deps: Vec<String> = if let Some(ref interpolate) = job.interpolate {
            let interpolate_index = task
                .deps
                .iter()
                .enumerate()
                .find(|(_, d)| d.contains('#'))
                .unwrap()
                .0;
            vec![replace_interpolate(
                &task.deps[interpolate_index],
                interpolate,
            )]
        } else {
            vec![]
        };

        self.expand_job_deps(job_num, &mut deps);

        // relative target for backward compatibility
        let relative_target = if !target.is_empty() {
            relative_path(&target, &self.cwd)
        } else {
            "".to_string()
        };
        env.insert("TARGET".to_string(), relative_target.to_owned());

        let relative_targets = if !targets.is_empty() {
            relative_path(&targets, &self.cwd)
        } else {
            "".to_string()
        };
        env.insert("TARGETS".to_string(), relative_targets);

        let first_dep = deps.get(0);
        // relative dep for backward compatibility
        let relative_dep = if first_dep.is_some() {
            relative_path(&first_dep.unwrap(), &self.cwd)
        } else {
            "".to_string()
        };
        env.insert("DEP".to_string(), relative_dep);

        let relative_deps = deps.iter().map(|d| {
            relative_path(d, &self.cwd)
        }).collect::<Vec<String>>();
        env.insert("DEPS".to_string(), relative_deps.join(":"));

        if task.chomp_task.args.is_some() {
            for arg in task.chomp_task.args.as_ref().unwrap() {
                let k = arg.to_uppercase();
                if !env.contains_key(&k) {
                    env.insert(k, String::from(""));
                }
            }
        }

        let targets = job.targets.clone();
        let engine = task.chomp_task.engine.unwrap_or_default();
        let env_replace = task.chomp_task.env_replace.unwrap_or(true);
        let echo = if let Some(echo) = task.chomp_task.echo {
            echo
        } else {
            self.chompfile.echo
        };
        let cmd_num = {
            let stdio = task.chomp_task.stdio.unwrap_or_default();
            let display_name = if matches!(
                task.chomp_task.display,
                Some(TaskDisplay::InitStatus) | Some(TaskDisplay::InitOnly) | None
            ) || echo
            {
                Some(job.display_name(&self.tasks))
            } else {
                None
            };
            let cwd = match &task.chomp_task.cwd {
                Some(cwd) => {
                    let cwd_path = PathBuf::from(cwd);
                    let cwd = if Path::is_absolute(&cwd_path) {
                        cwd_path
                    } else {
                        let mut base = PathBuf::from(&self.cwd);
                        base.push(&cwd_path);
                        base
                    };
                    Some(match canonicalize(&cwd) {
                        Ok(cwd) => {
                            let cwd = cwd.to_str().unwrap();
                            if cwd.starts_with(r"\\?\") {
                                String::from(&cwd[4..])
                            } else {
                                cwd.to_string()
                            }
                        }
                        Err(_) => {
                            panic!("Unable to resolve task CWD {}", &cwd.to_str().unwrap());
                        }
                    })
                }
                None => None,
            };
            let cmd_num = self.cmd_pool.batch(
                display_name,
                run,
                targets,
                env,
                env_replace,
                cwd,
                engine,
                stdio,
                echo,
            );
            let job = self.get_job_mut(job_num).unwrap();
            job.state = JobState::Running;
            job.cmd_num = Some(cmd_num);
            cmd_num
        };
        let exec_future = self.cmd_pool.get_exec_future(cmd_num);
        Some((
            cmd_num,
            async move {
                match exec_future.await {
                    _ => {}
                };
                StateTransition::from_job(job_num, JobState::Running, Some(cmd_num))
            }
            .boxed_local(),
        ))
    }

    // top-down driver - initiates future starts
    fn drive_all(
        &mut self,
        job_num: usize,
        force: bool,
        futures: &mut Vec<Pin<Box<dyn Future<Output = StateTransition> + 'a>>>,
        queued: &mut QueuedStateTransitions,
        parent: Option<usize>,
        watch_listener: UnboundedSender<DebouncedEvent>,
    ) -> Result<JobOrFileState> {
        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if let Some(parent) = parent {
                    if job.parents.iter().find(|&&p| p == parent).is_none() {
                        job.parents.push(parent);
                    }
                }
                if parent.is_none() {
                    if !job.live {
                        return Ok(JobOrFileState::Job(job.state));
                    }
                } else {
                    job.live = true;
                }
                match job.state {
                    JobState::Sentinel | JobState::Uninitialized => {
                        panic!("Unexpected uninitialized job {}", job_num);
                    }
                    JobState::Initialized => {
                        let targets = job.targets.clone();
                        let mtime_future = async { check_target_mtimes(targets, false).await }
                            .boxed_local()
                            .shared();
                        job.mtime_future = Some(mtime_future.clone());
                        job.state = JobState::Checking;
                        let transition = queued
                            .insert_job(job_num, JobState::Checking, None)
                            .expect("Expected first job check");
                        futures.push(
                            async move {
                                mtime_future.await;
                                transition
                            }
                            .boxed_local(),
                        );
                        Ok(JobOrFileState::Job(JobState::Checking))
                    }
                    JobState::Checking => {
                        let job = self.get_job(job_num).unwrap();
                        if let Some(transition) =
                            queued.insert_job(job_num, JobState::Checking, None)
                        {
                            let mtime_future = job.mtime_future.as_ref().unwrap().clone();
                            futures.push(
                                async move {
                                    mtime_future.await;
                                    transition
                                }
                                .boxed_local(),
                            );
                        }
                        Ok(JobOrFileState::Job(JobState::Checking))
                    }
                    JobState::Pending => {
                        let mut all_completed = true;
                        let job = self.get_job(job_num).unwrap();
                        let serial = self.tasks[job.task].chomp_task.serial.unwrap_or_default();
                        let deps = job.deps.clone();

                        for dep in deps {
                            // permit self-builds, arbitrary cycles will stall still though
                            if dep == job_num {
                                continue;
                            }
                            let dep_state = self.drive_all(
                                dep,
                                force,
                                futures,
                                queued,
                                Some(job_num),
                                watch_listener.clone(),
                            )?;
                            match dep_state {
                                JobOrFileState::Job(JobState::Fresh)
                                | JobOrFileState::File(FileState::Found) => {}
                                JobOrFileState::Job(JobState::Failed)
                                | JobOrFileState::File(FileState::NotFound) => {
                                    self.mark_complete(job_num, None, None, true);
                                    let transition = queued
                                        .insert_job(job_num, JobState::Running, None)
                                        .unwrap();
                                    self.drive_completion(
                                        transition,
                                        force,
                                        futures,
                                        queued,
                                        watch_listener.clone(),
                                    )?;
                                    return Ok(JobOrFileState::Job(JobState::Failed));
                                }
                                _ => {
                                    // Serial only proceeds on a completion result
                                    if serial {
                                        return Ok(JobOrFileState::Job(JobState::Pending));
                                    }
                                    all_completed = false;
                                }
                            }
                        }

                        // we could have driven this job to completion already...
                        let job = self.get_job(job_num).unwrap();
                        if job.state != JobState::Pending {
                            return Ok(JobOrFileState::Job(job.state));
                        }

                        // deps all completed -> execute this job
                        if all_completed {
                            return match self.run_job(job_num, force) {
                                Some((cmd_num, future)) => {
                                    match queued.insert_job(
                                        job_num,
                                        JobState::Running,
                                        Some(cmd_num),
                                    ) {
                                        Some(_) => futures.push(future),
                                        None => {}
                                    };
                                    Ok(JobOrFileState::Job(JobState::Running))
                                }
                                None => {
                                    let transition = queued
                                        .insert_job(job_num, JobState::Running, None)
                                        .unwrap();
                                    self.drive_completion(
                                        transition,
                                        force,
                                        futures,
                                        queued,
                                        watch_listener,
                                    )?;
                                    Ok(JobOrFileState::Job(JobState::Fresh))
                                }
                            };
                        }
                        Ok(JobOrFileState::Job(JobState::Pending))
                    }
                    JobState::Running => {
                        let job = self.get_job(job_num).unwrap();
                        let cmd_num = job.cmd_num.unwrap();
                        if let Some(transition) =
                            queued.insert_job(job_num, JobState::Running, Some(cmd_num))
                        {
                            let future = self.cmd_pool.get_exec_future(cmd_num);
                            futures.push(
                                async move {
                                    match future.await {
                                        _ => {}
                                    };
                                    transition
                                }
                                .boxed_local(),
                            );
                        }
                        Ok(JobOrFileState::Job(JobState::Running))
                    }
                    JobState::Failed => Ok(JobOrFileState::Job(JobState::Failed)),
                    JobState::Fresh => Ok(JobOrFileState::Job(JobState::Fresh)),
                }
            }
            Node::File(ref mut file) => {
                if let Some(parent) = parent {
                    if file.parents.iter().find(|&&p| p == parent).is_none() {
                        file.parents.push(parent);
                    }
                }
                match file.state {
                    FileState::Uninitialized => panic!("Unexpected file state"),
                    FileState::Initialized => {
                        let name = file.name.to_string();
                        let mtime_future = async move {
                            match fs::metadata(&name).await {
                                Ok(n) => {
                                    let mtime = n.modified().expect("No modified implementation");
                                    Some(mtime.duration_since(UNIX_EPOCH).unwrap())
                                }
                                Err(e) => match e.kind() {
                                    NotFound => None,
                                    _ => panic!("Unknown file error for '{}': {:?}", &name, e),
                                },
                            }
                        }
                        .boxed_local()
                        .shared();
                        file.mtime_future = Some(mtime_future.clone());
                        file.state = FileState::Checking;
                        let transition = queued
                            .insert_file(job_num, FileState::Checking, None)
                            .expect("Expected first file check");
                        futures.push(
                            async move {
                                mtime_future.await;
                                transition
                            }
                            .boxed_local(),
                        );
                        Ok(JobOrFileState::File(FileState::Checking))
                    }
                    FileState::Checking => {
                        if let Some(transition) =
                            queued.insert_file(job_num, FileState::Checking, None)
                        {
                            let future = file.mtime_future.as_ref().unwrap().clone();
                            futures.push(
                                async move {
                                    future.await;
                                    transition
                                }
                                .boxed_local(),
                            );
                        }
                        Ok(JobOrFileState::File(FileState::Checking))
                    }
                    FileState::Found => Ok(JobOrFileState::File(FileState::Found)),
                    FileState::NotFound => {
                        if !self.watch {
                            return Err(anyhow!("File {} not found", file.name));
                        } else {
                            panic!("Watching files not yet created is not yet supported, in depending on {}. This should be supported, please post an issue on GitHub!", file.name);
                        }
                    }
                }
            }
        }
    }

    // bottom-up completer - initiates active deferred future starts
    fn drive_completion(
        &mut self,
        transition: StateTransition,
        force: bool,
        futures: &mut Vec<Pin<Box<dyn Future<Output = StateTransition> + 'a>>>,
        queued: &mut QueuedStateTransitions,
        watch_listener: UnboundedSender<DebouncedEvent>,
    ) -> Result<()> {
        if !queued.state_transitions.remove(&transition) {
            return Ok(());
        }
        // drives the completion of a state transition to subsequent transitions
        let node_num = transition.node_num;
        match transition.state {
            JobOrFileState::Job(JobState::Checking) => {
                let job = self.get_job_mut(node_num).unwrap();
                job.state = JobState::Pending;
                let mtime_future = job.mtime_future.take().unwrap();
                // we know it's ready so this isn't blocking
                let mtime = executor::block_on(mtime_future);
                job.mtime = mtime;
                job.mtime_future = None;
                self.drive_all(node_num, force, futures, queued, None, watch_listener)?;
                Ok(())
            }
            JobOrFileState::Job(JobState::Running) => {
                // job can complete running without an exec if eg cached
                let job = self.get_job(node_num).unwrap();
                let validation = self.tasks[job.task]
                    .chomp_task
                    .validation
                    .unwrap_or_default();
                if let Some(cmd_num) = job.cmd_num {
                    let exec_future = self.cmd_pool.get_exec_future(cmd_num);
                    let (status, mtime, cmd_time) = match executor::block_on(exec_future) {
                        Ok(result) => result,
                        Err(err) => return Err(anyhow!("Exec error: {:?}", err)),
                    };
                    match status {
                        ExecState::Completed => {
                            let job = self.get_job(node_num).unwrap();
                            for target in &job.targets {
                                let mut path = PathBuf::from(&self.cwd);
                                #[cfg(not(target_os = "windows"))]
                                path.push(&target);
                                #[cfg(target_os = "windows")]
                                path.push(target.replace('/', "\\"));
                                let evt = DebouncedEvent::Write(path);
                                watch_listener
                                    .send(evt)
                                    .expect("Unable to send watcher event to server channel");
                            }
                            self.mark_complete(
                                node_num,
                                mtime,
                                Some(cmd_time),
                                matches!(validation, ValidationCheck::NotOk)
                                    || matches!(
                                        validation,
                                        ValidationCheck::TargetsOnly | ValidationCheck::OkTargets
                                    ) && mtime.is_none(),
                            );
                        }
                        ExecState::Failed => match validation {
                            ValidationCheck::NotOk => {
                                self.mark_complete(node_num, mtime, Some(cmd_time), false)
                            }
                            ValidationCheck::OkOnly | ValidationCheck::OkTargets => {
                                self.mark_complete(node_num, mtime, Some(cmd_time), true)
                            }
                            ValidationCheck::None | ValidationCheck::TargetsOnly => self
                                .mark_complete(
                                    node_num,
                                    mtime,
                                    Some(cmd_time),
                                    matches!(validation, ValidationCheck::TargetsOnly)
                                        && mtime.is_none(),
                                ),
                        },
                        ExecState::Terminated => return Ok(()),
                        _ => panic!("Unexpected promise exec state"),
                    };
                }
                let job = self.get_job(node_num).unwrap();
                if matches!(job.state, JobState::Fresh | JobState::Failed) {
                    for parent in job.parents.clone() {
                        self.drive_all(
                            parent,
                            force,
                            futures,
                            queued,
                            None,
                            watch_listener.clone(),
                        )?;
                    }
                }
                Ok(())
            }
            JobOrFileState::File(FileState::Checking) => {
                let file = match self.nodes[node_num] {
                    Node::File(ref mut file) => file,
                    _ => panic!("Expected file"),
                };
                let mtime_future = file.mtime_future.take().unwrap();
                // we know it's ready so this isn't blocking
                let mtime = executor::block_on(mtime_future);
                file.mtime = mtime;
                file.state = match file.mtime {
                    Some(_mtime) => FileState::Found,
                    None => FileState::NotFound,
                };
                for parent in file.parents.clone() {
                    self.drive_all(parent, force, futures, queued, None, watch_listener.clone())?;
                }
                Ok(())
            }
            _ => panic!("Unexpected promise transition state"),
        }
    }

    fn lookup_task(&mut self, task: usize) -> Option<usize> {
        for (id, node) in self.nodes.iter().enumerate() {
            let job = match node {
                Node::File(_) => continue,
                Node::Job(job) => job,
            };
            // find the job for the task or interpolation task parent
            if job.task != task || job.interpolate.is_some() {
                continue;
            }
            return Some(id);
        }
        None
    }

    async fn lookup_task_name(
        &mut self,
        watcher: &mut RecommendedWatcher,
        task: &str,
    ) -> Result<Option<usize>> {
        match self.task_jobs.get(task) {
            Some(&job_num) => Ok(Some(job_num)),
            None => {
                // Check for interpolated task names
                let mut interpolate_match = None;
                let mut interpolate_lhs_match_len = 0;
                let mut interpolate_rhs_match_len = 0;
                for job_num in &self.interpolate_nodes {
                    let job = self.get_job(*job_num).unwrap();
                    let job_task = &self.tasks[job.task];
                    if let Some(name) = &job_task.name {
                        if let Some((interpolate_idx, double)) = find_interpolate(name)? {
                            let lhs = &name[0..interpolate_idx];
                            let rhs = &name[interpolate_idx + if double { 2 } else { 1 }..];
                            if task.starts_with(lhs)
                                && task.len() > lhs.len() + rhs.len()
                                && task.ends_with(rhs)
                            {
                                interpolate_match = Some((
                                    *job_num,
                                    &task[interpolate_idx..task.len() - rhs.len()],
                                ));
                                if lhs.len() >= interpolate_lhs_match_len
                                    && rhs.len() > interpolate_rhs_match_len
                                {
                                    interpolate_lhs_match_len = lhs.len();
                                    interpolate_rhs_match_len = rhs.len();
                                }
                            }
                        }
                    }
                }
                match interpolate_match {
                    Some((job_num, interpolate)) => {
                        let task_deps = &self.tasks[self.get_job(job_num).unwrap().task].deps;
                        let input = replace_interpolate(
                            task_deps.iter().find(|dep| dep.contains("#")).unwrap(),
                            interpolate,
                        );
                        let num = self
                            .expand_interpolate_match(
                                watcher,
                                &input,
                                interpolate,
                                job_num,
                                self.get_job(job_num).unwrap().task,
                            )
                            .await?;
                        Ok(Some(num))
                    }
                    None => Ok(None),
                }
            }
        }
    }

    fn get_interpolate_target(&self, interpolate_job: usize) -> Option<&String> {
        self.tasks[self.get_job(interpolate_job).unwrap().task]
            .targets
            .iter()
            .find(|&target| target.contains('#'))
    }

    #[async_recursion(?Send)]
    async fn lookup_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        glob_files: bool,
    ) -> Result<usize> {
        // First match task by name
        if target.as_bytes()[0] as char == ':' {
            return match self.lookup_task_name(watcher, &target[1..]).await? {
                Some(job_num) => Ok(job_num),
                None => return Err(anyhow!("No {} task found.", target)),
            };
        }

        // Match by exact file name
        match self.file_nodes.get(target) {
            Some(&job_num) => Ok(job_num),
            // Then by interpolate
            None => {
                let mut interpolate_match = None;
                let mut interpolate_lhs_match_len = 0;
                let mut interpolate_rhs_match_len = 0;
                for job_num in &self.interpolate_nodes {
                    if let Some(interpolate) = self.get_interpolate_target(*job_num) {
                        let (interpolate_idx, double) = find_interpolate(interpolate)?.unwrap();
                        let lhs = &interpolate[0..interpolate_idx];
                        let rhs = &interpolate[interpolate_idx + if double { 2 } else { 1 }..];
                        if target.starts_with(lhs)
                            && target.len() > lhs.len() + rhs.len()
                            && target.ends_with(rhs)
                        {
                            interpolate_match = Some((
                                *job_num,
                                &target[interpolate_idx..target.len() - rhs.len()],
                            ));
                            if lhs.len() >= interpolate_lhs_match_len
                                && rhs.len() > interpolate_rhs_match_len
                            {
                                interpolate_lhs_match_len = lhs.len();
                                interpolate_rhs_match_len = rhs.len();
                            }
                        }
                    }
                }
                match interpolate_match {
                    Some((job_num, interpolate)) => {
                        let task_deps = &self.tasks[self.get_job(job_num).unwrap().task].deps;
                        let input = replace_interpolate(
                            task_deps.iter().find(|dep| dep.contains("#")).unwrap(),
                            interpolate,
                        );
                        let num = self
                            .expand_interpolate_match(
                                watcher,
                                &input,
                                interpolate,
                                job_num,
                                self.get_job(job_num).unwrap().task,
                            )
                            .await?;
                        Ok(num)
                    }
                    None => {
                        // fallback to task name
                        match self.lookup_task_name(watcher, target).await? {
                            Some(job_num) => Ok(job_num),
                            // Otherwise add as a file dependency
                            None => {
                                if glob_files {
                                    Ok(self.add_file(String::from(target))?)
                                } else {
                                    Err(anyhow!("No target task '{}' defined in the Chompfile. \nRun \x1b[36mchomp --list\x1b[0m to see the available named targets.", target))
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[async_recursion(?Send)]
    async fn lookup_glob_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        glob_files: bool,
    ) -> Result<Vec<usize>> {
        assert!(has_glob_chars(target));
        let task_pattern = target.as_bytes()[0] as char == ':';
        let target = if task_pattern { &target[1..] } else { target };
        let target_pattern = match Pattern::new(target) {
            Ok(pattern) => pattern,
            Err(e) => {
                return Err(anyhow!("Unable to parse pattern {}, {}", target, e.msg));
            }
        };

        // Determine non-glob prefix and suffix of the target
        let mut target_prefix_len = 0;
        let mut target_suffix_len = 0;
        while target_prefix_len < target.len() && !has_glob_chars(&target[0..target_prefix_len + 1])
        {
            target_prefix_len = target_prefix_len + 1;
        }
        while target_suffix_len < target.len()
            && !has_glob_chars(&target[target.len() - target_suffix_len - 1..])
        {
            target_suffix_len = target_suffix_len + 1;
        }
        let target_prefix = &target[0..target_prefix_len];
        let target_suffix = &target[target.len() - target_suffix_len..];

        let mut found = Vec::new();

        // iterate tasks comparing them to the glob
        if task_pattern {
            // all interpolate tasks with names matching the non-glob prefix and suffix are then _fully_ expanded
            let mut expansions = Vec::new();
            for job_num in &self.interpolate_nodes {
                let job = self.get_job(*job_num).unwrap();
                let task_num = job.task;
                let job_task = &self.tasks[task_num];
                if let Some(name) = &job_task.name {
                    if let Some((interpolate_idx, double)) = find_interpolate(name)? {
                        let lhs = &name[0..interpolate_idx];
                        let rhs = &name[interpolate_idx + if double { 2 } else { 1 }..];

                        let maybe_intersects = if lhs.len() > target_prefix.len() {
                            lhs.starts_with(target_prefix)
                        } else {
                            target_prefix.starts_with(lhs)
                        } && if rhs.len() > target_suffix.len() {
                            rhs.ends_with(target_suffix)
                        } else {
                            target_suffix.ends_with(rhs)
                        };

                        if !maybe_intersects {
                            continue;
                        }

                        let interpolate_dep =
                            job_task.deps.iter().find(|&dep| dep.contains('#')).unwrap();
                        expansions.push(((interpolate_dep.to_owned()), *job_num, task_num));
                    }
                }
            }

            for (dep, job_num, task_num) in expansions.drain(..) {
                self.expand_interpolate(watcher, dep, job_num, task_num)
                    .await?;
            }

            for (task, &job_num) in &self.task_jobs {
                if target_pattern.matches(task) {
                    found.push(job_num);
                }
            }

            if found.len() == 0 {
                return Err(anyhow!(
                    "No task names found matching the pattern {}",
                    target
                ));
            }
        } else {
            let mut globbed_targets: HashSet<String> = HashSet::new();

            // all interpolates which match that non-glob prefix and suffix are then _fully_ expanded
            let mut expansions = Vec::new();
            for job_num in &self.interpolate_nodes {
                if let Some(interpolate) = self.get_interpolate_target(*job_num) {
                    let (interpolate_idx, double) = find_interpolate(interpolate).unwrap().unwrap();
                    let lhs = &interpolate[0..interpolate_idx];
                    let rhs = &interpolate[interpolate_idx + if double { 2 } else { 1 }..];

                    let maybe_intersects = if lhs.len() > target_prefix.len() {
                        lhs.starts_with(target_prefix)
                    } else {
                        target_prefix.starts_with(lhs)
                    } && if rhs.len() > target_suffix.len() {
                        rhs.ends_with(target_suffix)
                    } else {
                        target_suffix.ends_with(rhs)
                    };

                    if !maybe_intersects {
                        continue;
                    }

                    let job = self.get_job(*job_num).unwrap();
                    let task_num = job.task;
                    let interpolate_dep = self.tasks[task_num]
                        .deps
                        .iter()
                        .find(|&dep| dep.contains('#'))
                        .unwrap();
                    expansions.push((interpolate_dep.to_owned(), *job_num, task_num));
                }
            }

            for (dep, job_num, task_num) in expansions.drain(..) {
                self.expand_interpolate(watcher, dep, job_num, task_num)
                    .await?;
            }

            // this picks up both static file targets and interpolates expanded above
            for (file, &job_num) in &self.file_nodes {
                if target_pattern.matches(file) {
                    found.push(job_num);
                    globbed_targets.insert(String::from(file));
                }
            }

            // finally we do file system globbing, with defined files above overriding file system matches
            if glob_files {
                for entry in glob(target).expect("Failed to read glob pattern") {
                    match entry {
                        Ok(entry) => {
                            let dep_path =
                                String::from(entry.path().to_str().unwrap()).replace('\\', "/");
                            if !globbed_targets.contains(&dep_path) {
                                let job_num = self.add_file(dep_path.to_string())?;
                                found.push(job_num);
                                globbed_targets.insert(dep_path);
                            }
                        }
                        Err(e) => {
                            eprintln!("{:?}", e);
                            return Err(anyhow!("GLOB ERROR"));
                        }
                    }
                }
            }

            if found.len() == 0 {
                return Err(anyhow!("No files or target paths found matching the pattern '{}'.\nTo glob task names, use the task prefix character:\n\n  \x1b[36mchomp :{}\x1b[0m\n", target, target));
            }
        }

        Ok(found)
    }

    #[async_recursion(?Send)]
    async fn expand_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        glob_files: bool,
        drives: Option<usize>,
    ) -> Result<Vec<usize>> {
        let job_nums = if !has_glob_chars(target) {
            vec![self.lookup_target(watcher, target, glob_files).await?]
        } else {
            self.lookup_glob_target(watcher, target, glob_files).await?
        };
        for &job_num in job_nums.iter() {
            self.expand_job(watcher, job_num, drives).await?;
        }
        Ok(job_nums)
    }

    // expand out the full job graph for the given targets
    #[async_recursion(?Send)]
    async fn expand_job(
        &mut self,
        watcher: &mut RecommendedWatcher,
        job_num: usize,
        parent: Option<usize>,
    ) -> Result<()> {
        if let Some(parent) = parent {
            let deps = &mut self.get_job_mut(parent).unwrap().deps;
            if deps.iter().find(|&&d| d == job_num).is_some() {
                return Ok(());
            }
            deps.push(job_num);
        }

        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if !matches!(job.state, JobState::Uninitialized) {
                    if let Some(parent) = parent {
                        job.parents.push(parent);
                    }
                    return Ok(());
                }
                let mut is_interpolate = None;
                let mut double_interpolate = false;
                let display_name = job.display_name(&self.tasks);

                let task_num = job.task;
                let task = &self.tasks[job.task];
                let mut job_targets = Vec::new();
                for target in task.targets.iter() {
                    if has_glob_chars(&target) {
                        return Err(anyhow!("Error processing target '{}' in task {} - glob characters are not supported", &target, &display_name));
                    }
                    if target.contains('#') {
                        if is_interpolate.is_some() {
                            return Err(anyhow!("Error processing target '{}' in task {} - can only have a single interpolation target per task", &target, &display_name));
                        }
                        is_interpolate = Some(target.clone());
                        double_interpolate = target.contains("##");
                    }
                    job_targets.push(target.to_string());
                }
                if task.chomp_task.args.is_some() && is_interpolate.is_some() {
                    return Err(anyhow!(
                        "Invalid task {} - cannot apply args to interpolate tasks.",
                        &display_name
                    ));
                }
                if is_interpolate.is_none() {
                    job.targets = job_targets;
                }

                job.state = JobState::Initialized;

                let mut expanded_interpolate = false;
                let mut dep_double_interpolate = false;
                let task_id = job.task;
                let deps = task.deps.clone();
                for dep in deps {
                    if dep.contains('#') {
                        if has_glob_chars(&dep) {
                            return Err(anyhow!("Error processing dep '{}' in task {} - glob deps are not supported in interpolates", &dep, &display_name));
                        }
                        if expanded_interpolate {
                            return Err(anyhow!("Error processing dep '{}' in task {} - only one interpolated deps is allowed", &dep, &display_name));
                        }
                        dep_double_interpolate = dep.contains("##");
                        self.expand_interpolate(watcher, dep, job_num, task_num)
                            .await?;
                        expanded_interpolate = true;
                    } else if dep.starts_with('&') {
                        if dep == "&next" {
                            if task_id + 1 >= self.tasks.len() {
                                return Err(anyhow!(
                                    "No next task to reference for dep '&next' in task {}",
                                    &display_name
                                ));
                            }
                            let dep_num = self.lookup_task(task_id + 1).unwrap();
                            self.expand_job(watcher, dep_num, Some(job_num)).await?;
                        } else if dep == "&prev" {
                            if task_id == 0 {
                                return Err(anyhow!(
                                    "No previous task to reference for dep '&prev' in task {}",
                                    &display_name
                                ));
                            }
                            let dep_num = self.lookup_task(task_id - 1).unwrap();
                            self.expand_job(watcher, dep_num, Some(job_num)).await?;
                        } else {
                            return Err(anyhow!(
                                "Invalid task reference '{}' in task {}",
                                &dep,
                                &display_name
                            ));
                        }
                    } else {
                        self.expand_target(watcher, &String::from(dep), true, Some(job_num))
                            .await?;
                    }
                }

                if is_interpolate.is_some() {
                    if !expanded_interpolate {
                        return Err(anyhow!(
                            "Task {} defines an interpolation target {} without an interpolation dep",
                            &display_name,
                            is_interpolate.unwrap()
                        ));
                    }
                    if dep_double_interpolate != double_interpolate {
                        return Err(anyhow!(
                            "Task {} defines a {} interpolate target {} but with a {} interpolation dep. Dependency interpolation must use a '{}' interpolate to match.",
                            &display_name,
                            if double_interpolate { "double" } else { "single" },
                            is_interpolate.unwrap(),
                            if double_interpolate { "single" } else { "double" },
                            if double_interpolate { "##" } else { "#" }
                        ));
                    }
                }
            }
            Node::File(ref mut file) => {
                file.init(if self.watch { Some(watcher) } else { None });
            }
        }
        Ok(())
    }

    async fn expand_interpolate(
        &mut self,
        watcher: &mut RecommendedWatcher,
        dep: String,
        parent_job: usize,
        parent_task: usize,
    ) -> Result<()> {
        let (interpolate_idx, double) = find_interpolate(&dep)?.unwrap();
        let mut glob_target = String::new();
        glob_target.push_str(&dep[0..interpolate_idx]);
        if double {
            if !glob_target.starts_with("##")
                && !glob_target.ends_with('/')
                && !glob_target.ends_with('\\')
            {
                return Err(anyhow!("Unable to apply deep globbing to interpolate {}. Deep globbing interpolates are only supported for full paths with '##' immediately following a separator position.", &dep));
            }
            glob_target.push_str("(**/*)");
        } else {
            glob_target.push_str("(*)");
        }
        glob_target.push_str(&dep[interpolate_idx + if double { 2 } else { 1 }..]);
        for entry in
            glob(&glob_target).expect(&format!("Failed to read glob pattern {}", &glob_target))
        {
            match entry {
                Ok(entry) => {
                    let dep_path = String::from(entry.path().to_str().unwrap().replace('\\', "/"));
                    let interpolate = &dep_path[interpolate_idx
                        ..dep_path.len() + interpolate_idx + if double { 2 } else { 1 }
                            - dep.len()];

                    let task = &self.tasks[parent_task];
                    if check_interpolate_exclude(&task, &dep_path) {
                        return Ok(());
                    }

                    self.expand_interpolate_match(
                        watcher,
                        &dep_path,
                        interpolate,
                        parent_job,
                        parent_task,
                    )
                    .await?;
                }
                Err(e) => {
                    eprintln!("{:?}", e);
                    return Err(anyhow!("GLOB ERROR"));
                }
            }
        }
        Ok(())
    }

    #[async_recursion(?Send)]
    async fn expand_interpolate_match(
        &mut self,
        watcher: &mut RecommendedWatcher,
        dep_path: &str,
        interpolate: &str,
        parent_job: usize,
        parent_task: usize,
    ) -> Result<usize> {
        let watch = self.watch;
        let task = &self.tasks[parent_task];
        let targets = task.targets.clone();

        let (job_num, new_job) = self.add_job(parent_task, Some(String::from(interpolate)))?;

        // Already defined -> skip
        if !new_job {
            return Ok(job_num);
        }

        let dep_num = if let Some(&existing) = self.file_nodes.get(dep_path) {
            match self.nodes[existing] {
                Node::File(ref mut file) => file.parents.push(job_num),
                Node::Job(ref mut job) => job.parents.push(job_num),
            }
            existing
        } else {
            let dep_num = self.add_file(dep_path.to_string())?;
            let file = self.get_file_mut(dep_num).unwrap();
            file.parents.push(job_num);
            file.init(if watch { Some(watcher) } else { None });
            dep_num
        };

        let job = self.get_job_mut(job_num).unwrap();
        job.deps.push(dep_num);
        // just because an interpolate is expanded, does not mean it is live
        job.state = JobState::Initialized;

        let mut expansions = Vec::new();

        for parent_target in targets {
            let expanded_target = replace_interpolate(&parent_target, interpolate);
            // If the interpolation target is itself an interpolation source, then drive that
            // Note: this should also apply to wildcard dependency expansions as well!
            if parent_target.contains("#") || parent_target.contains("##") {
                for job_num in &self.interpolate_nodes {
                    let job = self.get_job(*job_num).unwrap();
                    let task_num = job.task;
                    let job_task = &self.tasks[task_num];
                    if let Some(interpolation_dep) = job_task.deps.iter().find(|&t| t.contains('#'))
                    {
                        let dep_glob = if interpolation_dep.contains("##") {
                            interpolation_dep.replace("##", "(**/*)")
                        } else {
                            interpolation_dep.replace('#', "(*)")
                        };
                        if Pattern::new(&dep_glob).unwrap().matches(&expanded_target) {
                            if !check_interpolate_exclude(&job_task, &expanded_target) {
                                let interpolate =
                                    get_interpolate_match(&interpolation_dep, &expanded_target);
                                let input = replace_interpolate(interpolation_dep, &interpolate);
                                expansions.push((
                                    input.to_owned(),
                                    interpolate,
                                    *job_num,
                                    task_num,
                                ));
                            }
                        }
                    }
                }
                let job = self.get_job_mut(job_num).unwrap();
                job.targets.push(expanded_target);
            }
        }
        let parent = self.get_job_mut(parent_job).unwrap();
        parent.deps.push(job_num);
        for (dep, interpolate, parent_job, parent_task) in expansions.drain(..) {
            self.expand_interpolate_match(watcher, &dep, &interpolate, parent_job, parent_task)
                .await?;
        }

        // non-interpolation parent interpolation template deps are child deps
        let parent_task_deps = self.tasks[parent_task].deps.clone();
        for dep in parent_task_deps {
            if !dep.contains('#') {
                self.expand_target(watcher, &dep, true, Some(job_num))
                    .await?;
            }
        }

        Ok(job_num)
    }

    // find the job for the target, and drive its completion
    async fn drive_jobs(
        &mut self,
        watcher: &mut RecommendedWatcher,
        jobs: &HashSet<usize>,
        force: bool,
        rx: Receiver<DebouncedEvent>,
        watch_listener: UnboundedSender<DebouncedEvent>,
        mut writer: UnboundedReceiver<FileEvent>,
    ) -> Result<()> {
        let mut futures: Vec<Pin<Box<dyn Future<Output = StateTransition> + 'a>>> = Vec::new();

        let mut queued = QueuedStateTransitions::new();

        // first try named target, then fall back to file name check
        for &job_num in jobs {
            // if a job, make it live
            if let Some(ref mut job) = self.get_job_mut(job_num) {
                job.live = true;
            }
            self.drive_all(
                job_num,
                force,
                &mut futures,
                &mut queued,
                None,
                watch_listener.clone(),
            )?;
        }
        if self.watch {
            futures.push(Runner::watcher_interval().boxed_local());
        }
        while futures.len() > 0 {
            let (transition, _idx, new_futures) = select_all(futures).await;
            futures = new_futures;
            match transition.state {
                // Sentinel value used to enforce watcher task looping
                JobOrFileState::Job(JobState::Sentinel) => {
                    let mut redrives = HashSet::new();
                    while self
                        .check_watcher(
                            watcher,
                            &rx,
                            watch_listener.clone(),
                            &mut writer,
                            &mut queued,
                            &mut redrives,
                        )
                        .await?
                    {}
                    for job_num in redrives {
                        self.drive_all(
                            job_num,
                            false,
                            &mut futures,
                            &mut queued,
                            None,
                            watch_listener.clone(),
                        )?;
                    }
                    futures.push(Runner::watcher_interval().boxed_local());
                }
                _ => {
                    self.drive_completion(
                        transition,
                        force,
                        &mut futures,
                        &mut queued,
                        watch_listener.clone(),
                    )?;
                }
            }
        }
        Ok(())
    }

    async fn watcher_interval() -> StateTransition {
        time::sleep(Duration::from_millis(50)).await;
        StateTransition {
            node_num: 0,
            cmd_num: None,
            state: JobOrFileState::Job(JobState::Sentinel),
        }
    }

    async fn check_watcher(
        &mut self,
        watcher: &mut RecommendedWatcher,
        rx: &Receiver<DebouncedEvent>,
        watch_listener: UnboundedSender<DebouncedEvent>,
        writer: &mut UnboundedReceiver<FileEvent>,
        queued: &mut QueuedStateTransitions,
        redrives: &mut HashSet<usize>,
    ) -> Result<bool> {
        let mut keep_checking = true;
        while keep_checking {
            match writer.try_recv() {
                Ok(FileEvent::WatchFile(path)) => {
                    let subpath = path
                        .strip_prefix(&self.cwd)
                        .expect("Internal error: Invalid path to watch");
                    let normalized_target = subpath.to_str().unwrap().replace('\\', "/");
                    let jobs = self
                        .expand_target(watcher, &normalized_target, true, None)
                        .await?;
                    for job_num in jobs {
                        // server watcher can actually create new live jobs
                        if let Some(ref mut job) = self.get_job_mut(job_num) {
                            job.live = true;
                            redrives.insert(job_num);
                        }
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => keep_checking = false,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    panic!("Server file channel disconnected")
                }
            };
        }
        let evt = match rx.try_recv() {
            Ok(evt) => evt,
            Err(TryRecvError::Empty) => {
                return Ok(false);
            }
            Err(TryRecvError::Disconnected) => panic!("Watcher disconnected"),
        };
        let result = match &evt {
            DebouncedEvent::NoticeWrite(_)
            | DebouncedEvent::NoticeRemove(_)
            | DebouncedEvent::Chmod(_) => Ok(false),
            DebouncedEvent::Remove(path)
            | DebouncedEvent::Create(path)
            | DebouncedEvent::Write(path)
            | DebouncedEvent::Rename(_, path) => {
                self.invalidate_path(path.clone(), queued, redrives)
            }
            DebouncedEvent::Rescan => panic!("Watcher rescan"),
            DebouncedEvent::Error(err, maybe_path) => {
                panic!("WATCHER ERROR {:?} {:?}", err, maybe_path.clone())
            }
        };
        watch_listener
            .send(evt)
            .expect("Unable to send watcher event to server channel");
        result
    }

    pub async fn run(
        &mut self,
        opts: RunOptions,
        watch_listener: UnboundedSender<DebouncedEvent>,
        watch_writer: UnboundedReceiver<FileEvent>,
    ) -> Result<bool> {
        let (tx, rx) = channel();
        let mut watcher = watcher(tx, Duration::from_millis(250)).unwrap();
        let normalized_targets: Vec<String> = opts
            .targets
            .iter()
            .map(|t| {
                let normalized = t.replace('\\', "/");
                if normalized.starts_with("./") {
                    normalized[2..].to_string()
                } else {
                    normalized
                }
            })
            .collect();
        let mut job_nums = HashSet::new();
        for target in normalized_targets {
            let jobs = self
                .expand_target(&mut watcher, &target, false, None)
                .await?;
            for job in jobs {
                if opts.rerun {
                    let mut job = self.get_job_mut(job).unwrap();
                    job.mtime = None;
                    job.state = JobState::Pending;
                }
                job_nums.insert(job);
            }
        }
        // When running with arguments, mutate the task environment to include the arguments
        // Arguments tasks cannot be cached
        if let Some(args) = opts.args {
            if job_nums.len() > 1 {
                return Err(anyhow!(
                    "Custom args are only supported when running a single command."
                ));
            }
            let &job_num = job_nums.iter().next().unwrap();
            let task_num = self.get_job(job_num).unwrap().task;
            let task = &mut self.tasks[task_num];
            let task_args_len = match &task.chomp_task.args {
                Some(args) => args.len(),
                None => {
                    return Err(anyhow!(
                        "Task \x1b[1m{}\x1b[0m doesn't take any arguments.",
                        self.get_job(job_num).unwrap().display_name(&self.tasks)
                    ));
                }
            };
            if task_args_len < args.len() {
                return Err(anyhow!(
                    "Task \x1b[1m{}\x1b[0m only takes {} arguments, while {} were provided.",
                    self.get_job(job_num).unwrap().display_name(&self.tasks),
                    task_args_len,
                    args.len()
                ));
            }
            let task_args = task.chomp_task.args.as_ref().unwrap();
            for (i, arg) in args.iter().enumerate() {
                task.env.insert(task_args[i].to_uppercase(), arg.clone());
            }
        }

        self.drive_jobs(
            &mut watcher,
            &job_nums,
            opts.force,
            rx,
            watch_listener,
            watch_writer,
        )
        .await?;
        // if all jobs completed successfully, exit code is 0, otherwise its an error
        let mut all_ok = true;
        for &job_num in job_nums.iter() {
            let job = self.get_job(job_num).unwrap();
            if !matches!(job.state, JobState::Fresh) {
                all_ok = false;
                break;
            }
        }

        Ok(all_ok)
    }
}

pub fn relative_path(name: &str, cwd: &str) -> String {
    if name.starts_with(':') || (!name.contains('/') && !name.contains("\\")) {
        name.to_owned()
    } else {
        diff_paths(Path::new(&name), Path::new(cwd))
            .unwrap()
            .to_string_lossy()
            .to_string()
    }
}

use crate::chompfile::{ChompTemplate, Chompfile, ChompTaskMaybeTemplated};
use crate::engines::{CmdPool, ChompEngine};
use crate::ui::ChompUI;
use async_std::process::ExitStatus;
use futures::future::{select_all, Future, FutureExt, Shared};
use notify::op::Op;
use notify::{RawEvent, RecommendedWatcher};
use std::collections::BTreeMap;
use std::io::ErrorKind::NotFound;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
extern crate num_cpus;
use async_recursion::async_recursion;
use async_std::fs;
use capturing_glob::glob;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Instant;
extern crate notify;
use derivative::Derivative;
use crate::js::run_js_fn;
use serde_json::json;

use notify::{raw_watcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

#[derive(Debug)]
struct Task {
    name: Option<String>,
    targets: Vec<String>,
    deps: Vec<String>,
    env: BTreeMap<String, String>,
    run: Option<String>,
    engine: ChompEngine,
}

pub struct RunOptions<'a> {
    pub ui: &'a ChompUI,
    pub cwd: PathBuf,
    pub cfg_file: PathBuf,
    pub targets: Vec<String>,
    pub watch: bool,
}

#[derive(Debug)]
pub enum TaskError {
    IoError(std::io::Error),
    BadFileError(String),
    ConfigParseError(toml::de::Error),
    InvalidVersionError(String),
    TaskNotFound(String, String),
}

impl From<std::io::Error> for TaskError {
    fn from(e: std::io::Error) -> TaskError {
        TaskError::IoError(e)
    }
}

impl From<toml::de::Error> for TaskError {
    fn from(e: toml::de::Error) -> TaskError {
        TaskError::ConfigParseError(e)
    }
}

// impl fmt::Display for TaskError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, format!("Compile error: {}", "test"))
//     }
// }

#[derive(Clone, Copy, Debug)]
enum JobState {
    Uninitialized,
    Initializing,
    Pending,
    Running,
    Fresh,
    Failed,
}

#[derive(Debug, Derivative)]
struct Job {
    interpolate: Option<String>,
    task: usize,
    deps: Vec<usize>,
    drives: Vec<usize>,
    state: JobState,
    mtime: Option<Duration>,
    targets: Vec<String>,
    start_time_deps: Option<Instant>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    #[derivative(Debug = "ignore")]
    future: Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
}

#[derive(Debug)]
enum Node {
    Job(Job),
    File(File),
}

#[derive(Debug)]
enum FileState {
    Uninitialized,
    Initializing,
    Found,
    NotFound,
}

#[derive(Debug)]
struct File {
    name: String,
    drives: Vec<usize>,
    state: FileState,
    mtime: Option<Duration>,
}

impl File {
    fn new(name: String) -> File {
        File {
            name,
            mtime: None,
            drives: Vec::new(),
            state: FileState::Uninitialized,
        }
    }

    async fn init(&mut self, watcher: &mut RecommendedWatcher, parent_job: Option<usize>) {
        self.state = FileState::Initializing;
        if let Some(parent_job) = parent_job {
            self.drives.push(parent_job);
        }
        match fs::metadata(&self.name).await {
            Ok(n) => {
                let mtime = n.modified().expect("No modified implementation");
                self.mtime = Some(mtime.duration_since(UNIX_EPOCH).unwrap());
                self.state = FileState::Found;
            }
            Err(e) => match e.kind() {
                NotFound => {
                    self.state = FileState::NotFound;
                }
                _ => panic!("Unknown file error"),
            },
        };
        match watcher.watch(&self.name, RecursiveMode::Recursive) {
            Ok(_) => {}
            Err(_) => {
                eprintln!("Unable to watch {}", self.name);
            }
        };
    }
}

struct Runner<'a> {
    ui: &'a ChompUI,
    cmd_pool: CmdPool,
    chompfile: &'a Chompfile,
    tasks: Vec<Task>,

    nodes: Vec<Node>,

    task_jobs: BTreeMap<String, usize>,
    file_nodes: BTreeMap<String, usize>,
    interpolate_nodes: Vec<(String, usize)>,
}

impl<'a> Job {
    fn new(task: usize, interpolate: Option<String>) -> Job {
        Job {
            interpolate,
            task,
            deps: Vec::new(),
            drives: Vec::new(),
            state: JobState::Uninitialized,
            targets: Vec::new(),
            mtime: None,
            start_time_deps: None,
            start_time: None,
            end_time: None,
            future: None,
        }
    }

    fn display_name(&self, runner: &Runner) -> String {
        match self.targets.first() {
            Some(target) => {
                if target.contains('#') {
                    target.replace("#", &self.interpolate.as_ref().unwrap())
                } else {
                    String::from(target)
                }
            }
            None => {
                let task = &runner.tasks[self.task];
                match &task.name {
                    Some(name) => String::from(format!(":{}", name)),
                    None => match &task.run {
                        Some(run) => String::from(format!("{}", run)),
                        None => String::from(format!("[task {}]", self.task)),
                    },
                }
            }
        }
    }

    async fn init(&mut self, parent_job: Option<usize>) {
        self.state = JobState::Initializing;
        self.start_time_deps = Some(Instant::now());
        if let Some(parent_job) = parent_job {
            self.drives.push(parent_job);
        }
        let mut futures = Vec::new();
        for target in &self.targets {
            futures.push(
                async move {
                    match fs::metadata(target).await {
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
                .boxed(),
            );
        }
        while futures.len() > 0 {
            let (completed, _, new_futures) = select_all(futures).await;
            futures = new_futures;
            if completed > self.mtime {
                self.mtime = completed;
            }
        }
        self.state = JobState::Pending;
    }
}

impl<'a> Runner<'a> {
    fn new(ui: &'a ChompUI, chompfile: &'a Chompfile, cwd: &'a PathBuf) -> Runner<'a> {
        let cmd_pool = CmdPool::new(8, cwd.to_str().unwrap().to_string());
        let mut runner = Runner {
            ui,
            cmd_pool,
            chompfile,
            nodes: Vec::new(),
            tasks: Vec::new(),
            task_jobs: BTreeMap::new(),
            file_nodes: BTreeMap::new(),
            interpolate_nodes: Vec::new(),
        };

        let mut templates: BTreeMap<&String, &ChompTemplate> = BTreeMap::new();
        if let Some(ref chompfile_templates) = chompfile.template {
            for template in chompfile_templates {
                templates.insert(&template.name, &template);
            }
        }

        // expand tasks into initial job list
        if let Some(tasks) = &runner.chompfile.task {
            for (i, task) in tasks.iter().enumerate() {
                let mut maybe_template_task: ChompTaskMaybeTemplated;
                let task = match &task.template {
                    Some(template) => {
                        // evaluate templates into tasks
                        if task.deps.is_some() || task.engine.is_some() || task.env.is_some() ||
                            task.targets.is_some() || task.target.is_some() || task.run.is_some() {
                            panic!("Template does not support normal task fields");
                        }

                        let template = templates.get(template).expect("Unable to find template");
                        maybe_template_task = run_js_fn(&template.definition.task, json!({
                            "hello": "world"
                        }));
                        if let Some(name) = &task.name {
                            maybe_template_task.name = Some(name.to_string());
                        }
                        &maybe_template_task
                    }
                    None => task
                };
                let deps = match &task.deps {
                    Some(deps) => deps.clone(),
                    None => Vec::new()
                };
                let engine = match &task.engine {
                    Some(engine) if engine == "node" => ChompEngine::Node,
                    Some(engine) if engine == "cmd" => ChompEngine::Cmd,
                    Some(engine) => panic!("Unknown engine {}", engine),
                    None => ChompEngine::Cmd,
                };
                let env = if let Some(global_env) = &runner.chompfile.env {
                    let mut env = BTreeMap::new();
                    for (item, value) in global_env {
                        env.insert(item.to_uppercase(), value.to_string());
                    }
                    if let Some(local_env) = &task.env {
                        for (item, value) in local_env {
                            env.insert(item.to_uppercase(), value.to_string());
                        }
                    }
                    env
                } else if let Some(local_env) = &task.env {
                    let mut env = BTreeMap::new();
                    for (item, value) in local_env {
                        env.insert(item.to_uppercase(), value.to_string());
                    }
                    env
                } else {
                    BTreeMap::new()
                };

                let mut targets: Vec<String> = Vec::new();
                if let Some(target) = &task.target {
                    targets.push(target.to_string());
                }
                else if let Some(task_targets) = &task.targets {
                    for target in task_targets {
                        targets.push(target.to_string());
                    }
                }

                runner.tasks.push(Task {
                    name: task.name.clone(),
                    deps,
                    engine,
                    env,
                    run: task.run.clone(),
                    targets,
                });
                runner.add_job(i, None);
            }
        }
        runner
    }

    fn add_job(&mut self, task_num: usize, interpolate: Option<String>) -> usize {
        let num = self.nodes.len();
        let task = &self.tasks[task_num];

        let mut is_interpolate_target = false;
        for t in task.targets.iter() {
            if !is_interpolate_target && t.contains('#') {
                is_interpolate_target = true;
            }
        }

        // map target name
        if let Some(ref name) = task.name {
            if interpolate.is_none() {
                if self.task_jobs.contains_key(name) {
                    panic!("Already has job");
                }
                self.task_jobs.insert(name.to_string(), num);
            }
        }

        // map interpolation for primary interpolation job
        if is_interpolate_target && interpolate.is_none() {
            for t in task.targets.iter() {
                if t.contains('#') {
                    self.interpolate_nodes.push((t.to_string(), num));
                }
            }
        }

        // map target file as file node
        if !is_interpolate_target || interpolate.is_some() {
            for target in task.targets.iter() {
                let file_target = match &interpolate {
                    Some(interpolate) => {
                        if !target.contains("#") {
                            continue;
                        }
                        target.replace("#", interpolate)
                    }
                    None => target.to_string(),
                };
                match self.file_nodes.get(&file_target) {
                    Some(_) => {
                        panic!("Multiple targets pointing to same file");
                    }
                    None => {
                        self.file_nodes.insert(file_target, num);
                    }
                }
            }
        }

        self.nodes.push(Node::Job(Job::new(task_num, interpolate)));
        return num;
    }

    fn add_file(&mut self, file: String) -> usize {
        let num = self.nodes.len();
        let file2 = file.to_string();
        self.nodes.push(Node::File(File::new(file)));
        if self.file_nodes.contains_key(&file2) {
            panic!("Already has file");
        }
        self.file_nodes.insert(file2, num);
        return num;
    }

    fn get_job(&self, num: usize) -> Option<&Job> {
        match self.nodes[num] {
            Node::Job(ref job) => Some(job),
            _ => None,
        }
    }

    fn get_job_mut(&mut self, num: usize) -> Option<&mut Job> {
        match self.nodes[num] {
            Node::Job(ref mut job) => Some(job),
            _ => None,
        }
    }

    fn get_file_mut(&mut self, num: usize) -> Option<&mut File> {
        match self.nodes[num] {
            Node::File(ref mut file) => Some(file),
            _ => None,
        }
    }

    fn mark_complete(&mut self, job_num: usize, updated: bool, failed: bool) {
        {
            let job = self.get_job_mut(job_num).unwrap();
            if updated {
                job.mtime = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
            }
            job.end_time = Some(Instant::now());
            job.state = if failed {
                JobState::Failed
            } else {
                JobState::Fresh
            };
            job.future = None;
        }
        let job = self.get_job(job_num).unwrap();
        let end_time = job.end_time.unwrap();
        if let Some(start_time_deps) = job.start_time_deps {
            if let Some(start_time) = job.start_time {
                if failed {
                    println!(
                        "x {} [{:?} {:?}]",
                        job.display_name(self),
                        end_time - start_time,
                        end_time - start_time_deps
                    );
                } else {
                    println!(
                        "√ {} [{:?} {:?}]",
                        job.display_name(self),
                        end_time - start_time,
                        end_time - start_time_deps
                    );
                }
            } else {
                if failed {
                    panic!("Did not expect failed for cached");
                }
                println!(
                    "- {} [- {:?}]",
                    job.display_name(self),
                    end_time - start_time_deps
                );
            }
        } else {
            if let Some(start_time) = job.start_time {
                if failed {
                    println!(
                        "x {} [{:?}]",
                        job.display_name(self),
                        end_time - start_time
                    );
                } else {
                    println!(
                        "√ {} [{:?}]",
                        job.display_name(self),
                        end_time - start_time
                    );
                }
            } else {
                if failed {
                    panic!("Did not expect failed for cached");
                }
                println!("● {} [cached]", job.display_name(self));
            }
        }
        {
            let job = self.get_job_mut(job_num).unwrap();
            job.start_time_deps = None;
        }
    }

    fn invalidate(
        &mut self,
        path: PathBuf,
        jobs: &mut Vec<usize>,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
    ) -> Result<bool, TaskError> {
        let cwd = std::env::current_dir()?;
        let cwd_str = cwd.to_str().unwrap();
        let path_str = path.to_str().unwrap();
        if !path_str.starts_with(cwd_str) {
            panic!("Expected path within cwd");
        }
        let rel_str = &path_str[cwd_str.len() + 1..];
        let sanitized_path = rel_str.replace("\\", "/");
        match self.file_nodes.get(&sanitized_path) {
            Some(&job_num) => match self.nodes[job_num] {
                Node::Job(_) => panic!("TODO: Job invalidator"),
                Node::File(ref mut file) => {
                    file.mtime = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
                    let drives = file.drives.clone();
                    for drive in drives {
                        self.drive_all(drive, jobs, futures, true)?;
                    }
                    Ok(true)
                }
            },
            None => Ok(false),
        }
    }

    fn run_job(
        &mut self,
        job_num: usize,
    ) -> Result<Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>, TaskError> {
        let job = match &self.nodes[job_num] {
            Node::Job(job) => job,
            Node::File(_) => panic!("Expected job"),
        };
        let task = &self.tasks[job.task];
        // CMD Exec
        if task.run.is_none() {
            self.mark_complete(job_num, false, false);
            return Ok(None);
        }
        // the interpolation template itself is not run
        if job.interpolate.is_none() {
            let mut has_interpolation = false;
            for target in task.targets.iter() {
                if !has_interpolation && target.contains('#') {
                    has_interpolation = true;
                }
            }
            if has_interpolation {
                self.mark_complete(job_num, false, false);
                return Ok(None);
            }
        }
        // If we have an mtime, check if we need to do work
        if let Some(mtime) = job.mtime {
            let mut all_fresh = true;
            for &dep in job.deps.iter() {
                let dep_change = match &self.nodes[dep] {
                    Node::Job(dep) => {
                        let invalidated = match dep.mtime {
                            Some(dep_mtime) if dep_mtime > mtime => true,
                            None => true,
                            _ => false,
                        };
                        if invalidated {
                            println!(
                                "  {} invalidated by {}.",
                                job.display_name(self),
                                dep.display_name(self)
                            );
                        }
                        invalidated
                    }
                    Node::File(dep) => {
                        let invalidated = match dep.mtime {
                            Some(dep_mtime) if dep_mtime > mtime => true,
                            None => true,
                            _ => false,
                        };
                        if invalidated {
                            println!(
                                "  {} invalidated by {}",
                                job.display_name(self),
                                dep.name
                            );
                        }
                        invalidated
                    }
                };
                if dep_change {
                    all_fresh = false;
                    break;
                }
            }
            if all_fresh {
                self.mark_complete(job_num, false, false);
                return Ok(None);
            }
        }
        println!("○ {}", job.display_name(self));

        let run: String = task.run.as_ref().unwrap().to_string();
        let mut env = task.env.clone();
        if let Some(interpolate) = &job.interpolate {
            env.insert("MATCH".to_string(), interpolate.to_string());
        }
        let mut target_index = 0;
        for target in task.targets.iter() {
            let target_str = if let Some(interpolate) = &job.interpolate {
                target.replace("#", &interpolate)
            } else {
                target.to_string()
            };
            if target_index == 0 {
                env.insert("TARGET".to_string(), target_str);
            } else {
                env.insert(format!("TARGET{}", target_index), target_str);
            }
            target_index += 1;
        }
        let dep_index = if job.interpolate.is_some() {
            task.deps.iter()
                .enumerate()
                .find(|(_, d)| d.contains('#'))
                .unwrap()
                .0
        } else {
            0
        };
        for (num, dep) in task.deps.iter().enumerate() {
            if num == dep_index {
                let dep_str = if let Some(interpolate) = &job.interpolate {
                    dep.replace("#", &interpolate)
                } else {
                    dep.to_string()
                };
                env.insert("DEP".to_string(), dep_str);
            }
            env.insert(format!("DEP{}", num), dep.to_string());
        }

        let future = self.cmd_pool.run(run, &mut env, task.engine);
        {
            let job = self.get_job_mut(job_num).unwrap();
            job.future = Some(future.boxed().shared());
            job.state = JobState::Running;
            job.start_time = Some(Instant::now());
            Ok(Some(job.future.clone().unwrap()))
        }
    }

    fn drive_all(
        &mut self,
        job_num: usize,
        jobs: &mut Vec<usize>,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
        invalidation: bool,
    ) -> Result<bool, TaskError> {
        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if invalidation {
                    match job.state {
                        JobState::Failed | JobState::Fresh => {
                            job.state = JobState::Pending;
                        }
                        JobState::Running => {
                            return Ok(false);
                        }
                        _ => {}
                    }
                }
                match job.state {
                    JobState::Uninitialized | JobState::Initializing => {
                        panic!("Expected initialized job");
                    }
                    JobState::Running => {
                        let job = self.get_job(job_num).unwrap();
                        if let Some(future) = &job.future {
                            if !jobs.contains(&job_num) {
                                jobs.push(job_num);
                                futures.push(future.clone());
                            }
                            Ok(false)
                        } else {
                            panic!("Unexpected internal state");
                        }
                    }
                    JobState::Pending => {
                        let mut all_completed = true;
                        let job = self.get_job_mut(job_num).unwrap();
                        let deps = job.deps.clone();
                        // TODO: Use a driver counter for deps
                        for dep in deps {
                            let completed = self.drive_all(dep, jobs, futures, invalidation)?;
                            if !completed {
                                all_completed = false;
                            }
                        }
                        // deps all completed -> execute this job
                        if all_completed {
                            return match self.run_job(job_num)? {
                                Some(future) => {
                                    futures.push(future);
                                    jobs.push(job_num);
                                    Ok(false)
                                }
                                None => {
                                    // already complete -> skip straight to driving parents
                                    // let drives = self.get_job(job_num).unwrap().drives.clone();
                                    // for drive in drives {
                                    //     if self.get_job(job_num).unwrap().live {
                                    //         self.drive_all(drive, jobs, futures)?;
                                    //     }
                                    // }
                                    Ok(true)
                                }
                            };
                        }
                        Ok(false)
                    }
                    JobState::Failed => Ok(false),
                    JobState::Fresh => Ok(true),
                }
            }
            Node::File(ref mut file) => {
                if file.mtime.is_some() {
                    file.state = FileState::Found;
                    Ok(true)
                } else {
                    dbg!(file);
                    panic!("TODO: NON-EXISTING FILE WATCH");
                }
            }
        }
    }

    #[async_recursion(?Send)]
    async fn lookup_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        as_task: bool,
    ) -> Result<usize, TaskError> {
        // First match task by name
        if as_task {
            if target.as_bytes()[0] as char == ':' {
                let name = &target[1..];
                return match self.task_jobs.get(name) {
                    Some(&job_num) => Ok(job_num),
                    None => {
                        panic!("TODO: TASK NOT FOUND");
                    }
                };
            }
            match self.task_jobs.get(target) {
                Some(&job_num) => return Ok(job_num),
                None => {}
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
                for (interpolate, job_num) in &self.interpolate_nodes {
                    let interpolate_idx = interpolate.find("#").unwrap();
                    let lhs = &interpolate[0..interpolate_idx];
                    let rhs = &interpolate[interpolate_idx + 1..];
                    if target.starts_with(lhs)
                        && target.len() > lhs.len() + rhs.len()
                        && target.ends_with(rhs)
                    {
                        interpolate_match =
                            Some((*job_num, &target[interpolate_idx..target.len() - rhs.len()]));
                        if lhs.len() >= interpolate_lhs_match_len
                            && rhs.len() > interpolate_rhs_match_len
                        {
                            interpolate_lhs_match_len = lhs.len();
                            interpolate_rhs_match_len = rhs.len();
                        }
                    }
                }
                match interpolate_match {
                    Some((job_num, interpolate)) => {
                        let task_deps = &self
                            .tasks[self.get_job(job_num).unwrap().task]
                            .deps;
                        let input = task_deps
                            .iter()
                            .find(|dep| dep.contains("#"))
                            .unwrap()
                            .replace("#", interpolate);
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
                    // Otherwise add as a file dependency
                    None => Ok(self.add_file(String::from(target))),
                }
            }
        }
    }

    #[async_recursion(?Send)]
    async fn expand_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        drives: Option<usize>,
    ) -> Result<(), TaskError> {
        let job_num = self.lookup_target(watcher, target, true).await?;
        self.expand_job(watcher, job_num, drives).await
    }

    // expand out the full job graph for the given targets
    #[async_recursion(?Send)]
    async fn expand_job(
        &mut self,
        watcher: &mut RecommendedWatcher,
        job_num: usize,
        drives: Option<usize>,
    ) -> Result<(), TaskError> {
        if let Some(drives) = drives {
            self.get_job_mut(drives).unwrap().deps.push(job_num);
        }

        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if matches!(job.state, JobState::Pending) {
                    if let Some(drives) = drives {
                        job.drives.push(drives);
                    }
                    return Ok(());
                }

                let mut is_interpolate = false;
                let mut is_wildcard = false;

                let task_num = job.task;
                let task = &self.tasks[job.task];
                let mut job_targets = Vec::new();
                for target in task.targets.iter() {
                    if !is_interpolate && target.contains("#") {
                        is_interpolate = true;
                    }
                    if !is_wildcard && target.contains("*") {
                        is_wildcard = true;
                    }
                    if is_wildcard && is_interpolate {
                        panic!("Cannot have wildcard + interpolate");
                    }
                    job_targets.push(target.to_string());
                }
                if !is_interpolate {
                    job.targets = job_targets;
                }

                // this must come after setting target above
                job.init(drives).await;

                if is_wildcard {
                    panic!("TODO: wildcard targets");
                }

                let mut expanded_interpolate = false;
                let deps = task.deps.clone();
                for dep in deps {
                    if dep.contains('#') {
                        if dep.contains('*') {
                            panic!("Wildcard + interpolate not supported");
                        }
                        if !is_interpolate {
                            panic!("Interpolate in deps can only be used when contained in target (and run)");
                        }
                        if expanded_interpolate {
                            panic!("Only one interpolated deps is allowed");
                        }
                        self.expand_interpolate(watcher, String::from(dep), job_num, task_num)
                            .await?;
                        expanded_interpolate = true;
                    } else if dep.contains('*') {
                        panic!("TODO: Wilrdcard deps");
                    } else {
                        self.expand_target(watcher, &String::from(dep), Some(job_num))
                            .await?;
                    }
                }
                if is_interpolate && !expanded_interpolate {
                    panic!("Never found deps interpolates");
                }
            }
            Node::File(ref mut file) => {
                if let Some(drives) = drives {
                    file.drives.push(drives);
                }
                file.init(watcher, drives).await;
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
    ) -> Result<(), TaskError> {
        let interpolate_idx = dep.find("#").unwrap();
        if dep[interpolate_idx + 1..].find("#").is_some() {
            panic!("multiple interpolates");
        }
        let mut glob_target = String::new();
        glob_target.push_str(&dep[0..interpolate_idx]);
        glob_target.push_str("(**/*)");
        glob_target.push_str(&dep[interpolate_idx + 1..]);
        for entry in glob(&glob_target).expect("Failed to read glob pattern") {
            match entry {
                Ok(entry) => {
                    let input_path =
                        String::from(entry.path().to_str().unwrap()).replace("\\", "/");
                    let interpolate = &input_path
                        [interpolate_idx..input_path.len() - dep.len() + interpolate_idx + 1];
                    self.expand_interpolate_match(
                        watcher,
                        &input_path,
                        interpolate,
                        parent_job,
                        parent_task,
                    )
                    .await?;
                }
                Err(e) => {
                    eprintln!("{:?}", e);
                    panic!("GLOB ERROR");
                }
            }
        }
        Ok(())
    }

    async fn expand_interpolate_match(
        &mut self,
        watcher: &mut RecommendedWatcher,
        input: &str,
        interpolate: &str,
        parent_job: usize,
        parent_task: usize,
    ) -> Result<usize, TaskError> {
        let job_num = self.add_job(parent_task, Some(String::from(interpolate)));
        let file_num = self.add_file(input.to_string());
        {
            let file = self.get_file_mut(file_num).unwrap();
            file.init(watcher, Some(job_num)).await;
        }
        let task = &self.tasks[parent_task];
        let mut parent_targets = Vec::new();
        for parent_target in task.targets.iter() {
            parent_targets.push(parent_target.to_string());
        }
        for parent_target in parent_targets {
            let output_path = parent_target.replace("#", interpolate);
            let job = self.get_job_mut(job_num).unwrap();
            job.deps.push(file_num);
            job.targets = vec![output_path.to_string()];
            job.init(Some(parent_job)).await;
            let parent = self.get_job_mut(parent_job).unwrap();
            parent.deps.push(job_num);
            // non-interpolation parent interpolation template deps are child deps
            let parent_task_deps = self.tasks[parent_task].deps.clone();
            for dep in parent_task_deps {
                if !dep.contains("#") {
                    let dep_job = self.lookup_target(watcher, &dep, true).await?;
                    let job = self.get_job_mut(job_num).unwrap();
                    job.deps.push(dep_job);
                    // important aspect of retaining depth-first semantics
                    self.expand_job(watcher, dep_job, Some(job_num)).await?;
                }
            }
        }
        Ok(job_num)
    }

    // find the job for the target, and drive its completion
    async fn drive_targets(&mut self, targets: &Vec<String>) -> Result<(), TaskError> {
        let mut jobs: Vec<usize> = Vec::new();
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>> = Vec::new();

        // dbg!(&self.nodes);
        // dbg!(&self.file_nodes);

        // first try named target, then fall back to file name check
        for target in targets {
            let name = if target.as_bytes()[0] as char == ':' {
                &target[1..]
            } else {
                &target
            };

            let job_num = match self.task_jobs.get(name) {
                Some(&job_num) => job_num,
                None => match self.file_nodes.get(name) {
                    Some(&job_num) => job_num,
                    None => {
                        println!("{}", name);
                        panic!("TODO: target not found error");
                    }
                },
            };

            self.drive_all(job_num, &mut jobs, &mut futures, false)?;
        }

        loop {
            if jobs.len() == 0 {
                break;
            }
            let (completed, idx, new_futures) = select_all(futures).await;
            futures = new_futures;
            let completed_job_num = jobs[idx];
            jobs.remove(idx);
            match completed.code() {
                Some(code) => {
                    if code == 0 {
                        self.mark_complete(completed_job_num, true, false);
                        let job = match &self.nodes[completed_job_num] {
                            Node::Job(job) => job,
                            _ => panic!("Expected job"),
                        };
                        let drives = job.drives.clone();
                        for drive in drives {
                            let job = match &self.nodes[drive] {
                                Node::Job(job) => job,
                                _ => panic!("Expected job"),
                            };
                            if !matches!(job.state, JobState::Uninitialized) {
                                self.drive_all(drive, &mut jobs, &mut futures, false)?;
                            }
                        }
                    } else {
                        self.mark_complete(completed_job_num, true, true);
                    }
                }
                None => {
                    panic!("Unexpected signal exit of subprocess")
                }
            }
        }

        Ok(())
    }

    async fn check_watcher(
        &mut self,
        rx: &Receiver<RawEvent>,
        jobs: &mut Vec<usize>,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
        blocking: bool,
    ) -> Result<bool, TaskError> {
        let evt = if blocking {
            match rx.recv() {
                Ok(evt) => evt,
                Err(_) => panic!("Watcher disconnected"),
            }
        } else {
            match rx.try_recv() {
                Ok(evt) => evt,
                Err(TryRecvError::Empty) => {
                    return Ok(false);
                }
                Err(TryRecvError::Disconnected) => panic!("Watcher disconnected"),
            }
        };
        if let Some(path) = evt.path {
            match evt.op {
                Ok(Op::REMOVE) | Ok(Op::WRITE) | Ok(Op::CREATE) | Ok(Op::CLOSE_WRITE)
                | Ok(Op::RENAME) => self.invalidate(path, jobs, futures),
                Err(e) => {
                    eprintln!("Watch error: {:?}", e);
                    Ok(false)
                }
                _ => Ok(false),
            }
        } else {
            match evt.op {
                Ok(Op::RESCAN) => {
                    panic!("TODO: Watcher rescan");
                }
                Err(e) => {
                    eprintln!("Watch error: {:?}", e);
                    Ok(false)
                }
                _ => Ok(false),
            }
        }
    }
}

async fn drive_watcher<'a>(
    runner: &mut Runner<'a>,
    rx: &Receiver<RawEvent>,
) -> Result<(), TaskError> {
    let mut jobs: Vec<usize> = Vec::new();
    let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>> = Vec::new();
    loop {
        if runner
            .check_watcher(&rx, &mut jobs, &mut futures, true)
            .await?
        {
            loop {
                while runner
                    .check_watcher(&rx, &mut jobs, &mut futures, false)
                    .await?
                {}
                if futures.len() == 0 {
                    break;
                }
                let (completed, idx, new_futures) = select_all(futures).await;
                futures = new_futures;
                let completed_job_num = jobs[idx];
                jobs.remove(idx);
                match completed.code() {
                    Some(code) => {
                        if code == 0 {
                            runner.mark_complete(completed_job_num, true, false);
                            let job = match &runner.nodes[completed_job_num] {
                                Node::Job(job) => job,
                                _ => panic!("Expected job"),
                            };
                            let drives = job.drives.clone();
                            for drive in drives {
                                let job = match &runner.nodes[drive] {
                                    Node::Job(job) => job,
                                    _ => panic!("Expected job"),
                                };
                                if !matches!(job.state, JobState::Uninitialized) {
                                    runner.drive_all(drive, &mut jobs, &mut futures, true)?;
                                }
                            }
                        } else {
                            runner.mark_complete(completed_job_num, true, true);
                        }
                    }
                    None => {
                        panic!("Unexpected signal exit of subprocess")
                    }
                }
            }
            // println!("Watching...");
        }
    }
}

pub async fn run<'a>(opts: RunOptions<'a>) -> Result<(), TaskError> {
    let chompfile_source = fs::read_to_string(opts.cfg_file).await?;
    let chompfile: Chompfile = toml::from_str(&chompfile_source)?;

    if chompfile.version != 0.1 {
        return Err(TaskError::InvalidVersionError(format!(
            "Invalid chompfile version {}, only 0.1 is supported",
            chompfile.version
        )));
    }

    let mut runner = Runner::new(opts.ui, &chompfile, &opts.cwd);
    let (tx, rx) = channel();
    let mut watcher = raw_watcher(tx).unwrap();

    for target in &opts.targets {
        runner.expand_target(&mut watcher, target, None).await?;
    }

    runner.drive_targets(&opts.targets).await?;

    // block on watcher if watching
    if opts.watch {
        println!("Watching for changes...");
        drive_watcher(&mut runner, &rx).await?;
    }

    Ok(())
}

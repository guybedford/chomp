use crate::chompfile::{
    ChompEngine, ChompTaskMaybeTemplated, ChompTaskMaybeTemplatedNoDefault, ChompTemplate,
    Chompfile, TargetCheck,
};
use crate::engines::CmdPool;
use crate::js::init_js_platform;
use crate::ui::ChompUI;
use async_std::path::Path;
use async_std::process::ExitStatus;
use futures::future::{select_all, Future, FutureExt, Shared};
use notify::op::Op;
use notify::{RawEvent, RecommendedWatcher};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::collections::VecDeque;
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
use crate::js::run_js_fn;
use anyhow::{anyhow, Result};
use derivative::Derivative;

use notify::{raw_watcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

#[derive(Debug)]
pub struct Task {
    name: Option<String>,
    targets: Vec<String>,
    target_check: TargetCheck,
    deps: Vec<String>,
    serial: bool,
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

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
enum JobState {
    Uninitialized,
    Initialized,
    Checking,
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
    serial: bool,
    parents: Vec<usize>,
    state: JobState,
    mtime: Option<Duration>,
    mtime_future: Option<Shared<Pin<Box<dyn Future<Output = Option<Duration>> + Send>>>>,
    targets: Vec<String>,
    start_time_deps: Option<Instant>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    #[derivative(Debug = "ignore")]
    run_future:
        Option<Shared<Pin<Box<dyn Future<Output = (ExitStatus, Option<Duration>)> + Send>>>>,
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

#[derive(Debug, Derivative)]
struct File {
    name: String,
    parents: Vec<usize>,
    state: FileState,
    mtime: Option<Duration>,
    #[derivative(Debug = "ignore")]
    mtime_future: Option<Shared<Pin<Box<dyn Future<Output = Option<Duration>> + Send>>>>,
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
                    eprintln!("Unable to watch {}", self.name);
                }
            };
        }
    }
}

struct Runner<'a> {
    ui: &'a ChompUI,
    cmd_pool: CmdPool,
    chompfile: &'a Chompfile,
    watch: bool,
    tasks: Vec<Task>,

    nodes: Vec<Node>,

    task_jobs: BTreeMap<String, usize>,
    file_nodes: BTreeMap<String, usize>,
    interpolate_nodes: Vec<(String, usize)>,
}

impl<'a> Job {
    fn new(task: usize, serial: bool, interpolate: Option<String>) -> Job {
        Job {
            interpolate,
            task,
            deps: Vec::new(),
            serial,
            parents: Vec::new(),
            state: JobState::Uninitialized,
            targets: Vec::new(),
            mtime: None,
            start_time_deps: None,
            start_time: None,
            end_time: None,
            run_future: None,
            mtime_future: None,
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

    fn init(&mut self) {
        self.state = JobState::Initialized;
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
enum JobOrFileState {
    Job(JobState),
    File(FileState),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct StateTransition {
    job_num: usize,
    state: JobOrFileState,
}

impl StateTransition {
    fn from_job(idx: usize, state: JobState) -> Self {
        StateTransition {
            job_num: idx,
            state: JobOrFileState::Job(state),
        }
    }
    fn from_file(idx: usize, state: FileState) -> Self {
        StateTransition {
            job_num: idx,
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
    fn insert_job(&mut self, idx: usize, state: JobState) -> Option<StateTransition> {
        let transition = StateTransition::from_job(idx, state);
        if self.state_transitions.insert(transition.clone()) {
            Some(transition)
        } else {
            None
        }
    }
    fn insert_file(&mut self, idx: usize, state: FileState) -> Option<StateTransition> {
        let transition = StateTransition::from_file(idx, state);
        if self.state_transitions.insert(transition.clone()) {
            Some(transition)
        } else {
            None
        }
    }
}

impl Default for QueuedStateTransitions {
    fn default() -> Self {
        QueuedStateTransitions::new()
    }
}

// None = NotFound
pub async fn check_target_mtimes(targets: Vec<String>) -> Option<Duration> {
    if targets.len() == 0 {
        return Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
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
                        NotFound => {
                            if let Some(parent) = target_path.parent() {
                                fs::create_dir_all(parent).await.unwrap();
                            }
                            None
                        }
                        _ => panic!("Unknown file error"),
                    },
                }
            }
            .boxed(),
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

impl<'a> Runner<'a> {
    fn new(
        ui: &'a ChompUI,
        chompfile: &'a Chompfile,
        cwd: &'a PathBuf,
        watch: bool,
    ) -> Result<Runner<'a>> {
        let cmd_pool = CmdPool::new(8, cwd.to_str().unwrap().to_string());
        init_js_platform();
        let mut runner = Runner {
            watch,
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
        for template in &chompfile.template {
            // first template wins (and local overrides chomp included)
            if templates.get(&template.name).is_none() {
                templates.insert(&template.name, &template);
            }
        }

        // expand tasks into initial job list
        let mut task_queue: VecDeque<ChompTaskMaybeTemplated> =
            VecDeque::from(runner.chompfile.task.clone());
        let mut cur_name: Option<String> = None;
        let mut cur_deps: Option<Vec<String>> = None;
        let mut cur_targets: Option<Vec<String>> = None;
        let mut cur_env: Option<BTreeMap<String, String>> = None;
        while task_queue.len() > 0 {
            let task = task_queue.pop_front().unwrap();
            if task.template.is_none() {
                let name = if cur_name.is_some() {
                    if task.name.is_some() {
                        return Err(anyhow!("Cannot set name for template as it has a name"));
                    }
                    cur_name.take()
                } else {
                    task.name.clone()
                };
                let mut deps = Vec::new();
                // this ordering means template instantiation deps come first
                if let Some(ref mut cur_deps) = cur_deps.take() {
                    deps.append(cur_deps);
                }
                for dep in task.deps {
                    deps.push(dep);
                }
                let mut targets = Vec::new();
                // this ordering means template instantiation targets come first
                if let Some(ref mut cur_targets) = cur_targets.take() {
                    targets.append(cur_targets);
                }
                if let Some(target) = &task.target {
                    targets.push(target.to_string());
                } else if let Some(task_targets) = &task.targets {
                    for target in task_targets {
                        targets.push(target.to_string());
                    }
                }
                let mut env = BTreeMap::new();
                for (item, value) in &chompfile.env {
                    env.insert(item.to_uppercase(), value.to_string());
                }
                for (item, value) in task.env {
                    env.insert(item.to_uppercase(), value.to_string());
                }
                if let Some(cur_env) = cur_env.take() {
                    for (item, value) in cur_env {
                        env.insert(item.to_uppercase(), value.to_string());
                    }
                }

                let task = Task {
                    name,
                    deps,
                    serial: task.serial.unwrap_or(false),
                    engine: task.engine.unwrap_or_default(),
                    env,
                    run: task.run.clone(),
                    targets,
                    target_check: task.target_check.unwrap_or_default(),
                };

                runner.tasks.push(task);
                runner.add_job(runner.tasks.len() - 1, None)?;
                continue;
            }
            let template = task.template.as_ref().unwrap();
            // evaluate templates into tasks
            if task.engine.is_some()
                || task.run.is_some()
                || task.target_check.is_some()
                || task.serial.is_some()
            {
                return Err(anyhow!("Template invocation does not support overriding 'run', 'engine', 'serial' or 'target_check' fields."));
            }

            let template = match templates.get(template) {
                Some(template) => template,
                None => return Err(anyhow!("Unable to find template {}", template)),
            };
            let mut template_tasks: Vec<ChompTaskMaybeTemplatedNoDefault> =
                run_js_fn(&template.definition, &template.name, &task.args, &chompfile.env)?;
            if template_tasks.len() == 0 {
                continue;
            }
            if let Some(name) = task.name {
                if cur_name.is_some() {
                    return Err(anyhow!("Template does not support name override"));
                }
                cur_name = Some(name);
            }
            if task.deps.len() > 0 {
                if let Some(ref mut cur_deps) = cur_deps {
                    for dep in task.deps {
                        cur_deps.push(dep);
                    }
                } else {
                    cur_deps = Some(task.deps);
                }
            }
            if let Some(target) = task.target {
                if let Some(ref mut cur_targets) = cur_targets {
                    cur_targets.push(target);
                } else {
                    cur_targets = Some(vec![target]);
                }
            } else if let Some(targets) = task.targets {
                if let Some(ref mut cur_targets) = cur_targets {
                    for target in targets {
                        cur_targets.push(target);
                    }
                } else {
                    cur_targets = Some(targets);
                }
            }
            if task.env.len() > 0 {
                if let Some(ref mut cur_env) = cur_env {
                    for (item, value) in task.env {
                        cur_env.insert(item.to_uppercase(), value.to_string());
                    }
                } else {
                    cur_env = Some(Default::default());
                }
            }
            // template functions output a list of tasks
            for template_task in template_tasks.drain(..).rev() {
                task_queue.push_front(ChompTaskMaybeTemplated {
                    name: template_task.name,
                    target: template_task.target,
                    targets: template_task.targets,
                    target_check: template_task.target_check,
                    deps: template_task.deps.unwrap_or_default(),
                    serial: template_task.serial,
                    env: template_task.env.unwrap_or_default(),
                    run: template_task.run,
                    engine: template_task.engine,
                    template: template_task.template,
                    args: template_task.args.unwrap_or_default(),
                });
            }
        }
        Ok(runner)
    }

    fn add_job(&mut self, task_num: usize, interpolate: Option<String>) -> Result<usize> {
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
                    return Err(anyhow!("Already has job {}", name));
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
                        // return Err(anyhow!("Multiple targets pointing to same file {}", file_target));
                    }
                    None => {
                        self.file_nodes.insert(file_target, num);
                    }
                }
            }
        }

        self.nodes
            .push(Node::Job(Job::new(task_num, task.serial, interpolate)));
        return Ok(num);
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

    fn mark_complete(&mut self, job_num: usize, updated: bool, failed: bool) -> Result<()> {
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
            job.run_future = None;
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
                        "✓ {} [{:?} {:?}]",
                        job.display_name(self),
                        end_time - start_time,
                        end_time - start_time_deps
                    );
                }
            } else {
                if failed {
                    println!(
                        "x {} [- {:?}]",
                        job.display_name(self),
                        end_time - start_time_deps
                    );
                } else {
                    println!(
                        "- {} [- {:?}]",
                        job.display_name(self),
                        end_time - start_time_deps
                    );
                }
            }
        } else {
            if let Some(start_time) = job.start_time {
                if failed {
                    println!("x {} [{:?}]", job.display_name(self), end_time - start_time);
                } else {
                    println!("√ {} [{:?}]", job.display_name(self), end_time - start_time);
                }
            } else {
                if failed {
                    return Err(anyhow!("Did not expect failed for cached"));
                }
                println!("● {} [cached]", job.display_name(self));
            }
        }
        {
            let job = self.get_job_mut(job_num).unwrap();
            job.start_time_deps = None;
            Ok(())
        }
    }

    fn invalidate(
        &mut self,
        path: PathBuf,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>>,
        queued: &mut QueuedStateTransitions,
    ) -> Result<bool> {
        let cwd = std::env::current_dir()?;
        let cwd_str = cwd.to_str().unwrap();
        let path_str = path.to_str().unwrap();
        if !path_str.starts_with(cwd_str) {
            return Err(anyhow!("Expected path within cwd"));
        }
        let rel_str = &path_str[cwd_str.len() + 1..];
        let sanitized_path = rel_str.replace("\\", "/");
        match self.file_nodes.get(&sanitized_path) {
            Some(&job_num) => match self.nodes[job_num] {
                Node::Job(_) => panic!("TODO: Job invalidator"),
                Node::File(ref mut file) => {
                    file.mtime = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
                    let parents = file.parents.clone();
                    for parent in parents {
                        self.drive_all(parent, true, futures, queued, None)?;
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
    ) -> Result<Option<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>>> {
        let job = match &self.nodes[job_num] {
            Node::Job(job) => job,
            Node::File(_) => panic!("Expected job"),
        };
        if job.state != JobState::Pending {
            panic!("Expected pending job");
        }
        let task = &self.tasks[job.task];
        // CMD Exec
        if task.run.is_none() {
            self.mark_complete(job_num, false, false)?;
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
                self.mark_complete(job_num, false, false)?;
                return Ok(None);
            }
        }
        // If we have an mtime, check if we need to do work
        if job.targets.len() > 0 {
            if let Some(mtime) = job.mtime {
                let mut all_fresh = true;
                for &dep in job.deps.iter() {
                    let dep_change = match &self.nodes[dep] {
                        Node::Job(dep) => {
                            let invalidated = match dep.mtime {
                                Some(dep_mtime) => match &self.tasks[dep.task].target_check {
                                    TargetCheck::Exists => false,
                                    TargetCheck::Mtime => dep_mtime > mtime,
                                },
                                None => true,
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
                                Some(dep_mtime) => dep_mtime > mtime,
                                None => true,
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
                    self.mark_complete(job_num, false, false)?;
                    return Ok(None);
                }
            }
        }
        println!("🞂 {}", job.display_name(self));

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
            task.deps
                .iter()
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

        let job_future = self
            .cmd_pool
            .run(
                run,
                job.targets.clone(),
                &mut env,
                task.engine,
                self.chompfile.debug,
            )
            .boxed()
            .shared();
        let return_future = job_future.clone();
        let job = self.get_job_mut(job_num).unwrap();
        job.run_future = Some(job_future);
        job.state = JobState::Running;
        job.start_time = Some(Instant::now());
        let transition = StateTransition::from_job(job_num, JobState::Running);
        Ok(Some(
            async move {
                return_future.await;
                transition
            }
            .boxed()
            .shared(),
        ))
    }

    // top-down driver - initiates future starts
    fn drive_all(
        &mut self,
        job_num: usize,
        invalidation: bool,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>>,
        queued: &mut QueuedStateTransitions,
        parent: Option<usize>,
    ) -> Result<JobOrFileState> {
        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if let Some(parent) = parent {
                    if job.parents.iter().find(|&&p| p == parent).is_none() {
                        job.parents.push(parent);
                    }
                }
                if invalidation {
                    match job.state {
                        JobState::Failed | JobState::Fresh => {
                            job.state = JobState::Pending;
                        }
                        JobState::Running => {
                            return Ok(JobOrFileState::Job(JobState::Running));
                        }
                        _ => {}
                    }
                }
                match job.state {
                    JobState::Uninitialized => {
                        panic!("Unexpected uninitialized job");
                    }
                    JobState::Initialized => {
                        job.start_time_deps = Some(Instant::now());
                        let targets = job.targets.clone();
                        let mtime_future = check_target_mtimes(targets).boxed().shared();
                        job.mtime_future = Some(mtime_future.clone());
                        job.state = JobState::Checking;
                        let transition = queued
                            .insert_job(job_num, JobState::Checking)
                            .expect("Expected first job check");
                        futures.push(
                            async move {
                                mtime_future.await;
                                transition
                            }
                            .boxed()
                            .shared(),
                        );
                        Ok(JobOrFileState::Job(JobState::Checking))
                    }
                    JobState::Checking => {
                        let job = self.get_job(job_num).unwrap();
                        if let Some(transition) = queued.insert_job(job_num, JobState::Checking) {
                            let future = job.run_future.clone().unwrap();
                            futures.push(
                                async move {
                                    future.await;
                                    transition
                                }
                                .boxed()
                                .shared(),
                            );
                        }
                        Ok(JobOrFileState::Job(JobState::Checking))
                    }
                    JobState::Pending => {
                        let mut all_completed = true;
                        let job = self.get_job(job_num).unwrap();
                        let serial = job.serial;
                        let deps = job.deps.clone();

                        // TODO: Use a driver counter for deps
                        for dep in deps {
                            let dep_state =
                                self.drive_all(dep, false, futures, queued, Some(job_num))?;
                            match dep_state {
                                JobOrFileState::Job(JobState::Fresh)
                                | JobOrFileState::File(FileState::Found) => {}
                                JobOrFileState::Job(JobState::Failed)
                                | JobOrFileState::File(FileState::NotFound) => {
                                    // Serial propagates any dep error to parent
                                    if serial {
                                        self.mark_complete(job_num, false, true)?;
                                        self.drive_completion(
                                            StateTransition::from_job(job_num, JobState::Running),
                                            invalidation,
                                            futures,
                                            queued,
                                        )?;
                                        return Ok(JobOrFileState::Job(JobState::Failed));
                                    }
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
                            return match self.run_job(job_num)? {
                                Some(future) => {
                                    match queued.insert_job(job_num, JobState::Running) {
                                        Some(_) => futures.push(future),
                                        None => {}
                                    }
                                    Ok(JobOrFileState::Job(JobState::Running))
                                }
                                None => {
                                    self.drive_completion(
                                        StateTransition::from_job(job_num, JobState::Running),
                                        invalidation,
                                        futures,
                                        queued,
                                    )?;
                                    Ok(JobOrFileState::Job(JobState::Fresh))
                                }
                            };
                        }
                        Ok(JobOrFileState::Job(JobState::Pending))
                    }
                    JobState::Running => {
                        let job = self.get_job(job_num).unwrap();
                        if let Some(transition) = queued.insert_job(job_num, JobState::Running) {
                            let future = job.run_future.clone().unwrap();
                            futures.push(
                                async move {
                                    future.await;
                                    transition
                                }
                                .boxed()
                                .shared(),
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
                                    _ => panic!("Unknown file error"),
                                },
                            }
                        }
                        .boxed()
                        .shared();
                        file.mtime_future = Some(mtime_future.clone());
                        file.state = FileState::Checking;
                        let transition = queued
                            .insert_file(job_num, FileState::Checking)
                            .expect("Expected first file check");
                        futures.push(
                            async move {
                                mtime_future.await;
                                transition
                            }
                            .boxed()
                            .shared(),
                        );
                        Ok(JobOrFileState::File(FileState::Checking))
                    }
                    FileState::Checking => {
                        if let Some(transition) = queued.insert_file(job_num, FileState::Checking) {
                            let future = file.mtime_future.clone().unwrap();
                            futures.push(
                                async move {
                                    future.await;
                                    transition
                                }
                                .boxed()
                                .shared(),
                            );
                        }
                        Ok(JobOrFileState::File(FileState::Checking))
                    }
                    FileState::Found => Ok(JobOrFileState::File(FileState::Found)),
                    FileState::NotFound => {
                        if !self.watch {
                            return Err(anyhow!("Task {} not found", file.name));
                        } else {
                            // dbg!(file);
                            panic!("TODO: NON-EXISTING FILE WATCH");
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
        invalidation: bool,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>>,
        queued: &mut QueuedStateTransitions,
    ) -> Result<()> {
        // drives the completion of a state transition to subsequent transitions
        let job_num = transition.job_num;
        match transition.state {
            JobOrFileState::Job(JobState::Checking) => {
                let job = self.get_job_mut(job_num).unwrap();
                job.state = JobState::Pending;
                let mtime_future = job.mtime_future.as_ref().unwrap();
                let mtime = mtime_future.peek().unwrap();
                job.mtime = mtime.clone();
                job.mtime_future = None;
                self.drive_all(job_num, invalidation, futures, queued, None)?;
                Ok(())
            }
            JobOrFileState::Job(JobState::Running) => {
                // job can complete running without an exec promise if eg cached
                let job = self.get_job(job_num).unwrap();
                if let Some(ref run_future) = job.run_future {
                    let (status, mtime) = run_future.peek().unwrap();
                    let success = status.success() && mtime.is_some();
                    self.mark_complete(job_num, true, !success)?;
                }
                let job = self.get_job(job_num).unwrap();
                if matches!(job.state, JobState::Fresh | JobState::Failed) {
                    for parent in job.parents.clone() {
                        self.drive_all(parent, invalidation, futures, queued, None)?;
                    }
                }
                Ok(())
            }
            JobOrFileState::File(FileState::Checking) => {
                let file = match self.nodes[job_num] {
                    Node::File(ref mut file) => file,
                    _ => panic!("Expected file"),
                };
                let mtime_future = file.mtime_future.as_ref().unwrap();
                let mtime = mtime_future.peek().unwrap();
                file.mtime = mtime.clone();
                file.mtime_future = None;
                file.state = match file.mtime {
                    Some(_mtime) => FileState::Found,
                    None => FileState::NotFound,
                };
                for parent in file.parents.clone() {
                    self.drive_all(parent, invalidation, futures, queued, None)?;
                }
                Ok(())
            }
            _ => {
                dbg!(transition.state);
                panic!("Unexpected promise transition state");
            }
        }
    }

    #[async_recursion(?Send)]
    async fn lookup_target(
        &mut self,
        watcher: &mut RecommendedWatcher,
        target: &str,
        as_task: bool,
    ) -> Result<usize> {
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
                        let task_deps = &self.tasks[self.get_job(job_num).unwrap().task].deps;
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
                    None => Ok(self.add_file(String::from(target))?),
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
    ) -> Result<()> {
        let job_num = self.lookup_target(watcher, target, true).await?;
        self.expand_job(watcher, job_num, drives).await
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
                if matches!(job.state, JobState::Pending) {
                    if let Some(parent) = parent {
                        job.parents.push(parent);
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
                        return Err(anyhow!("Cannot have wildcard + interpolate"));
                    }
                    job_targets.push(target.to_string());
                }
                if !is_interpolate {
                    job.targets = job_targets;
                }

                // this must come after setting target above
                job.init();

                if is_wildcard {
                    panic!("TODO: wildcard targets");
                }

                let mut expanded_interpolate = false;
                let deps = task.deps.clone();
                for dep in deps {
                    if dep.contains('#') {
                        if dep.contains('*') {
                            return Err(anyhow!("Wildcard + interpolate not supported"));
                        }
                        if !is_interpolate {
                            return Err(anyhow!("Interpolate in deps can only be used when contained in target (and run)"));
                        }
                        if expanded_interpolate {
                            return Err(anyhow!("Only one interpolated deps is allowed"));
                        }
                        self.expand_interpolate(watcher, String::from(dep), job_num, task_num)
                            .await?;
                        expanded_interpolate = true;
                    } else if dep.contains('*') {
                        panic!("TODO: Wildcard deps");
                    } else {
                        self.expand_target(watcher, &String::from(dep), Some(job_num))
                            .await?;
                    }
                }
                if is_interpolate {
                    if !expanded_interpolate {
                        return Err(anyhow!("Never found deps interpolates"));
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
        let interpolate_idx = dep.find("#").unwrap();
        if dep[interpolate_idx + 1..].find("#").is_some() {
            return Err(anyhow!("multiple interpolates"));
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
                    return Err(anyhow!("GLOB ERROR"));
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
    ) -> Result<usize> {
        let watch = self.watch;
        let job_num = self.add_job(parent_task, Some(String::from(interpolate)))?;
        let file_num = self.add_file(input.to_string())?;
        {
            let file = self.get_file_mut(file_num).unwrap();
            file.init(if watch { Some(watcher) } else { None });
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
            job.init();
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
    async fn drive_targets(&mut self, targets: &Vec<String>) -> Result<()> {
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>> =
            Vec::new();

        let mut queued = QueuedStateTransitions::new();

        if self.chompfile.debug {
            dbg!(&self.task_jobs);
            dbg!(&self.nodes);
        }

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

            self.drive_all(job_num, false, &mut futures, &mut queued, None)?;
        }

        while futures.len() > 0 {
            let (transition, _idx, new_futures) = select_all(futures).await;
            futures = new_futures;
            self.drive_completion(transition, false, &mut futures, &mut queued)?;
        }

        Ok(())
    }

    async fn check_watcher(
        &mut self,
        rx: &Receiver<RawEvent>,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>>,
        queued: &mut QueuedStateTransitions,
        blocking: bool,
    ) -> Result<bool> {
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
                | Ok(Op::RENAME) => self.invalidate(path, futures, queued),
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

async fn drive_watcher<'a>(runner: &mut Runner<'a>, rx: &Receiver<RawEvent>) -> Result<()> {
    loop {
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = StateTransition> + Send>>>> =
            Vec::new();
        let mut queued = QueuedStateTransitions::new();
        if runner
            .check_watcher(&rx, &mut futures, &mut queued, true)
            .await?
        {
            loop {
                while runner
                    .check_watcher(&rx, &mut futures, &mut queued, false)
                    .await?
                {}
                if futures.len() == 0 {
                    break;
                }
                let (transition, _idx, new_futures) = select_all(futures).await;
                futures = new_futures;
                runner.drive_completion(transition, true, &mut futures, &mut queued)?;
            }
            // println!("Watching...");
        }
    }
}

pub async fn run<'a>(chompfile: &Chompfile, opts: RunOptions<'a>) -> Result<bool> {
    let mut runner = Runner::new(opts.ui, &chompfile, &opts.cwd, opts.watch)?;
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

    // if all targets completed successfully, exit code is 0, otherwise its an error
    let mut all_ok = true;
    for target in &opts.targets {
        let job_num = runner.lookup_target(&mut watcher, target, true).await?;
        let task = runner.get_job(job_num).unwrap();
        if !matches!(task.state, JobState::Fresh) {
            all_ok = false;
            break;
        }
    }

    Ok(all_ok)
}

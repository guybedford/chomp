use std::io::ErrorKind::NotFound;
use std::time::Duration;
use std::time::UNIX_EPOCH;
use crate::cmd::CmdPool;
use async_std::process::ExitStatus;
use futures::future::{select_all, Future, FutureExt, Shared};
use std::collections::BTreeMap;
extern crate num_cpus;
use async_recursion::async_recursion;
use capturing_glob::glob;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Instant;
use async_std::fs;

use derivative::Derivative;

#[derive(Debug, Serialize, Deserialize)]
struct Chompfile {
    version: f32,
    task: Option<Vec<ChompTask>>,
    group: Option<BTreeMap<String, BTreeMap<String, ChompTask>>>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
struct ChompTask {
    name: Option<String>,
    target: Option<String>,
    deps: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    run: Option<String>,
}

pub struct RunOptions {
    pub cwd: PathBuf,
    pub cfg_file: PathBuf,
    pub targets: Vec<String>,
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
    Pending,
    Running,
    Fresh,
    Failed,
}

#[derive(Debug, Derivative)]
struct Job<'a> {
    // task index
    idx: usize,
    interpolate: Option<String>,
    task: &'a ChompTask,
    deps: Vec<usize>,
    drives: Vec<usize>,
    state: JobState,
    mtime: Option<Duration>,
    target: Option<String>,
    start_time_deps: Option<Instant>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    #[derivative(Debug = "ignore")]
    future: Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
    live: bool,
}

#[derive(Debug)]
enum Node<'a> {
    Job(Job<'a>),
    File(File),
}

#[derive(Debug)]
enum FileState {
    Uninitialized,
    Fresh,
    Changed,
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
}

struct Runner<'a> {
    cmd_pool: CmdPool,
    chompfile: &'a Chompfile,

    nodes: Vec<Node<'a>>,

    task_jobs: BTreeMap<String, usize>,
    file_nodes: BTreeMap<String, usize>,
    interpolate_nodes: Vec<(String, usize)>,
}

impl<'a> Job<'a> {
    fn new(idx: usize, task: &'a ChompTask, interpolate: Option<String>) -> Job<'a> {
        Job {
            idx,
            interpolate,
            task,
            deps: Vec::new(),
            drives: Vec::new(),
            state: JobState::Uninitialized,
            target: None,
            mtime: None,
            start_time_deps: None,
            start_time: None,
            end_time: None,
            future: None,
            live: false,
        }
    }

    fn display_name(&self) -> String {
        match &self.target {
            Some(target) => {
                if target.contains("#") {
                    target.replace("#", &self.interpolate.as_ref().unwrap())
                }
                else {
                    String::from(target)
                }
            },
            _ => match &self.task.name {
                Some(name) => String::from(format!(":{}", name)),
                None => match &self.task.run {
                    Some(run) => String::from(format!("{}", run)),
                    None => String::from(format!("[task {}]", self.idx)),
                },
            }
        }
    }
}

impl<'a> Runner<'a> {
    fn new(chompfile: &'a Chompfile, cwd: &'a PathBuf) -> Runner<'a> {
        let cmd_pool = CmdPool::new(8, cwd.to_str().unwrap().to_string());
        Runner {
            cmd_pool,
            chompfile,
            nodes: Vec::new(),
            task_jobs: BTreeMap::new(),
            file_nodes: BTreeMap::new(),
            interpolate_nodes: Vec::new(),
        }
    }

    fn add_job(&mut self, idx: usize, task: &'a ChompTask, interpolate: Option<String>) -> usize {
        let num = self.nodes.len();
        self.nodes.push(Node::Job(Job::new(idx, task, interpolate)));
        return num;
    }

    fn add_file(&mut self, file: String) -> usize {
        let num = self.nodes.len();
        self.nodes.push(Node::File(File::new(file)));
        return num;
    }

    fn get_job(&self, num: usize) -> Option<&Job> {
        match self.nodes[num] {
            Node::Job(ref job) => Some(job),
            _ => None,
        }
    }

    fn get_job_mut(&mut self, num: usize) -> Option<&mut Job<'a>> {
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

    fn initialize_tasks(&mut self) {
        // expand all tasks into all jobs
        if let Some(tasks) = &self.chompfile.task {
            for (idx, task) in tasks.iter().enumerate() {
                let job_num = self.add_job(idx, task, None);

                // map task name to task job
                if let Some(name) = &task.name {
                    if self.task_jobs.contains_key(name) {
                        panic!("Already has job");
                    }
                    self.task_jobs.insert(name.to_string(), job_num);
                }

                if task.target.is_none() {
                    continue;
                }
                let target = task.target.as_ref().unwrap();
                if target.contains("#") {
                    self.interpolate_nodes.push((target.to_string(), job_num));
                }
                else {
                    match self.file_nodes.get(target) {
                        Some(_) => {
                            panic!("Multiple targets pointing to same file");
                        }
                        None => {
                            self.file_nodes.insert(target.to_string(), job_num);
                        }
                    }
                }
            }
        }
    }

    fn mark_complete(&mut self, job_num: usize, failed: bool) {
        let job = self.get_job_mut(job_num).unwrap();
        job.end_time = Some(Instant::now());
        job.state = if failed {
            JobState::Failed
        } else {
            JobState::Fresh
        };
        job.future = None;
        if let Some(start_time) = job.start_time {
            println!(
                "√ {} [{:?}, {:?} with subtasks]",
                job.display_name(),
                job.end_time.unwrap() - start_time,
                job.end_time.unwrap() - job.start_time_deps.unwrap()
            );
        } else {
            println!(
                "● {} [cached]",
                job.display_name(),
            );
        }
    }

    fn run_job(
        &mut self,
        job_num: usize,
    ) -> Result<Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>, TaskError> {
        let job = match &self.nodes[job_num] {
            Node::Job(job) => job,
            Node::File(_) => panic!("Expected job")
        };
        // CMD Exec
        if job.task.run.is_none() {
            self.mark_complete(job_num, false);
            return Ok(None);
        }
        // the interpolation template itself is not run
        if job.task.target.as_ref().unwrap().contains("#") && job.interpolate.is_none() {
            self.mark_complete(job_num, false);
            return Ok(None);
        }
        // If we have an mtime, check if we need to do work
        if let Some(mtime) = job.mtime {
            let mut all_fresh = true;
            for dep in job.deps.iter() {
                let dep_change = match &self.nodes[*dep] {
                    Node::Job(dep) => match dep.mtime {
                        Some(dep_mtime) if dep_mtime > mtime => true,
                        None => true,
                        _ => false,
                    },
                    Node::File(dep) => {
                        match dep.mtime {
                            Some(dep_mtime) if dep_mtime > mtime => true,
                            None => true,
                            _ => false,
                        }
                    }
                };
                if dep_change {
                    all_fresh = false;
                    break;
                }
            }
            if all_fresh {
                self.mark_complete(job_num, false);
                return Ok(None);
            }
        }
        println!("○ {}", job.display_name());

        let mut run: String = job.task.run.as_ref().unwrap().to_string();
        if let Some(interpolate) = &job.interpolate {
            run = run.replace("#", interpolate);
        }
        let future = self.cmd_pool.run(&run, &job.task.env);
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
    ) -> Result<bool, TaskError> {
        match self.nodes[job_num] {
            Node::Job(ref job) => match job.state {
                JobState::Uninitialized => {
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
                    job.live = true;
                    let deps = job.deps.clone();
                    for dep in deps {
                        let completed = self.drive_all(dep, jobs, futures)?;
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
            },
            Node::File(ref mut file) => {
                if file.mtime.is_some() {
                    file.state = FileState::Fresh;
                    Ok(true)
                }
                else {
                    dbg!(file);
                    panic!("TODO: NON-EXISTING FILE WATCH");
                }
            }
        }
    }

    fn lookup_target(&mut self, target: &str) -> Result<usize, TaskError> {
        let name = if target.as_bytes()[0] as char == ':' {
            &target[1..]
        } else {
            &target
        };

        // First match task by name
        match self.task_jobs.get(name) {
            Some(&job_num) => Ok(job_num),
            // Then by exact file name
            None => match self.file_nodes.get(name) {
                Some(&job_num) => Ok(job_num),
                // Then by interpolate
                None => {
                    println!("INTERPOLATE CHECK");
                    let mut interpolate_match = None;
                    let mut interpolate_lhs_match_len = 0;
                    let mut interpolate_rhs_match_len = 0;
                    for (interpolate, job_num) in &self.interpolate_nodes {
                        let interpolate_idx = interpolate.find("#").unwrap();
                        let lhs = &interpolate[0..interpolate_idx];
                        let rhs = &interpolate[interpolate_idx + 1..];
                        if name.starts_with(lhs) && name.len() > lhs.len() + rhs.len() && name.ends_with(rhs) {
                            interpolate_match = Some(job_num);
                            if (lhs.len() >= interpolate_lhs_match_len && rhs.len() > interpolate_rhs_match_len) {
                                interpolate_lhs_match_len = lhs.len();
                                interpolate_rhs_match_len = rhs.len();
                            }
                        }
                    }
                    match interpolate_match {
                        Some(&job_num) => {
                            panic!("INTERPOLATE MATCH {}", job_num);
                            // job_num
                        },
                        // Otherwise add as a file dependency
                        None => Ok(self.add_file(String::from(name)))
                    }
                },
            },
        }
    }

    // expand out the full job graph for the given targets
    #[async_recursion]
    async fn expand_target(
        &mut self,
        target: &str,
        drives: Option<usize>,
    ) -> Result<(), TaskError> {
        let job_num = self.lookup_target(target)?;

        if let Some(drives) = drives {
            self.get_job_mut(drives).unwrap().deps.push(job_num);   
        }

        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if let Some(drives) = drives {
                    job.drives.push(drives);
                }
                if matches!(job.state, JobState::Pending) {
                    return Ok(());
                }

                let idx = job.idx;

                let mut is_interpolate = false;
                let mut is_wildcard = false;

                job.start_time_deps = Some(Instant::now());
                job.state = JobState::Pending;
                if let Some(target) = &job.task.target {
                    is_interpolate = target.contains("#");
                    is_wildcard = target.contains("*");
                    if is_wildcard && is_interpolate {
                        panic!("Cannot have wildcard + interpolate");
                    }
                    if !target.contains("#") {
                        job.target = Some(target.to_string());
                    }
                    job.mtime = match fs::metadata(target).await {
                        Ok(n) => Some(n.modified()?.duration_since(UNIX_EPOCH).unwrap()),
                        Err(e) => match e.kind() {
                            NotFound => None,
                            _ => panic!("Unknown file error"),
                        },
                    };
                };

                if is_wildcard {
                    panic!("TODO: wildcard targets");
                }

                let deps_cloned = match &job.task.deps {
                    Some(deps) => Some(deps.clone()),
                    None => None,
                };
                let mut expanded_interpolate = false;
                if let Some(deps_cloned) = deps_cloned {
                    for dep in deps_cloned {
                        if dep.contains("#") {
                            if !is_interpolate {
                                panic!("Interpolate in deps can only be used when contained in target (and run)");
                            }
                            self.expand_interpolate(String::from(dep), job_num, idx).await?;
                            expanded_interpolate = true;
                        }
                        else {
                            self.expand_target(&String::from(dep), Some(job_num)).await?;
                        }
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
                if target.contains("*") {
                    dbg!(target);
                    panic!("TODO: wildcard deps");
                }
                file.mtime = match fs::metadata(target).await {
                    Ok(n) => Some(n.modified()?.duration_since(UNIX_EPOCH).unwrap()),
                    Err(e) => match e.kind() {
                        NotFound => None,
                        _ => panic!("Unknown file error"),
                    }
                    _ => panic!("Unknown file error"),
                };
            }
        }
        Ok(())
    }

    async fn expand_interpolate(&mut self, dep: String, parent_job: usize, parent_task_idx: usize) -> Result<(), TaskError> {
        let parent = &self.chompfile.task.as_ref().unwrap()[parent_task_idx];
        let parent_target = parent.target.as_ref().unwrap();
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
                    let input_path = String::from(entry.path().to_str().unwrap()).replace("\\", "/");
                    let interpolate = &input_path[interpolate_idx..input_path.len() - dep.len() + interpolate_idx + 1];
                    let job_num = self.add_job(parent_task_idx, parent, Some(String::from(interpolate)));
                    let file_num = self.add_file(input_path.to_string());
                    {
                        let file = self.get_file_mut(file_num).unwrap();
                        file.drives.push(job_num);
                        file.mtime = match fs::metadata(&input_path).await {
                            Ok(n) => Some(n.modified()?.duration_since(UNIX_EPOCH).unwrap()),
                            Err(e) => match e.kind() {
                                NotFound => None,
                                _ => panic!("Unknown file error"),
                            }
                            _ => panic!("Unknown file error"),
                        };
                    }
                    let job = self.get_job_mut(job_num).unwrap();
                    job.deps.push(file_num);
                    let output_path = parent_target.replace("#", interpolate);
                    job.target = Some(output_path.to_string());
                    job.state = JobState::Pending;
                    job.start_time_deps = Some(Instant::now());
                    job.drives.push(parent_job);
                    job.mtime = match fs::metadata(output_path).await {
                        Ok(n) => Some(n.modified()?.duration_since(UNIX_EPOCH).unwrap()),
                        Err(e) => match e.kind() {
                            NotFound => None,
                            _ => panic!("Unknown file error"),
                        },
                    };

                    let parent = self.get_job_mut(parent_job).unwrap();
                    parent.deps.push(job_num);
                    for dep in parent.task.deps.as_ref().unwrap() {
                        if !dep.contains("#") {
                            let parent_job = self.lookup_target(&dep)?;
                            let job = self.get_job_mut(job_num).unwrap();
                            job.deps.push(parent_job);
                        }
                    }
                },
                Err(e) => {
                    eprintln!("{:?}", e);
                    panic!("GLOB ERROR");
                }
            }
        }
        Ok(())
    }

    // find the job for the target, and drive its completion
    async fn drive_targets(&mut self, targets: &Vec<String>) -> Result<(), TaskError> {
        let mut jobs: Vec<usize> = Vec::new();
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>> = Vec::new();

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
                        panic!("TODO: target not found error");
                    }
                },
            };

            self.drive_all(job_num, &mut jobs, &mut futures)?;
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
                        self.mark_complete(completed_job_num, false);
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
                            if job.live {
                                self.drive_all(drive, &mut jobs, &mut futures)?;
                            }
                        }
                    } else {
                        self.mark_complete(completed_job_num, true);
                    }
                }
                None => {
                    panic!("Unexpected signal exit of subprocess")
                }
            }
        }

        Ok(())
    }
}

pub async fn run(opts: RunOptions) -> Result<(), TaskError> {
    let chompfile_source = fs::read_to_string(opts.cfg_file).await?;
    let chompfile: Chompfile = toml::from_str(&chompfile_source)?;

    if chompfile.version != 0.1 {
        return Err(TaskError::InvalidVersionError(format!(
            "Invalid chompfile version {}, only 0.1 is supported",
            chompfile.version
        )));
    }

    let mut runner = Runner::new(&chompfile, &opts.cwd);

    runner.initialize_tasks();

    for target in &opts.targets {
        runner.expand_target(target, None).await?;
    }

    runner.drive_targets(&opts.targets).await?;

    Ok(())
}

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
    pub target: Vec<String>,
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
    idx: usize,
    task: &'a ChompTask,
    deps: Vec<usize>,
    drives: Vec<usize>,
    state: JobState,
    mtime: Option<Duration>,
    target: Option<&'a str>,
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
}

impl<'a> Job<'a> {
    fn new(idx: usize, task: &'a ChompTask) -> Job<'a> {
        Job {
            idx,
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
        match self.target {
            Some(target) => String::from(target),
            None => match &self.task.name {
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
        }
    }

    fn add_job(&mut self, idx: usize, task: &'a ChompTask) -> usize {
        let num = self.nodes.len();
        self.nodes.push(Node::Job(Job::new(idx, task)));
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

    fn initialize_tasks(&mut self) {
        // expand all tasks into all jobs
        if let Some(tasks) = &self.chompfile.task {
            for (idx, task) in tasks.iter().enumerate() {
                let job_num = self.add_job(idx, task);

                // map task name to task job
                if let Some(name) = &task.name {
                    if self.task_jobs.contains_key(name) {
                        panic!("Already has job");
                    }
                    self.task_jobs.insert(name.to_string(), job_num);
                }

                // if a file target, set to file job
                if let Some(target) = &task.target {
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
                    Node::File(dep) => match dep.mtime {
                        Some(dep_mtime) if dep_mtime > mtime => true,
                        None => true,
                        _ => false,
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

        let run: &str = job.task.run.as_ref().unwrap();
        let future = self.cmd_pool.run(run, &job.task.env);
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
    ) -> Result<JobState, TaskError> {
        match &self.nodes[job_num] {
            Node::Job(job) => match job.state {
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
                        Ok(JobState::Running)
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
                        let dep_state = self.drive_all(dep, jobs, futures)?;
                        match dep_state {
                            JobState::Fresh => {}
                            _ => {
                                all_completed = false;
                            }
                        }
                    }
                    // deps all completed -> execute this job
                    if all_completed {
                        return match self.run_job(job_num)? {
                            Some(future) => {
                                futures.push(future);
                                jobs.push(job_num);
                                Ok(JobState::Running)
                            }
                            None => {
                                // already complete -> skip straight to driving parents
                                // let drives = self.get_job(job_num).unwrap().drives.clone();
                                // for drive in drives {
                                //     if self.get_job(job_num).unwrap().live {
                                //         self.drive_all(drive, jobs, futures)?;
                                //     }
                                // }
                                Ok(JobState::Fresh)
                            }
                        };
                    }
                    Ok(JobState::Pending)
                }
                JobState::Failed => Ok(JobState::Failed),
                JobState::Fresh => Ok(JobState::Fresh),
            },
            Node::File(file) => {
                panic!("FILE DRIVE");
            }
        }
    }

    // expand out the full job graph for the given targets
    #[async_recursion]
    async fn expand_target(
        &mut self,
        target: &str,
        drives: Option<usize>,
    ) -> Result<(), TaskError> {
        let name = if target.as_bytes()[0] as char == ':' {
            &target[1..]
        } else {
            &target
        };

        let job_num = match self.task_jobs.get(name) {
            Some(&job_num) => job_num,
            None => match self.file_nodes.get(name) {
                Some(&job_num) => job_num,
                // no target found -> create a new file job for it
                None => self.add_file(String::from(name)),
            },
        };

        if let Some(drives) = drives {
            self.get_job_mut(drives).unwrap().deps.push(job_num);   
        }

        match self.nodes[job_num] {
            Node::Job(ref mut job) => {
                if let Some(drives) = drives {
                    job.drives.push(drives);
                }
                if let Some(deps) = &job.task.deps {
                    let deps_cloned = deps.clone();
                    for dep in deps_cloned {
                        self.expand_target(&String::from(dep), Some(job_num)).await?;
                    }
                }

                let job = self.get_job_mut(job_num).unwrap();

                if let Some(target) = &job.task.target {
                    job.target = Some(target);
                    job.mtime = match fs::metadata(target).await {
                        Ok(n) => Some(n.modified()?.duration_since(UNIX_EPOCH).unwrap()),
                        Err(e) => match e.kind() {
                            NotFound => None,
                            _ => panic!("Unknown file error"),
                        }
                        _ => panic!("Unknown file error"),
                    }
                };

                job.start_time_deps = Some(Instant::now());
                job.state = JobState::Pending;
            }
            Node::File(ref mut file) => {
                if let Some(drives) = drives {
                    file.drives.push(drives);
                }

                file.state = FileState::Uninitialized;
            }
        }
        // // dbg!(&self.task_jobs);

        // for entry in glob("/media/**/(*).jpg").expect("Failed to read glob pattern") {
        //     match entry {
        //         Ok(entry) => println!("Path {:?}, name {:?}",
        //             entry.path().display(), entry.group(1).unwrap()),
        //         Err(e) => println!("{:?}", e),
        //     }
        // }
        Ok(())
    }

    // find the job for the target, and drive its completion
    async fn drive_targets(&mut self, targets: &Vec<String>) -> Result<(), TaskError> {
        let mut jobs: Vec<usize> = Vec::new();
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>> = Vec::new();

        // dbg!(&self.nodes);

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

    for target in &opts.target {
        runner.expand_target(target, None).await?;
    }

    runner.drive_targets(&opts.target).await?;

    Ok(())
}

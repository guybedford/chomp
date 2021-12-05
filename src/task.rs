use async_std::process::Child;
use crate::cmd::CmdPool;
use async_std::process::Command;
use async_std::fs;
use async_std::process::{ExitStatus};
use futures::future::{select_all, Future, Shared, FutureExt};
use std::collections::BTreeMap;
extern crate num_cpus;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Instant};

use capturing_glob::glob;
use serde::{Deserialize, Serialize};

use derivative::{Derivative};

use crate::cmd;

#[derive(Debug, Serialize, Deserialize)]
struct Chompfile {
    version: f32,
    task: Option<BTreeMap<String, ChompTask>>,
    group: Option<BTreeMap<String, BTreeMap<String, ChompTask>>>,
}

#[derive(Debug, Serialize, PartialEq, Deserialize)]
struct ChompTask {
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
    Initialized,
    Running,
    Completed,
    Failed,
}

#[derive(Derivative)]
struct Job<'a> {
    name: &'a str,
    task: &'a ChompTask,
    deps: Vec<usize>,
    drives: Vec<usize>,
    state: JobState,
    target_mtime: Option<Instant>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    #[derivative(Debug="ignore")]
    future: Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
    live: bool,
}

struct Runner<'a> {
    cmd_pool: CmdPool,
    chompfile: &'a Chompfile,

    jobs: Vec<Job<'a>>,

    task_jobs: BTreeMap<String, usize>,
    file_jobs: BTreeMap<String, usize>,
}

impl<'a> Job<'a> {
    fn new(name: &'a str, task: &'a ChompTask) -> Job<'a> {
        Job {
            name,
            task,
            deps: Vec::new(),
            drives: Vec::new(),
            state: JobState::Uninitialized,
            target_mtime: None,
            start_time: None,
            end_time: None,
            future: None,
            live: false,
        }
    }
}

impl<'a> Runner<'a> {
    fn new(chompfile: &'a Chompfile, cwd: &'a PathBuf) -> Runner<'a> {
        let cmd_pool = CmdPool::new(8, cwd.to_str().unwrap().to_string());
        Runner {
            cmd_pool,
            chompfile,
            jobs: Vec::new(),
            task_jobs: BTreeMap::new(),
            file_jobs: BTreeMap::new(),
        }
    }

    fn add_job(&mut self, name: &'a str, task: &'a ChompTask) -> usize {
        let num = self.jobs.len();
        self.jobs.push(Job::new(name, task));
        return num;
    }

    fn initialize_tasks(&mut self) -> Result<(), TaskError> {
        // expand all tasks into all jobs
        if let Some(tasks) = self.chompfile.task.as_ref() {
            for (name, task) in tasks {
                let job_num = self.add_job(&name, &task);

                // map task name to task job
                if self.task_jobs.contains_key(name) {
                    panic!("Already has job");
                }
                self.task_jobs.insert(name.to_string(), job_num);

                // if a file target, set to file job
                if let Some(target) = task.target.as_ref() {
                    match self.file_jobs.get(target) {
                        Some(_) => {
                            panic!("Multiple targets pointing to same file");
                        }
                        None => {
                            self.file_jobs.insert(target.to_string(), job_num);
                        }
                    }
                }
            }
        }

        // loop through each job, and patch their deps and drives
        let mut i = 0;
        'outer: while i < self.jobs.len() {
            if let Some(task_deps) = self.jobs[i].task.deps.as_ref() {
                for dep in task_deps {
                    if dep.as_bytes()[0] as char == ':' {
                        match self.task_jobs.get(&dep[1..]) {
                            Some(&task_job) => {
                                self.jobs[i].deps.push(task_job);
                                self.jobs[task_job].drives.push(i);
                            }
                            None => {
                                self.jobs[i].state = JobState::Failed;
                                i += 1;
                                // (TaskError::TaskNotFound(
                                //     dep[1..].to_string(),
                                //     name.to_string(),
                                // ));
                                continue 'outer;
                            }
                        };
                    } else {
                        match self.file_jobs.get(dep) {
                            Some(&file_job) => {
                                self.jobs[i].deps.push(file_job);
                                self.jobs[file_job].drives.push(i);
                            }
                            None => {
                                eprintln!("Task Link Failure: No file job for \"{}\" in \"{}\", perhaps you meant to write \":{}\"?", dep, self.jobs[i].name, dep);
                                self.jobs[i].state = JobState::Failed;
                                i += 1;
                                // (TaskError::TaskNotFound(
                                //     dep[1..].to_string(),
                                //     name.to_string(),
                                // ));
                                continue 'outer;
                            }
                        }
                    }
                }
            }

            let job = &mut self.jobs[i];
            job.start_time = Some(Instant::now());
            job.state = JobState::Initialized;
            i += 1;
        }

        Ok(())
    }

    async fn expand_targets(&mut self, targets: &Vec<String>) -> Result<(), TaskError> {
        // for entry in glob("/media/**/(*).jpg").expect("Failed to read glob pattern") {
        //     match entry {
        //         Ok(entry) => println!("Path {:?}, name {:?}",
        //             entry.path().display(), entry.group(1).unwrap()),
        //         Err(e) => println!("{:?}", e),
        //     }
        // }
        Ok(())
    }

    fn mark_complete (&mut self, job_num: usize, failed: bool) {
        let job = &mut self.jobs[job_num];
        job.end_time = Some(Instant::now());
        job.state = if failed { JobState::Failed } else { JobState::Completed };
        job.future = None;
        println!(
            "  √ {} ◌ [{:?}]",
            job.name,
            job.end_time.unwrap() - job.start_time.unwrap()
        );
    }

    fn run_job(
        &mut self,
        job_num: usize,
    ) -> Result<Option<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>, TaskError>
    {
        let job = &mut self.jobs[job_num];
        // CMD Exec
        if job.task.run.is_none() {
            self.mark_complete(job_num, false);
            return Ok(None);
        }
        println!("  ● {}", job.name);
        let run: &str = job.task.run.as_ref().unwrap();
        let future = self.cmd_pool.run(run, &job.task.env);
        job.future = Some(future.boxed().shared());
        job.state = JobState::Running;
        Ok(Some(job.future.clone().unwrap()))
    }

    fn drive_all(
        &mut self,
        job_num: usize,
        jobs: &mut Vec<usize>,
        futures: &mut Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>>,
    ) -> Result<JobState, TaskError> {
        return match self.jobs[job_num].state {
            JobState::Uninitialized => {
                panic!("Expected initialized job");
            }
            JobState::Running => {
                if let Some(future) = &mut self.jobs[job_num].future {
                    if !jobs.contains(&job_num) {
                        jobs.push(job_num);
                        futures.push(future.clone());
                    }
                    Ok(JobState::Running)
                } else {
                    panic!("Unexpected internal state");
                }
            }
            JobState::Initialized => {
                let mut all_completed = true;
                let  job = &mut self.jobs[job_num];
                job.live = true;
                let deps = job.deps.clone();
                for dep in deps {
                    let dep_state = self.drive_all(dep, jobs, futures)?;
                    match dep_state {
                        JobState::Completed => {}
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
                            let drives = self.jobs[job_num].drives.clone();
                            for drive in drives {
                                let job = &self.jobs[drive];
                                if job.live {
                                    self.drive_all(drive, jobs, futures)?;
                                }
                            }
                            Ok(JobState::Completed)
                        }
                    };
                }
                Ok(JobState::Initialized)
            }
            JobState::Failed => Ok(JobState::Failed),
            JobState::Completed => Ok(JobState::Completed),
        };
    }

    // find the job for the target, and drive its completion
    async fn drive_targets(&mut self, targets: &Vec<String>) -> Result<(), TaskError> {
        let mut jobs: Vec<usize> = Vec::new();
        let mut futures: Vec<Shared<Pin<Box<dyn Future<Output = ExitStatus> + Send>>>> =
            Vec::new();

        // dbg!(&self.task_jobs);

        // first try named target, then fall back to file name check
        for target in targets {
            let name = if target.as_bytes()[0] as char == ':' {
                &target[1..]
            } else {
                &target
            };

            let job_num = match self.task_jobs.get(name) {
                Some(&job_num) => job_num,
                None => match self.file_jobs.get(name) {
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
                        let drives = self.jobs[completed_job_num].drives.clone();
                        for drive in drives {
                            let job = &self.jobs[drive];
                            if job.live {
                                self.drive_all(drive, &mut jobs, &mut futures)?;
                            }
                        }
                    }
                    else {
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
    runner.initialize_tasks()?;

    runner.expand_targets(&opts.target).await?;

    runner.drive_targets(&opts.target).await?;  

    Ok(())
}

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

mod cmd;
mod deno;
mod node;

use crate::chompfile::ChompEngine;
use crate::chompfile::TaskStdio;
use crate::engines::deno::deno_runner;
use crate::engines::node::node_runner;
use crate::extensions::BatcherResult;
use crate::task::{check_target_mtimes, relative_path};
use crate::ExtensionEnvironment;
use anyhow::Result;
use anyhow::{anyhow, Error};
use cmd::create_cmd;
use futures::future::Shared;
use futures::future::{Future, FutureExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;
use tokio::fs;
use tokio::process::Child;
use tokio::time::sleep;

pub fn replace_env_vars_static(arg: &str, env: &BTreeMap<String, String>) -> String {
    let mut out_arg = String::new();
    let mut pos = 0;
    while let Some(idx) = arg[pos..].find("${{") {
        let close_idx = match arg[pos + idx + 3..].find("}}") {
            Some(idx) => idx,
            None => {
                out_arg.push_str("${{");
                pos = pos + idx + 3;
                continue;
            }
        } + pos
            + idx
            + 3;

        let var_str = arg[pos + idx + 3..close_idx].trim();
        out_arg.push_str(&arg[pos..pos + idx]);
        if let Some(replacement) = env.get(var_str) {
            out_arg.push_str(replacement);
        } else {
            if let Ok(replacement) = std::env::var(var_str) {
                out_arg.push_str(&replacement);
            }
        }
        pos = close_idx + 2;
    }
    out_arg.push_str(&arg[pos..]);
    out_arg
}

pub struct CmdPool<'a> {
    cmd_num: usize,
    pub extension_env: &'a mut ExtensionEnvironment,
    cmds: BTreeMap<usize, CmdOp>,
    exec_num: usize,
    execs: BTreeMap<usize, Exec<'a>>,
    exec_cnt: usize,
    batching: BTreeSet<usize>,
    cmd_execs: BTreeMap<usize, usize>,
    cwd: String,
    pool_size: usize,
    batch_future: Option<Shared<Pin<Box<dyn Future<Output = Result<(), Rc<Error>>> + 'a>>>>,
}

#[derive(Hash, Serialize, PartialEq, Eq, Debug)]
pub struct CmdOp {
    pub name: Option<String>,
    pub id: usize,
    pub run: String,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub engine: ChompEngine,
    pub stdio: TaskStdio,
    pub targets: Vec<String>,
    pub echo: bool,
}

#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct BatchCmd {
    pub id: Option<usize>,
    pub run: String,
    pub echo: bool,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub engine: ChompEngine,
    pub stdio: Option<TaskStdio>,
    pub ids: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum ExecState {
    Executing,
    Completed,
    Failed,
    Terminating,
    Terminated,
}

#[derive(Debug)]
pub struct Exec<'a> {
    cmd: BatchCmd,
    child: Option<Child>,
    state: ExecState,
    future:
        Shared<Pin<Box<dyn Future<Output = Option<(ExecState, Option<Duration>, Duration)>> + 'a>>>,
}

impl<'a> CmdPool<'a> {
    pub fn new(
        pool_size: usize,
        cwd: String,
        extension_env: &'a mut ExtensionEnvironment,
    ) -> CmdPool<'a> {
        CmdPool {
            cmd_num: 0,
            cwd,
            cmds: BTreeMap::new(),
            exec_num: 0,
            exec_cnt: 0,
            execs: BTreeMap::new(),
            pool_size,
            extension_env,
            batching: BTreeSet::new(),
            cmd_execs: BTreeMap::new(),
            batch_future: None,
        }
    }

    pub fn terminate(&mut self, cmd_num: usize, name: &str) {
        // Note: On Windows, terminating a process does not terminate
        // the child processes, which can leave zombie processes behind
        println!("Terminating {}...", name);
        let exec_num = self.cmd_execs.get(&cmd_num).unwrap();
        let exec = &mut self.execs.get_mut(&exec_num).unwrap();
        if matches!(exec.state, ExecState::Executing) {
            exec.state = ExecState::Terminating;
            let child = exec.child.as_mut().unwrap();
            child.start_kill().expect("Unable to terminate process");
        }
    }

    pub fn get_exec_future(
        &mut self,
        cmd_num: usize,
    ) -> Pin<
        Box<dyn Future<Output = Result<(ExecState, Option<Duration>, Duration), Rc<Error>>> + 'a>,
    > {
        let pool = self as *mut CmdPool;
        async move {
            let this = unsafe { &mut *pool };
            loop {
                if let Some(exec_num) = this.cmd_execs.get(&cmd_num) {
                    let exec = &this.execs[&exec_num];
                    let result = exec.future.clone().await;
                    if result.is_none() {
                        return Err(Rc::new(match exec.cmd.engine {
                            ChompEngine::Shell => anyhow!("Unable to initialize shell command engine"),
                            ChompEngine::Node => anyhow!("Unable to initialize the Node.js Chomp engine.\n\x1b[33mMake sure Node.js is correctly installed and the \x1b[1mnode\x1b[0m\x1b[33m command bin is in the environment PATH.\x1b[0m\n\nSee \x1b[36;4mhttps://nodejs.org/en/download/\x1b[0m\n"),
                            ChompEngine::Deno => anyhow!("Unable to initialize the Deno Chomp engine.\n\x1b[33mMake sure Deno is correctly installed and the \x1b[1mdeno\x1b[0m\x1b[33m bin is in the environment PATH.\x1b[0m\n\nSee \x1b[36;4mhttps://deno.land/#installation\x1b[0m\n"),
                        }));
                    }
                    return Ok(result.unwrap());
                }
                if this.batch_future.is_none() {
                    this.create_batch_future();
                }
                this.batch_future.as_ref().unwrap().clone().await?;
            }
        }.boxed_local()
    }

    fn create_batch_future(&mut self) {
        // This is bad Rust, but it's also totally fine given the static execution model
        // (in Zig it might even be called idomatic)...
        let pool = self as *mut CmdPool;
        let cmds = &mut self.cmds as *mut BTreeMap<usize, CmdOp>;
        self.batch_future = Some(
            async move {
                // batches with 5 millisecond execution groupings
                sleep(Duration::from_millis(5)).await;
                // pool itself is static. Rust doesn't know this.
                let this = unsafe { &mut *pool };
                // cmds are immutable, and retained as long as executions. Rust doesn't know this.
                let cmds = unsafe { &mut *cmds };
                let mut batch: HashSet<&CmdOp> =
                    this.batching.iter().map(|cmd_num| &cmds[cmd_num]).collect();
                let running: HashSet<&BatchCmd> = this
                    .execs
                    .values()
                    .filter(|exec| matches!(&exec.state, ExecState::Executing))
                    .map(|exec| &exec.cmd)
                    .collect();
                let mut global_completion_map: Vec<(usize, usize)> = Vec::new();
                let mut batched: Vec<BatchCmd> = Vec::new();

                let mut batcher = 0;
                if this.extension_env.has_batchers() {
                    'outer: loop {
                        let (
                            BatcherResult {
                                defer: mut queue,
                                mut exec,
                                mut completion_map,
                            },
                            next,
                        ) = this.extension_env.run_batcher(batcher, &batch, &running)?;
                        if let Some(completion_map) = completion_map.take() {
                            for (cmd_num, exec_num) in completion_map {
                                batch.remove(&cmds[&cmd_num]);
                                this.batching.remove(&cmd_num);
                                global_completion_map.push((cmd_num, exec_num));
                            }
                        }
                        if let Some(queue) = queue.take() {
                            for cmd_num in queue {
                                batch.remove(&cmds[&cmd_num]);
                            }
                        }
                        if let Some(mut exec) = exec.take() {
                            for cmd in exec.drain(..) {
                                for cmd_num in cmd.ids.iter() {
                                    this.batching.remove(&cmd_num);
                                    batch.remove(&cmds[&cmd_num]);
                                }
                                batched.push(cmd);
                            }
                        }
                        match next {
                            Some(num) => batcher = num,
                            None => break 'outer,
                        };
                    }
                }
                for (cmd_num, exec_num) in global_completion_map {
                    this.execs.get_mut(&exec_num).unwrap().cmd.ids.push(cmd_num);
                }
                for cmd in batched.drain(..) {
                    this.new_exec(cmd).await;
                }
                // any leftover unbatched just get batched
                for cmd in batch {
                    if this.exec_cnt == this.pool_size {
                        break;
                    }
                    this.batching.remove(&cmd.id);
                    this.new_exec(BatchCmd {
                        id: None,
                        echo: cmd.echo,
                        run: cmd.run.to_string(),
                        cwd: cmd.cwd.clone(),
                        engine: cmd.engine,
                        env: cmd.env.clone(),
                        stdio: Some(cmd.stdio.clone()),
                        ids: vec![cmd.id],
                    })
                    .await;
                }

                this.batch_future = None;
                Ok(())
            }
            .boxed_local()
            .shared(),
        );
    }

    async fn new_exec(&mut self, mut cmd: BatchCmd) {
        let exec_num = self.exec_num;
        cmd.id = Some(exec_num);

        let mut targets = Vec::new();
        for id in &cmd.ids {
            let cmd = &self.cmds[&id];
            self.cmd_execs.insert(*id, exec_num);
            if let Some(name) = &cmd.name {
                println!("\x1b[1m🞂 {}\x1b[0m", relative_path(name));
            }
            for target in &cmd.targets {
                let target_path = Path::new(target);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).await.unwrap();
                }
                targets.push(target.to_string());
            }
        }

        let pool = self as *mut CmdPool;

        match cmd.engine {
            ChompEngine::Shell => {
                let start_time = Instant::now();
                self.exec_cnt = self.exec_cnt + 1;
                let child = create_cmd(cmd.cwd.as_ref().unwrap_or(&self.cwd), &cmd, true);
                let future = async move {
                    let this = unsafe { &mut *pool };
                    let mut exec = &mut this.execs.get_mut(&exec_num).unwrap();
                    exec.state = match exec.child.as_mut().unwrap().wait().await {
                        Ok(status) => {
                            if status.success() {
                                ExecState::Completed
                            } else {
                                ExecState::Failed
                            }
                        }
                        Err(e) => match exec.state {
                            ExecState::Terminating => ExecState::Terminated,
                            _ => panic!("Unexpected exec error {:?}", e),
                        },
                    };
                    let end_time = Instant::now();
                    this.exec_cnt = this.exec_cnt - 1;
                    // finally we verify that the targets exist
                    let mtime = check_target_mtimes(targets, true).await;
                    Some((exec.state, mtime, end_time - start_time))
                }
                .boxed_local()
                .shared();
                self.execs.insert(
                    exec_num,
                    Exec {
                        cmd,
                        child,
                        future,
                        state: ExecState::Executing,
                    },
                );
                self.exec_num = self.exec_num + 1;
            }
            ChompEngine::Node => node_runner(self, cmd, targets),
            ChompEngine::Deno => deno_runner(self, cmd, targets),
        };
    }

    pub fn batch(
        &mut self,
        name: Option<String>,
        run: &String,
        targets: Vec<String>,
        env: BTreeMap<String, String>,
        replacements: bool,
        cwd: Option<String>,
        engine: ChompEngine,
        stdio: TaskStdio,
        echo: bool,
    ) -> usize {
        let id = self.cmd_num;
        let run = if matches!(engine, ChompEngine::Shell) && replacements {
            replace_env_vars_static(run, &env)
        } else {
            run.to_string()
        };
        self.cmds.insert(
            id,
            CmdOp {
                id,
                cwd,
                name,
                run,
                env,
                echo,
                engine,
                stdio,
                targets,
            },
        );
        self.cmd_num = id + 1;
        self.batching.insert(id);
        if self.batch_future.is_none() {
            self.create_batch_future();
        }
        id
    }
}

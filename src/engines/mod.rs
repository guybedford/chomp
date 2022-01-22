mod cmd;
mod node;

use std::rc::Rc;
use futures::future::Shared;
use crate::chompfile::Batcher;
use crate::chompfile::ChompEngine;
use crate::engines::node::node_runner;
use crate::js::run_js_batcher;
use crate::task::check_target_mtimes;
use anyhow::Error;
use async_std::process::Child;
use async_std::process::ExitStatus;
use cmd::create_cmd;
use futures::future::{Future, FutureExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;
use tokio::time::sleep;

pub struct CmdPool {
    cmd_num: usize,
    cmds: BTreeMap<usize, CmdOp>,
    exec_num: usize,
    execs: BTreeMap<usize, Exec>,
    exec_cnt: usize,
    batching: BTreeSet<usize>,
    cmd_execs: BTreeMap<usize, usize>,
    cwd: String,
    pool_size: usize,
    batchers: Vec<Batcher>,
    batch_future: Option<Shared<Pin<Box<dyn Future<Output = Result<(), Rc<Error>>>>>>>,
    debug: bool,
}

#[derive(Hash, Serialize, PartialEq, Eq)]
pub struct CmdOp {
    pub name: Option<String>,
    pub id: usize,
    pub run: String,
    pub env: BTreeMap<String, String>,
    pub engine: ChompEngine,
    pub targets: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct BatchCmd {
    pub id: Option<usize>,
    pub run: String,
    pub env: BTreeMap<String, String>,
    pub engine: ChompEngine,
    pub ids: Vec<usize>,
}

enum ExecState {
    Executing,
    Completed,
    Failed,
}

struct Exec {
    cmd: BatchCmd,
    child: Child,
    state: ExecState,
    future: Shared<Pin<Box<dyn Future<Output = (ExitStatus, Option<Duration>, Duration)>>>>
}

impl CmdPool {
    pub fn new(pool_size: usize, batchers: &Vec<Batcher>, cwd: String, debug: bool) -> CmdPool {
        CmdPool {
            cmd_num: 0,
            cmds: BTreeMap::new(),
            exec_num: 0,
            exec_cnt: 0,
            execs: BTreeMap::new(),
            pool_size,
            cwd,
            batchers: batchers.clone(),
            batching: BTreeSet::new(),
            cmd_execs: BTreeMap::new(),
            batch_future: None,
            debug,
        }
    }

    pub fn get_exec_future(
        &mut self,
        cmd_num: usize,
    ) -> Pin<Box<dyn Future<Output = Result<(ExitStatus, Option<Duration>, Duration), Rc<Error>>>>> {
        let pool = self as *mut CmdPool;
        async move {
            let this = unsafe { &mut *pool };
            loop {
                if let Some(exec_num) = this.cmd_execs.get(&cmd_num) {
                    let exec = &this.execs[&exec_num];
                    return Ok(exec.future.clone().await);
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
                let mut batch: HashSet<&CmdOp> = this
                    .batching
                    .iter()
                    .map(|cmd_num| &cmds[cmd_num])
                    .collect();
                let running: HashSet<&BatchCmd> = this.execs.values().filter(|exec| matches!(&exec.state, ExecState::Executing)).map(|exec| &exec.cmd).collect();
                let mut global_completion_map: Vec<(usize, usize)> = Vec::new();
                let mut batched: Vec<BatchCmd> = Vec::new();
                for batcher in &this.batchers {
                    let (queued, mut exec, completion_map) = run_js_batcher(
                        &batcher.batch,
                        &batcher.name,
                        &batch,
                        &running,
                    )?;
                    for (cmd_num, exec_num) in completion_map {
                        batch.remove(&cmds[&cmd_num]);
                        this.batching.remove(&cmd_num);
                        global_completion_map.push((cmd_num, exec_num));
                    }
                    for cmd_num in queued {
                        batch.remove(&cmds[&cmd_num]);
                    }
                    for cmd in exec.drain(..) {
                        for cmd_num in cmd.ids.iter() {
                            this.batching.remove(&cmd_num);
                            batch.remove(&cmds[&cmd_num]);
                        }
                        batched.push(cmd);
                    }
                }
                for (cmd_num, exec_num) in global_completion_map {
                    this.execs.get_mut(&exec_num).unwrap().cmd.ids.push(cmd_num);
                }
                for cmd in batched.drain(..) {
                    this.new_exec(cmd);
                }
                // any leftover unbatched just get batched
                for cmd in batch {
                    if this.exec_cnt + 1 == this.pool_size {
                        break;
                    }
                    this.batching.remove(&cmd.id);
                    this.new_exec(BatchCmd {
                        id: None,
                        run: cmd.run.to_string(),
                        engine: cmd.engine,
                        env: cmd.env.clone(),
                        ids: vec![cmd.id],
                    });
                }
                for &cmd in this.batching.iter() {
                    dbg!(cmd);
                }
                this.batch_future = None;
                Ok(())
            }.boxed_local().shared(),
        );
    }

    fn new_exec(&mut self, mut cmd: BatchCmd) {
        let debug = self.debug;

        let exec_num = self.exec_num;
        cmd.id = Some(exec_num);

        let mut targets = Vec::new();
        for id in &cmd.ids {
            let cmd = &self.cmds[&id];
            self.cmd_execs.insert(*id, exec_num);
            if let Some(name) = &cmd.name {
                println!("🞂 {}", name);
            }
            for target in &cmd.targets {
                targets.push(target.to_string());
            }
        }

        self.exec_cnt = self.exec_cnt + 1;

        let pool = self as *mut CmdPool;

        // let pool = self as *mut CmdPool;
        let (child, future) = match cmd.engine {
            ChompEngine::Cmd => {
                let start_time = Instant::now();
                let child = create_cmd(&self.cwd, &cmd, debug);
                let future = async move {
                    let this = unsafe { &mut *pool };
                    let mut exec = &mut this.execs.get_mut(&exec_num).unwrap();
                    let status = exec.child.status().await.expect("Child process error");
                    exec.state = if status.success() { ExecState::Completed } else { ExecState::Failed };
                    let end_time = Instant::now();
                    this.exec_cnt = this.exec_cnt - 1;
                    // finally we verify that the targets exist
                    let mtime = check_target_mtimes(targets).await;
                    (status, mtime, end_time - start_time)
                }
                .boxed_local().shared();
                (child, future)
            }
            ChompEngine::Node => node_runner(self, &mut cmd, targets, debug),
        };

        self.execs.insert(exec_num, Exec { cmd, child, future, state: ExecState::Executing });
        self.exec_num = self.exec_num + 1;
    }

    pub fn batch(
        &mut self,
        name: Option<String>,
        run: String,
        targets: Vec<String>,
        env: BTreeMap<String, String>,
        engine: ChompEngine,
    ) -> usize {
        let id = self.cmd_num;
        self.cmds.insert(
            id,
            CmdOp {
                id,
                name,
                run,
                env,
                engine,
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

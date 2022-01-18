mod cmd;
mod node;

use crate::chompfile::Batcher;
use crate::chompfile::ChompEngine;
use crate::engines::node::node_runner;
use crate::task::check_target_mtimes;
use async_std::process::Child;
use async_std::process::ExitStatus;
use cmd::create_cmd;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, LinkedList};
use std::time::Duration;

pub struct CmdPool {
    running: LinkedList<(CmdOp, Child)>,
    cwd: String,
    pool_size: usize,
    batchers: Vec<Batcher>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmdOp {
    run: String,
    engine: ChompEngine,
    env: BTreeMap<String, String>,
}

impl<'a> CmdPool {
    pub fn new(pool_size: usize, batchers: &Vec<Batcher>, cwd: String) -> CmdPool {
        CmdPool {
            pool_size,
            cwd,
            batchers: batchers.clone(),
            running: LinkedList::new(),
        }
    }

    fn batch(
        &mut self,
        run: String,
        env: &BTreeMap<String, String>,
        debug: bool,
    ) -> BoxFuture<'static, Child> {
        let child = create_cmd(&self.cwd, run, env, debug);
        // running.push_back((CmdOp { run, engine, env }))
        Box::pin(async { child })
    }

    pub fn run(
        &mut self,
        run: String,
        targets: Vec<String>,
        env: &mut BTreeMap<String, String>,
        engine: ChompEngine,
        debug: bool,
    ) -> BoxFuture<'a, (ExitStatus, Option<Duration>)> {
        match engine {
            ChompEngine::Cmd => {
                let child_future = self.batch(run, env, debug);
                Box::pin(async {
                    let mut child = child_future.await;
                    let status = child.status().await.expect("Child process error");
                    // finally we verify that the targets exist
                    let mtime = check_target_mtimes(targets).await;
                    (status, mtime)
                })
            }
            ChompEngine::Node => {
                let node_future = node_runner(self, run, env, debug);
                Box::pin(async {
                    let status = node_future.await;
                    let mtime = check_target_mtimes(targets).await;
                    (status, mtime)
                })
            }
        }
    }
}

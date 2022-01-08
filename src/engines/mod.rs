mod cmd;
mod node;

use std::time::Duration;
use futures::future::BoxFuture;
use crate::engines::node::node_runner;
use cmd::create_cmd;
use async_std::process::Child;
use async_std::process::ExitStatus;
use std::collections::BTreeMap;
use crate::task::check_target_mtimes;
use crate::chompfile::ChompEngine;

pub struct CmdPool {
    cwd: String,
    pool_size: usize,
}

impl<'a> CmdPool {
    pub fn new(pool_size: usize, cwd: String) -> CmdPool {
        CmdPool { pool_size, cwd }
    }

    // TODO: actually implement pooling
    fn get_next(
        &mut self,
        run: String,
        env: &BTreeMap<String, String>,
        debug: bool
    ) -> BoxFuture<'static, Child> {
        let child = create_cmd(&self.cwd, run, env, debug);
        Box::pin(async { child })
    }

    pub fn run(
        &mut self,
        run: String,
        targets: Vec<String>,
        env: &mut BTreeMap<String, String>,
        engine: ChompEngine,
        debug: bool
    ) -> BoxFuture<'a, (ExitStatus, Option<Duration>)> {
        match engine {
            ChompEngine::Cmd => {
                let child_future = self.get_next(run, env, debug);
                Box::pin(async {
                    let mut child = child_future.await;
                    let status = child.status().await.expect("Child process error");
                    // finally we verify that the targets exist
                    let mtime = check_target_mtimes(targets).await;
                    (status, mtime)
                })
            },
            ChompEngine::Node => {
                let node_future = node_runner(self, run, env, debug);
                Box::pin(async {
                    let status = node_future.await;
                    let mtime = check_target_mtimes(targets).await;
                    (status, mtime)
                })
            },
        }
    }
}

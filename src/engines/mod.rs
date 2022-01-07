mod cmd;
mod node;

use futures::future::BoxFuture;
use crate::engines::node::node_runner;
use cmd::create_cmd;
use async_std::process::Child;
use async_std::process::ExitStatus;
use std::collections::BTreeMap;
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
        env: &mut BTreeMap<String, String>,
        engine: ChompEngine,
        debug: bool
    ) -> BoxFuture<'a, ExitStatus> {
        match engine {
            ChompEngine::Cmd => {
                let child_future = self.get_next(run, env, debug);
                Box::pin(async {
                    let mut child = child_future.await;
                    child.status().await.expect("Child process error")
                })
            },
            ChompEngine::Node => node_runner(self, run, env, debug),
        }
    }
}

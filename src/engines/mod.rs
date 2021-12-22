mod cmd;
mod node;

use std::path::PathBuf;
use std::collections::VecDeque;
use futures::future::BoxFuture;
use crate::engines::node::node_runner;
use cmd::create_cmd;
use async_std::process::Child;
use async_std::process::ExitStatus;
use futures::future::Future;
use std::collections::BTreeMap;
use std::fs;

pub enum ChompEngine {
    Cmd,
    Node,
}

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
        env: Option<&BTreeMap<String, String>>,
    ) -> BoxFuture<'static, Child> {
        let child = create_cmd(&self.cwd, run, env);
        Box::pin(async { child })
    }

    pub fn run(
        &mut self,
        run: String,
        env: Option<&BTreeMap<String, String>>,
        engine: ChompEngine,
    ) -> BoxFuture<'a, ExitStatus> {
        match engine {
            ChompEngine::Cmd => {
                let child_future = self.get_next(run, env);
                Box::pin(async {
                    let mut child = child_future.await;
                    child.status().await.expect("Child process error")
                })
            },
            ChompEngine::Node => node_runner(self, run, env),
        }
    }
}

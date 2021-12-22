use async_std::process::ExitStatus;
use futures::future::BoxFuture;
use crate::engines::CmdPool;
use std::collections::BTreeMap;
use async_std::process::{Child};
use async_std::fs;
use std::env;
use uuid::Uuid;

pub fn node_runner (cmd_pool: &mut CmdPool, run: String, env: Option<&BTreeMap<String, String>>) -> BoxFuture<'static, ExitStatus> {
  let uuid = Uuid::new_v4();
  let mut tmp_file = env::temp_dir();
  tmp_file.push(&format!("{}.mjs", uuid.to_simple().to_string()));
  let child_future = cmd_pool.get_next(format!("node {}", tmp_file.to_str().unwrap()), env);
  Box::pin(async move {
    fs::write(&tmp_file, run).await.expect("unable to write temporary file");
    let mut child = child_future.await;
    let result = child.status().await.expect("Child process error");
    fs::remove_file(&tmp_file).await.expect("unable to cleanup tmp file");
    result
  })
}

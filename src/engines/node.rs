use crate::engines::BatchCmd;
use futures::future::Shared;
use std::time::Instant;
use crate::engines::check_target_mtimes;
use crate::engines::create_cmd;
use async_std::process::Child;
use crate::engines::CmdPool;
use async_std::fs;
use async_std::process::ExitStatus;
use std::env;
use uuid::Uuid;
use std::pin::Pin;
use std::time::Duration;
use futures::future::{Future, FutureExt};
use crate::chompfile::ChompEngine;

// Custom node loader to mimic current working directory despite loading from a tmp file
const NODE_CMD: &str = "node --no-warnings --loader \"data:text/javascript,import{readFileSync}from'fs';export function resolve(u,c,d){if(u.endsWith('[cm]'))return{url:u,format:'module'};return d(u,c);}export function load(u,c,d){if(u.endsWith('[cm]'))return{source:readFileSync(process.env.CHOMP_MAIN),format:'module'};return d(u,c)}export{load as getFormat,load as getSource}\" [cm]";

pub fn node_runner(
  cmd_pool: &mut CmdPool,
  batch_cmd: &mut BatchCmd,
  targets: Vec<String>,
  debug: bool,
) -> (Child, Shared<Pin<Box<dyn Future<Output = (ExitStatus, Option<Duration>, Duration)>>>>) {
  // TODO: debug should pipe console output for node.js run
  let start_time = Instant::now();
  let uuid = Uuid::new_v4();
  let mut tmp_file = env::temp_dir();
  tmp_file.push(&format!("{}.mjs", uuid.to_simple().to_string()));
  let tmp_file2 = tmp_file.clone();
  batch_cmd.env.insert("CHOMP_MAIN".to_string(), tmp_file.to_str().unwrap().to_string());
  batch_cmd.env.insert("CHOMP_PATH".to_string(), std::env::args().next().unwrap().to_string());
  let targets = targets.clone();
  let write_future = fs::write(tmp_file, batch_cmd.run.to_string());
  batch_cmd.run = NODE_CMD.to_string();
  batch_cmd.engine = ChompEngine::Cmd;
  cmd_pool.exec_cnt = cmd_pool.exec_cnt + 1;
  let pool = cmd_pool as *mut CmdPool;
  let mut child = create_cmd(&cmd_pool.cwd, batch_cmd, debug);
  let status = child.status();
  let future = async move {
    let cmd_pool = unsafe { &mut *pool };
    write_future.await.expect("unable to write temporary file");
    let status = status.await.expect("Child process error");
    cmd_pool.exec_cnt = cmd_pool.exec_cnt - 1;
    fs::remove_file(&tmp_file2).await.expect("unable to cleanup tmp file");
    let end_time = Instant::now();
    // finally we verify that the targets exist
    let mtime = check_target_mtimes(targets).await;
    (status, mtime, end_time - start_time)
  }.boxed_local().shared();
  (child, future)
}

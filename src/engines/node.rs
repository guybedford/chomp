use crate::engines::Exec;
use crate::engines::{BatchCmd, ExecState};
use std::time::Instant;
use crate::engines::check_target_mtimes;
use crate::engines::create_cmd;
use crate::engines::CmdPool;
use tokio::fs;
use std::env;
use uuid::Uuid;
use futures::future::{FutureExt};
use crate::chompfile::ChompEngine;

// Custom node loader to mimic current working directory despite loading from a tmp file
const NODE_CMD: &str = "node --no-warnings --loader \"data:text/javascript,import{readFileSync}from'fs';export function resolve(u,c,d){if(u.endsWith('[cm]'))return{url:u,format:'module'};return d(u,c);}export function load(u,c,d){if(u.endsWith('[cm]'))return{source:readFileSync(process.env.CHOMP_MAIN),format:'module'};return d(u,c)}export{load as getFormat,load as getSource}\" [cm]";

pub fn node_runner(
  cmd_pool: &mut CmdPool,
  mut cmd: BatchCmd,
  targets: Vec<String>,
  debug: bool,
) {
  // TODO: debug should pipe console output for node.js run
  let start_time = Instant::now();
  let uuid = Uuid::new_v4();
  let mut tmp_file = env::temp_dir();
  tmp_file.push(&format!("{}.mjs", uuid.to_simple().to_string()));
  let tmp_file2 = tmp_file.clone();
  cmd.env.insert("CHOMP_MAIN".to_string(), tmp_file.to_str().unwrap().to_string());
  cmd.env.insert("CHOMP_PATH".to_string(), std::env::args().next().unwrap().to_string());
  let targets = targets.clone();
  let write_future = fs::write(tmp_file, cmd.run.to_string());
  cmd.run = NODE_CMD.to_string();
  cmd.engine = ChompEngine::Cmd;
  let exec_num = cmd_pool.exec_num;
  cmd_pool.exec_cnt = cmd_pool.exec_cnt + 1;
  let pool = cmd_pool as *mut CmdPool;
  let child = create_cmd(&cmd_pool.cwd, &cmd, debug);
  let future = async move {
    let cmd_pool = unsafe { &mut *pool };
    let mut exec = &mut cmd_pool.execs.get_mut(&exec_num).unwrap();
    write_future.await.expect("unable to write temporary file");
    exec.state = match exec.child.as_mut().unwrap().wait().await {
      Ok(status) => {
          if status.success() {
              ExecState::Completed
          } else {
              ExecState::Failed
          }
      },
      Err(e) => match exec.state {
          ExecState::Terminating => ExecState::Terminated,
          _ => panic!("Unexpected exec error {:?}", e)
      }
    };
    cmd_pool.exec_cnt = cmd_pool.exec_cnt - 1;
    fs::remove_file(&tmp_file2).await.expect("unable to cleanup tmp file");
    let end_time = Instant::now();
    // finally we verify that the targets exist
    let mtime = check_target_mtimes(targets, true).await;
    (exec.state, mtime, end_time - start_time)
  }.boxed_local().shared();

  cmd_pool.execs.insert(exec_num, Exec { cmd, child: Some(child), future, state: ExecState::Executing });
  cmd_pool.exec_num = cmd_pool.exec_num + 1;

}

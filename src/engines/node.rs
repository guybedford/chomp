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

use crate::engines::check_target_mtimes;
use crate::engines::create_cmd;
use crate::engines::CmdPool;
use crate::engines::Exec;
use crate::engines::{BatchCmd, ExecState};
use futures::future::FutureExt;
use std::env;
use std::time::Instant;
use tokio::fs;
use uuid::Uuid;

// Custom node loader to mimic current working directory despite loading from a tmp file
const NODE_CMD: &str = "node --no-warnings --loader \"data:text/javascript,import{readFileSync}from'fs';export function resolve(u,c,d){if(u.endsWith('[cm]'))return{url:u,format:'module'};return d(u,c);}export function load(u,c,d){if(u.endsWith('[cm]'))return{source:readFileSync(process.env.CHOMP_MAIN),format:'module'};return d(u,c)}export{load as getFormat,load as getSource}\" [cm]";

pub fn node_runner(cmd_pool: &mut CmdPool, mut cmd: BatchCmd, targets: Vec<String>) {
  let start_time = Instant::now();
  let uuid = Uuid::new_v4();
  let mut tmp_file = env::temp_dir();
  tmp_file.push(&format!("{}.mjs", uuid.to_simple().to_string()));
  let tmp_file2 = tmp_file.clone();
  cmd.env.insert(
    "CHOMP_MAIN".to_string(),
    tmp_file.to_str().unwrap().to_string(),
  );
  cmd.env.insert(
    "CHOMP_PATH".to_string(),
    std::env::args().next().unwrap().to_string(),
  );
  let targets = targets.clone();
  // On posix, command starts executing before we wait on it!
  std::fs::write(tmp_file, cmd.run.to_string()).expect("unable to write temporary file");
  cmd.run = NODE_CMD.to_string();
  let echo = cmd.echo;
  cmd.echo = false;
  let exec_num = cmd_pool.exec_num;
  cmd_pool.exec_cnt = cmd_pool.exec_cnt + 1;
  let pool = cmd_pool as *mut CmdPool;
  let child = create_cmd(
    cmd.cwd.as_ref().unwrap_or(&cmd_pool.cwd),
    &cmd,
    false,
  );
  let future = async move {
    let cmd_pool = unsafe { &mut *pool };
    let mut exec = &mut cmd_pool.execs.get_mut(&exec_num).unwrap();
    if exec.child.is_none() {
      return None;
    }
    if echo {
      println!("<Node.js exec>");
    }
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
    cmd_pool.exec_cnt = cmd_pool.exec_cnt - 1;
    fs::remove_file(&tmp_file2)
      .await
      .expect("unable to cleanup tmp file");
    let end_time = Instant::now();
    // finally we verify that the targets exist
    let mtime = check_target_mtimes(targets, true).await;
    Some((exec.state, mtime, end_time - start_time))
  }
  .boxed_local()
  .shared();

  cmd_pool.execs.insert(
    exec_num,
    Exec {
      cmd,
      child,
      future,
      state: ExecState::Executing,
    },
  );
  cmd_pool.exec_num = cmd_pool.exec_num + 1;
}

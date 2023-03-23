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
use base64::{engine::general_purpose, Engine as _};
use futures::future::FutureExt;
use percent_encoding::percent_encode;
use percent_encoding::NON_ALPHANUMERIC;
use std::time::Instant;

// Custom node loader to mimic current working directory despite loading from a tmp file
// Note: We dont have to percent encode as we're not using `,! characters
// If this becomes a problem, switch to base64 encoding rather
const NODE_LOADER: &str = "let s;export function resolve(u,c,d){if(c.parentURL===undefined){const i=u.indexOf('data:text/javascript;base64,');s=Buffer.from(u.slice(i+28),'base64');return{url:u.slice(0,i)+(u[i-1]==='/'?'':'/')+'[cm]',format:'module',shortCircuit:true}}return d(u,c)}export function load(u,c,d){if(u.endsWith('[cm]'))return{source:s,format:'module',shortCircuit:true};return d(u,c)}export{load as getFormat,load as getSource}";

pub fn node_runner(cmd_pool: &mut CmdPool, mut cmd: BatchCmd, targets: Vec<String>) {
    let start_time = Instant::now();
    cmd.env.insert(
        "CHOMP_PATH".to_string(),
        std::env::args().next().unwrap().to_string(),
    );
    let targets = targets.clone();
    // On posix, command starts executing before we wait on it!
    cmd.run = format!(
    "node --no-warnings --loader \"data:text/javascript,{}\" \"data:text/javascript;base64,{}\"",
    percent_encode(NODE_LOADER.to_string().as_bytes(), NON_ALPHANUMERIC),
    general_purpose::STANDARD.encode(cmd.run.as_bytes())
  );
    let echo = cmd.echo;
    cmd.echo = false;
    let run_clone = if echo { Some(cmd.run.clone()) } else { None };
    let exec_num = cmd_pool.exec_num;
    cmd_pool.exec_cnt = cmd_pool.exec_cnt + 1;
    let pool = cmd_pool as *mut CmdPool;
    let child = create_cmd(
        cmd.cwd.as_ref().unwrap_or(&cmd_pool.cwd),
        &cmd_pool.path,
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
            println!("{}", run_clone.as_ref().unwrap());
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

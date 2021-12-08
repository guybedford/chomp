use std::collections::VecDeque;
use async_std::io::WriteExt;
use async_std::process::{Child, Command, ExitStatus, Stdio};
use std::pin::Pin;
use futures::future::{Future, FutureExt};
use std::collections::BTreeMap;
use std::env;
use std::time::Duration;
use async_std::task;

pub struct CmdPool {
    cwd: String,
    pool_size: usize,
}

#[cfg(target_os = "windows")]
fn create_cmd(cwd: &str, run: &str, env: &Option<BTreeMap<String, String>>) -> Child {
    let shell = if env::var("PSModulePath").is_ok() { "powershell" } else {
        panic!("Powershell is required on Windows");
        // "cmd"
    };
    let mut cmd = Command::new(shell);
    let mut path: String = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ";";
    }
    path.push_str(cwd);
    path += ".bin;";
    path.push_str(cwd);
    path += "/node_modules/.bin";
    cmd.env("PATH", path);
    if let Some(env) = env {
        for (name, value) in env {
            cmd.env(name, value);
        }
    }
    if shell == "powershell" {
        cmd.arg("-ExecutionPolicy");
        cmd.arg("Unrestricted");
        cmd.arg("-NonInteractive");
        cmd.arg("-NoLogo");
        let mut run_str = String::from("$PSDefaultParameterValues['Out-File:Encoding']='utf8';\n");
        run_str.push_str(&run);
        cmd.arg(run_str);
    }
    else {
        cmd.arg("/d");
        // cmd.arg("/s");
        cmd.arg("/c");
        cmd.arg(run);
    }
    cmd.spawn().unwrap()
}

#[cfg(not(target_os = "windows"))]
fn create_cmd(cwd: &str, run: &str, env: &Option<BTreeMap<String, String>>) -> Child {
    let mut cmd = Command::new("sh");
    let mut path = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ":";
    }
    path.push_str(cwd);
    path += ".bin:";
    path.push_str(cwd);
    path += "/node_modules/.bin";
    cmd.env("PATH", path);
    if let Some(env) = env {
        for (name, value) in env {
            cmd.env(name, value);
        }
    }
    cmd.arg("-c");
    cmd.arg(run);
    cmd.spawn().unwrap()
}

// For Cmd + Unix shell we just run command directly
// For powershell we immediately preinitialize the shell tasks in pools, as powershell can take a while to startup
impl CmdPool {
    pub fn new(pool_size: usize, cwd: String) -> CmdPool {
        CmdPool { pool_size, cwd }
    }

    fn get_next (&mut self, run: &str, env: &Option<BTreeMap<String, String>>) -> Child {
        create_cmd(&self.cwd, run, env)
    }

    pub fn run<'a>(
        &mut self,
        run: &str,
        env: &Option<BTreeMap<String, String>>
    ) -> impl Future<Output = ExitStatus> {
        // TODO: compare env to default_env and apply dirr for powershell
        let mut child = self.get_next(run, env);
        async move {
            let status = child.status().await.expect("Something went wrong");
            status
        }
    }
}

// #[cfg(unix)]
// mod test {

//     #[test]
//     fn test_into_inner() {
//         futures_lite::future::block_on(async {
//             use crate::Command;

//             use std::io::Result;
//             use std::process::Stdio;
//             use std::str::from_utf8;

//             use futures_lite::AsyncReadExt;

//             let mut ls_child = Command::new("cat")
//                 .arg("Cargo.toml")
//                 .stdout(Stdio::piped())
//                 .spawn()?;

//             let stdio: Stdio = ls_child.stdout.take().unwrap().into_stdio().await?;

//             let mut echo_child = Command::new("grep")
//                 .arg("async")
//                 .stdin(stdio)
//                 .stdout(Stdio::piped())
//                 .spawn()?;

//             let mut buf = vec![];
//             let mut stdout = echo_child.stdout.take().unwrap();

//             stdout.read_to_end(&mut buf).await?;
//             dbg!(from_utf8(&buf).unwrap_or(""));

//             Result::Ok(())
//         })
//         .unwrap();
//     }
// }

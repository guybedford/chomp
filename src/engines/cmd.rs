use async_std::process::Stdio;
use async_std::process::{Child, Command};
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

fn replace_env_vars(arg: &str, env: &BTreeMap<String, String>) -> String {
    let mut out_arg = arg.to_string();
    for (name, value) in env {
        let mut env_str = String::from("$");
        env_str.push_str(name);
        if out_arg.contains(&env_str) {
            out_arg = out_arg.replace(&env_str, value);
        }
    }
    out_arg
}

#[cfg(target_os = "windows")]
pub fn create_cmd(cwd: &str, run: String, env: Option<&BTreeMap<String, String>>) -> Child {
    lazy_static! {
        // Currently does not support spaces in arg quotes, to make arg splitting simpler
        static ref CMD: Regex = Regex::new("(?x)
            ^(?P<cmd>[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+?)
             (?P<args>(?:\\ (?:
                [^`~!\\#&*()\t\\{\\[|;'\"n<>?\\\\\\ ]+? |
                (?:\"[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]*?\") |
                (?:'[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]*?') |
            )*?)*?)$
        ").unwrap();
    }
    let mut path: String = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ";";
    }
    path.push_str(cwd);
    path += ".bin;";
    path.push_str(cwd);
    path += "/node_modules/.bin";
    // fast path for direct commands to skip the shell entirely
    if let Some(capture) = CMD.captures(&run) {
        let mut cmd = String::from(&capture["cmd"]);
        let mut do_spawn = true;
        // Path-like must be exact
        if cmd.contains("/") {
            // canonicalize returns UNC...
            let unc_path = fs::canonicalize(PathBuf::from(cmd.clone())).unwrap();
            let unc_str = unc_path.to_str().unwrap();
            if unc_str.starts_with(r"\\?\") {
                cmd = String::from(&unc_path.to_str().unwrap()[4..]);
            } else {
                do_spawn = false;
            }
        }
        if do_spawn {
            let mut command = Command::new(&cmd);
            command.env("PATH", &path);
            if let Some(env) = env {
                for (name, value) in env {
                    command.env(name, value);
                }
            }
            for arg in capture["args"][1..].split(" ") {
                if let Some(env) = env.as_ref() {
                    command.arg(replace_env_vars(arg, env));
                } else {
                    command.arg(arg);
                }
            }
            command.stdin(Stdio::null());
            match command.spawn() {
                Ok(child) => {
                    return child;
                }
                // If first attempt fails, try ".cmd" extension too
                // Note: this only works on latest nightly builds!
                Err(_) => {
                    cmd.push_str(".cmd");
                    let mut command = Command::new(&cmd);
                    command.env("PATH", &path);
                    if let Some(env) = env {
                        for (name, value) in env {
                            command.env(name, value);
                        }
                    }
                    for arg in capture["args"].split(" ") {
                        if let Some(env) = env.as_ref() {
                            command.arg(replace_env_vars(arg, env));
                        } else {
                            command.arg(arg);
                        }
                    }
                    command.stdin(Stdio::null());
                    match command.spawn() {
                        Ok(child) => {
                            return child;
                        }
                        Err(_) => {}
                    }
                }
            };
        }
    }

    let shell = if env::var("PSModulePath").is_ok() {
        "powershell"
    } else {
        panic!("Powershell is required on Windows for arbitrary scripts");
        // "cmd"
    };
    let mut command = Command::new(shell);
    if shell == "powershell" {
        command.arg("-ExecutionPolicy");
        command.arg("Unrestricted");
        command.arg("-NonInteractive");
        command.arg("-NoLogo");
        // ensure file operations use UTF8
        let mut run_str = String::from("$PSDefaultParameterValues['Out-File:Encoding']='utf8';");
        // we also set _custom_ variables as local variables for easy substitution
        if let Some(env) = env {
            for (name, value) in env {
                run_str.push_str(&format!("${}=\"{}\";", name, value));
            }
        }
        run_str.push('\n');
        run_str.push_str(&run);
        command.arg(run_str);
    } else {
        command.arg("/d");
        // command.arg("/s");
        command.arg("/c");
        command.arg(run);
    }
    command.env("PATH", path);
    if let Some(env) = env {
        for (name, value) in env {
            command.env(name, value);
        }
    }
    command.stdin(Stdio::null());
    command.spawn().unwrap()
}

#[cfg(not(target_os = "windows"))]
pub fn create_cmd(cwd: &str, run: String, env: Option<&BTreeMap<String, String>>) -> Child {
    let mut command = Command::new("sh");
    let mut path = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ":";
    }
    path.push_str(cwd);
    path += ".bin:";
    path.push_str(cwd);
    path += "/node_modules/.bin";
    command.env("PATH", path);
    if let Some(env) = env {
        for (name, value) in env {
            command.env(name, value);
        }
    }
    command.arg("-c");
    command.arg(run);
    command.stdin(Stdio::null());
    command.spawn().unwrap()
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

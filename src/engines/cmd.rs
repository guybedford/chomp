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

use crate::chompfile::TaskStdio;
use crate::engines::BatchCmd;
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};

fn replace_env_vars(arg: &str, env: &BTreeMap<String, String>) -> String {
    let mut out_arg = arg.to_string();
    if out_arg.find('$').is_none() {
        return out_arg;
    }
    for (name, value) in env {
        if !out_arg.contains(name) {
            continue;
        }
        let mut env_str = String::from("$");
        env_str.push_str(name);
        if out_arg.contains(&env_str) {
            out_arg = out_arg.replace(&env_str, &value);
            if out_arg.find('$').is_none() {
                return out_arg;
            }
        }
        let mut env_str_curly = String::from("${");
        env_str_curly.push_str(name);
        env_str_curly.push_str("}");
        if out_arg.contains(&env_str_curly) {
            out_arg = out_arg.replace(&env_str_curly, &value);
            if out_arg.find('$').is_none() {
                return out_arg;
            }
        }
    }
    for (name, value) in std::env::vars() {
        let name = name.to_uppercase();
        if !out_arg.contains(&name) {
            continue;
        }
        let mut env_str = String::from("$");
        env_str.push_str(&name);
        if out_arg.contains(&env_str) {
            out_arg = out_arg.replace(&env_str, &value);
            if out_arg.find('$').is_none() {
                return out_arg;
            }
        }
        let mut env_str_curly = String::from("${");
        env_str_curly.push_str(&name);
        env_str_curly.push_str("}");
        if out_arg.contains(&env_str_curly) {
            out_arg = out_arg.replace(&env_str_curly, &value);
            if out_arg.find('$').is_none() {
                return out_arg;
            }
        }
    }
    out_arg
}

fn set_cmd_stdio(command: &mut Command, stdio: TaskStdio) {
    match stdio {
        TaskStdio::All => {}
        TaskStdio::StderrOnly => {
            command.stdin(Stdio::null());
            command.stdout(Stdio::null());
        }
        TaskStdio::StdoutOnly => {
            command.stdin(Stdio::null());
            command.stderr(Stdio::null());
        }
        TaskStdio::NoStdin => {
            command.stdin(Stdio::null());
        }
        TaskStdio::None => {
            command.stdin(Stdio::null());
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }
    };
}

#[cfg(target_os = "windows")]
pub fn create_cmd(
    cwd: &str,
    path: &str,
    batch_cmd: &BatchCmd,
    fastpath_fallback: bool,
) -> Option<Child> {
    let run = batch_cmd.run.trim();
    lazy_static! {
        static ref CMD: Regex = Regex::new(
            "(?x)
            ^(?P<cmd>[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+?)
             (?P<args>(?:\\ (?:
                [^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?')
            )*?)*?)$
        "
        )
        .unwrap();
        static ref ARGS: Regex = Regex::new(
            "(?x)
            \\ (?:[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?'))
        "
        )
        .unwrap();
    }
    if batch_cmd.echo {
        println!("{}", &run);
    }
    // fast path for direct commands to skip the shell entirely
    if let Some(capture) = CMD.captures(&run) {
        let mut cmd = String::from(&capture["cmd"]);
        let mut do_spawn = true;
        // Path-like must be exact
        if cmd.contains('/') || cmd.contains('\\') {
            // canonicalize returns UNC...
            let cmd_buf = PathBuf::from(&cmd);
            let cmd_buf = if Path::is_absolute(&cmd_buf) {
                cmd_buf
            } else {
                let mut buf = PathBuf::from(&cwd);
                buf.push(cmd_buf);
                buf
            };

            if let Ok(unc_path) = fs::canonicalize(cmd_buf) {
                let unc_str = unc_path.to_str().unwrap();
                if unc_str.starts_with(r"\\?\") {
                    cmd = String::from(&unc_path.to_str().unwrap()[4..]);
                } else {
                    do_spawn = false;
                }
            } else {
                do_spawn = false;
            }
        }
        if do_spawn {
            // Try ".cmd" extension first
            // Note: this requires latest Rust version
            let mut cmd_with_ext = cmd.to_owned();
            cmd_with_ext.push_str(".cmd");
            let mut command = Command::new(&cmd_with_ext);
            command.env("PATH", &path);
            for (name, value) in &batch_cmd.env {
                command.env(name, value);
            }
            command.current_dir(cwd);
            for arg in ARGS.captures_iter(&capture["args"]) {
                let arg = arg.get(0).unwrap().as_str();
                let first_char = arg.as_bytes()[1];
                let arg_str = if first_char == b'\'' || first_char == b'"' {
                    &arg[2..arg.len() - 1]
                } else {
                    &arg[1..arg.len()]
                };
                if batch_cmd.env.len() > 0 {
                    command.arg(replace_env_vars(arg_str, &batch_cmd.env));
                } else {
                    command.arg(arg_str);
                }
            }
            set_cmd_stdio(&mut command, batch_cmd.stdio.unwrap_or_default());
            match command.spawn() {
                Ok(child) => return Some(child),
                Err(_) => {
                    let mut command = Command::new(&cmd);
                    command.env("PATH", &path);
                    for (name, value) in &batch_cmd.env {
                        command.env(name, value);
                    }
                    command.current_dir(cwd);
                    for arg in ARGS.captures_iter(&capture["args"]) {
                        let arg = arg.get(0).unwrap().as_str();
                        let first_char = arg.as_bytes()[1];
                        let arg_str = if first_char == b'\'' || first_char == b'"' {
                            &arg[2..arg.len() - 1]
                        } else {
                            &arg[1..arg.len()]
                        };
                        if batch_cmd.env.len() > 0 {
                            command.arg(replace_env_vars(arg_str, &batch_cmd.env));
                        } else {
                            command.arg(arg_str);
                        }
                    }
                    set_cmd_stdio(&mut command, batch_cmd.stdio.unwrap_or_default());
                    match command.spawn() {
                        Ok(child) => return Some(child),
                        Err(_) => {
                            if !fastpath_fallback {
                                return None;
                            }
                        } // fallback to shell
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
        let mut run_str = String::from(
            "$PSDefaultParameterValues['Out-File:Encoding']='utf8';$ErrorActionPreference='Stop';",
        );
        // we also set _custom_ variables as local variables for easy substitution
        for (name, value) in &batch_cmd.env {
            run_str.push_str(&format!("${}='{}';", name, value.replace("'", "''")));
        }
        run_str.push('\n');
        run_str.push_str(&run);
        command.arg(run_str);
    } else {
        command.arg("/d");
        // command.arg("/s");
        command.arg("/c");
        command.arg(&run);
    }
    command.env("PATH", path);
    for (name, value) in &batch_cmd.env {
        command.env(name, value);
    }
    command.current_dir(cwd);
    set_cmd_stdio(&mut command, batch_cmd.stdio.unwrap_or_default());
    Some(command.spawn().unwrap())
}

#[cfg(not(target_os = "windows"))]
pub fn create_cmd(
    cwd: &str,
    path: &str,
    batch_cmd: &BatchCmd,
    fastpath_fallback: bool,
) -> Option<Child> {
    let run = batch_cmd.run.trim();
    lazy_static! {
        static ref CMD: Regex = Regex::new(
            "(?x)
            ^(?P<cmd>[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+?)
             (?P<args>(?:\\ (?:
                [^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?')
            )*?)*?)$
        "
        )
        .unwrap();
        static ref ARGS: Regex = Regex::new(
            "(?x)
            \\ (?:[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?'))
        "
        )
        .unwrap();
    }

    if batch_cmd.echo {
        println!("{}", run);
    }
    // Spawn needs an exact path for Ubuntu?
    // fast path for direct commands to skip the shell entirely
    if let Some(capture) = CMD.captures(&run) {
        let mut cmd = capture["cmd"].to_string();
        let mut do_spawn = true;
        // Path-like must be exact
        if cmd.contains("/") {
            let cmd_buf = PathBuf::from(&cmd);
            let cmd_buf = if Path::is_absolute(&cmd_buf) {
                cmd_buf
            } else {
                let mut buf = PathBuf::from(&cwd);
                buf.push(cmd_buf);
                buf
            };
            if let Ok(canonical) = fs::canonicalize(cmd_buf) {
                cmd = String::from(&canonical.to_str().unwrap()[4..]);
            } else {
                do_spawn = false;
            }
        }
        if do_spawn {
            let mut command = Command::new(&cmd);
            command.env("PATH", &path);
            for (name, value) in &batch_cmd.env {
                command.env(name, value);
            }
            command.current_dir(cwd);
            for arg in ARGS.captures_iter(&capture["args"]) {
                let arg = arg.get(0).unwrap().as_str();
                let first_char = arg.as_bytes()[1];
                let arg_str = if first_char == b'\'' || first_char == b'"' {
                    &arg[2..arg.len() - 1]
                } else {
                    &arg[1..arg.len()]
                };
                if batch_cmd.env.len() > 0 {
                    command.arg(replace_env_vars(arg_str, &batch_cmd.env));
                } else {
                    command.arg(arg_str);
                }
            }
            set_cmd_stdio(&mut command, batch_cmd.stdio.unwrap_or_default());
            match command.spawn() {
                Ok(child) => return Some(child),
                Err(_) => {
                    if !fastpath_fallback {
                        return None;
                    }
                } // fallback to shell
            }
        }
    }

    let mut command = Command::new("bash");
    command.env("PATH", path);
    for (name, value) in &batch_cmd.env {
        command.env(name, value);
    }
    command.current_dir(cwd);
    command.arg("-e");
    command.arg("-c");
    command.arg(&run);
    set_cmd_stdio(&mut command, batch_cmd.stdio.unwrap_or_default());
    Some(command.spawn().unwrap())
}

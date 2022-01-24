use std::process::Stdio;
use crate::engines::BatchCmd;
use tokio::process::{Child, Command};
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
pub fn create_cmd(cwd: &str, batch_cmd: &BatchCmd, debug: bool) -> Child {
    let run = batch_cmd.run.trim();
    if debug {
        println!("RUN: {}", run);
    }
    lazy_static! {
        static ref CMD: Regex = Regex::new("(?x)
            ^(?P<cmd>[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+?)
             (?P<args>(?:\\ (?:
                [^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?')
            )*?)*?)$
        ").unwrap();
        
        static ref ARGS: Regex = Regex::new("(?x)
            \\ (?:[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?'))
        ").unwrap();
    }
    let mut path: String = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ";";
    }
    path.push_str(cwd);
    path += ".bin;";
    path.push_str(cwd);
    path += "\\node_modules\\.bin";
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
            // Try ".cmd" extension first
            // Note: this requires latest Rust version
            let mut cmd_with_ext = cmd.to_owned();
            cmd_with_ext.push_str(".cmd");
            let mut command = Command::new(&cmd_with_ext);
            command.env("PATH", &path);
            for (name, value) in &batch_cmd.env {
                command.env(name, value);
            }
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
            command.stdin(Stdio::null());
            match command.spawn() {
                Ok(child) => return child,
                Err(_) => {
                    let mut command = Command::new(&cmd);
                    command.env("PATH", &path);
                    for (name, value) in &batch_cmd.env {
                        command.env(name, value);
                    }
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
                    command.stdin(Stdio::null());
                    match command.spawn() {
                        Ok(child) => return child,
                        Err(_) => {}, // fallback to shell
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
        for (name, value) in &batch_cmd.env {
            run_str.push_str(&format!("${}=\"{}\";", name, value));
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
    command.stdin(Stdio::null());
    command.spawn().unwrap()
}

#[cfg(not(target_os = "windows"))]
pub fn create_cmd(cwd: &str, batch_cmd: &BatchCmd, debug: bool) -> Child {
    let run = batch_cmd.run.trim();
    if debug {
        println!("RUN: {}", run);
    }
    lazy_static! {
        static ref CMD: Regex = Regex::new("(?x)
            ^(?P<cmd>[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+?)
             (?P<args>(?:\\ (?:
                [^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?')
            )*?)*?)$
        ").unwrap();
        
        static ref ARGS: Regex = Regex::new("(?x)
            \\ (?:[^`~!\\#&*()\t\\{\\[|;'\"\\n<>?\\\\\\ ]+ |
                (?:\"[^\"\\n\\\\]*?\") |
                (?:'[^'\"\\n\\\\]*?'))
        ").unwrap();
    }
    let mut path: String = env::var("PATH").unwrap_or_default();
    if path.len() > 0 {
        path += ";";
    }
    path.push_str(cwd);
    path += ".bin;";
    path.push_str(cwd);
    path += "\\node_modules\\.bin";
    // fast path for direct commands to skip the shell entirely
    if let Some(capture) = CMD.captures(&run) {
        let mut cmd = String::from(&capture["cmd"]);
        // Path-like must be exact
        if cmd.contains("/") {
            let canonical = fs::canonicalize(PathBuf::from(cmd.clone())).unwrap();
            cmd = String::from(&canonical.to_str().unwrap()[4..]);
        }
        let mut command = Command::new(&cmd);
        command.env("PATH", &path);
        for (name, value) in &batch_cmd.env {
            command.env(name, value);
        }
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
        command.stdin(Stdio::null());
        match command.spawn() {
            Ok(child) => return child,
            Err(_) => {}, // fallback to shell
        }
    }

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
    for (name, value) in &batch_cmd.env {
        command.env(name, value);
    }
    command.arg("-c");
    command.arg(&run);
    command.stdin(Stdio::null());
    command.spawn().unwrap()
}

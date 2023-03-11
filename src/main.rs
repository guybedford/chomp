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

extern crate clap;
#[macro_use]
extern crate lazy_static;
use crate::chompfile::ChompTaskMaybeTemplated;
use crate::chompfile::Chompfile;
use crate::extensions::expand_template_tasks;
use crate::extensions::init_js_platform;
use crate::extensions::ExtensionEnvironment;
use crate::task::Runner;
use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, Command};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
extern crate num_cpus;
use crate::engines::replace_env_vars_static;
use hyper::Uri;
use std::env;
use std::fs::canonicalize;
use tokio::sync::mpsc::unbounded_channel;

mod ansi_windows;
mod chompfile;
mod engines;
mod extensions;
mod http_client;
mod server;
mod task;

use std::path::PathBuf;

const CHOMP_CORE: &str = "https://ga.jspm.io/npm:@chompbuild/extensions@0.1.28/";

const CHOMP_INIT: &str = r#"version = 0.1

[[task]]
name = 'build'
run = 'echo \"Build script goes here\"'
"#;

const CHOMP_EMPTY: &str = "version = 0.1\n";

fn uri_parse(uri_str: &str) -> Option<Uri> {
    match uri_str.parse::<Uri>() {
        Ok(uri) => match uri.scheme_str() {
            Some(_) => Some(uri),
            None => None,
        },
        Err(_) => None,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(not(debug_assertions))]
    let version = "0.2.12";
    #[cfg(debug_assertions)]
    let version = "0.2.12-debug";
    let matches = Command::new("Chomp")
        .version(version)
        .arg(
            Arg::new("watch")
                .short('w')
                .long("watch")
                .help("Watch the input files for changes")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("serve")
                .short('s')
                .long("serve")
                .help("Run a local dev server")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("server-root")
                .short('R')
                .long("server-root")
                .help("Server root path")
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .help("Custom port to serve")
        )
        .arg(
            Arg::new("jobs")
                .short('j')
                .long("jobs")
                .value_name("N")
                .value_parser(clap::value_parser!(usize))
                .help("Maximum number of jobs to run in parallel")
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("CONFIG")
                .default_value("chompfile.toml")
                .help("Custom chompfile path")
        )
        .arg(
            Arg::new("list")
                .short('l')
                .long("list")
                .help("List the available chompfile tasks")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("format")
                .short('F')
                .long("format")
                .help("Format and save the chompfile.toml")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("eject_templates")
                .long("eject")
                .help("Ejects templates into tasks saving the rewritten chompfile.toml")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("init")
                .short('i')
                .long("init")
                .help("Initialize a new chompfile.toml if it does not exist")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("import_scripts")
                .short('I')
                .long("import-scripts")
                .help("Import from npm \"scripts\" into the chompfile.toml")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("clear_cache")
                .short('C')
                .long("clear-cache")
                .help("Clear URL extension cache")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("rerun")
                .short('r')
                .long("rerun")
                .help("Rerun the target tasks even if cached")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Force rebuild targets")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("target")
                .value_name("TARGET")
                .help("Generate a target or list of targets")
                .action(ArgAction::Append)
        )
        .arg(
            Arg::new("arg")
                .last(true)
                .value_name("ARGS")
                .help("Custom task args")
                .action(ArgAction::Append)
        )
        .get_matches();

    #[cfg(target_os = "windows")]
    match ansi_windows::enable_ansi_support() {
        Ok(()) => {}
        Err(_) => {
            // TODO: handling disabling of ansi codes
        }
    };

    let mut targets: Vec<String> = Vec::new();
    let mut use_default_target = true;
    match matches.get_many::<String>("target") {
        Some(target) => {
            for item in target {
                targets.push(item.to_string());
            }
        }
        None => {}
    }

    let cfg_path = Path::new(matches.get_one::<String>("config").unwrap());
    let cfg_dir = cfg_path.parent().unwrap().to_str().unwrap();
    let mut cfg_file = canonicalize(if cfg_dir.len() == 0 { "." } else { cfg_dir }).unwrap();
    cfg_file.push(cfg_path.file_name().unwrap());

    let mut created = false;
    let chompfile_source = {
        let is_dir: bool = match fs::metadata(&cfg_file) {
            Ok(meta) => meta.is_dir(),
            Err(_) => false,
        };
        if is_dir {
            cfg_file.push("chompfile.toml");
        }
        match fs::read_to_string(&cfg_file) {
            Ok(source) => source,
            Err(_) => {
                if matches.get_flag("init") {
                    created = true;
                    if matches.get_flag("import_scripts") {
                        String::from(CHOMP_EMPTY)
                    } else {
                        String::from(CHOMP_INIT)
                    }
                } else {
                    if matches.get_flag("serve") {
                        String::from(CHOMP_EMPTY)
                    } else {
                        return Err(anyhow!(
                            "Unable to load the Chomp configuration {}. Pass the \x1b[1m--init\x1b[0m flag to create one, or try:\n\n\x1b[36mchomp --init --import-scripts\x1b[0m\n\nto create one and import from existing package.json scripts.",
                            &cfg_file.to_str().unwrap()
                        ));
                    }
                }
            }
        }
    };
    let mut chompfile: Chompfile = toml::from_str(&chompfile_source)?;
    if chompfile.version != 0.1 {
        return Err(anyhow!(
            "Invalid chompfile version {}, only 0.1 is supported",
            chompfile.version
        ));
    }

    let cwd = {
        let mut parent: PathBuf = PathBuf::from(cfg_file.parent().unwrap());
        if parent.to_str().unwrap().len() == 0 {
            parent = env::current_dir()?;
        }
        let unc_path = match canonicalize(&parent) {
            Ok(path) => path,
            Err(_) => {
                return Err(anyhow!(
                    "Unable to load the Chomp configuration {}.\nMake sure it exists in the current directory, or use --config to set a custom path.",
                    &cfg_file.to_str().unwrap()
                ));
            }
        };
        let unc_str = unc_path.to_str().unwrap();
        if unc_str.starts_with(r"\\?\") {
            PathBuf::from(String::from(&unc_path.to_str().unwrap()[4..]))
        } else {
            unc_path
        }
    };
    assert!(env::set_current_dir(&cwd).is_ok());

    if matches.get_flag("clear_cache") {
        http_client::clear_cache().await?;
        println!("\x1b[1;32m√\x1b[0m Cleared remote URL extension cache.");
        if targets.len() == 0 {
            return Ok(());
        }
    }

    init_js_platform();

    let pool_size = match matches.get_one::<usize>("jobs") {
        Some(&jobs) => jobs,
        None => num_cpus::get(),
    };

    let mut global_env = BTreeMap::new();
    for (key, value) in env::vars() {
        global_env.insert(key.to_uppercase(), value);
    }
    for (key, value) in &chompfile.env {
        global_env.insert(
            key.to_uppercase(),
            replace_env_vars_static(value, &global_env),
        );
    }
    if matches.get_flag("eject_templates") {
        global_env.insert("CHOMP_EJECT".to_string(), "1".to_string());
    }
    global_env.insert("CHOMP_POOL_SIZE".to_string(), pool_size.to_string());
    // extend global env with the chompfile env as well
    for (key, value) in &chompfile.env_default {
        if !global_env.contains_key(&key.to_uppercase()) {
            global_env.insert(
                key.to_uppercase(),
                replace_env_vars_static(value, &global_env),
            );
        }
    }

    let mut extension_env = ExtensionEnvironment::new(&global_env);

    http_client::prep_cache().await?;
    let mut extension_set: HashSet<String> = HashSet::new();
    let mut extensions = chompfile.extensions.clone();
    let mut i = 0;
    while i < extensions.len() {
        if extensions[i].starts_with("chomp:") {
            return Err(anyhow!("Chomp core extensions must be versioned - try \x1b[36m'chomp@0.1:{}'\x1b[0m instead", &extensions[i][6..]));
        }
        let ext = if extensions[i].starts_with("chomp@0.1:") {
            let mut s: String = match global_env.get("CHOMP_CORE") {
                Some(path) => String::from(path),
                None => String::from(CHOMP_CORE),
            };
            if !s.ends_with("/") && !s.ends_with("\\") {
                s.push_str("/");
            }
            s.push_str(&extensions[i][10..]);
            s.push_str(".js");
            s
        } else {
            extensions[i].clone()
        };
        let (canonical, extension_source) = match uri_parse(ext.as_ref()) {
            Some(uri) => {
                if !extension_set.contains(&ext) {
                    extension_set.insert(ext.to_string());
                    (
                        extension_set.get(&ext).unwrap(),
                        Some(http_client::fetch_uri_cached(&ext, uri).await?),
                    )
                } else {
                    (extension_set.get(&ext).unwrap(), None)
                }
            }
            None => {
                let canonical_str: String = match canonicalize(&ext) {
                    Ok(canonical) => canonical.to_str().unwrap().replace("\\", "/"),
                    Err(_) => {
                        return Err(anyhow!("Unable to read extension file '{}'.", &ext));
                    }
                };
                if !extension_set.contains(&canonical_str) {
                    extension_set.insert(canonical_str.to_string());
                    (
                        extension_set.get(&canonical_str).unwrap(),
                        Some(fs::read_to_string(&ext)?),
                    )
                } else {
                    (extension_set.get(&canonical_str).unwrap(), None)
                }
            }
        };
        if let Some(extension_source) = extension_source {
            match extension_env.add_extension(&extension_source, canonical)? {
                Some(mut new_includes) => {
                    for ext in new_includes.drain(..) {
                        // relative includes are relative to the parent
                        if ext.starts_with("./") {
                            let mut resolved_str =
                                canonical[0..canonical.rfind("/").unwrap() + 1].to_string();
                            resolved_str.push_str(&ext[2..]);
                            extensions.push(resolved_str);
                        } else {
                            extensions.push(ext);
                        }
                    }
                }
                None => {}
            }
        }
        i = i + 1;
    }
    extension_env.seal_extensions();

    // channel for watch events
    let (watch_event_sender, watch_event_receiver) = unbounded_channel();
    // channel for adding new files to watcher
    let (watch_sender, watch_receiver) = unbounded_channel();
    let mut serve_options = chompfile.server.clone();
    {
        if let Some(root) = matches.get_one::<String>("server-root") {
            serve_options.root = root.to_string();
        }
        if let Some(&port) = matches.get_one::<u16>("port") {
            serve_options.port = port;
        }
        if matches.get_flag("serve") {
            use_default_target = false;
            tokio::spawn(server::serve(
                serve_options,
                watch_event_receiver,
                watch_sender,
            ));
        }
    }

    let mut args: Vec<String> = Vec::new();
    if let Some(arg) = matches.get_many::<String>("arg") {
        for item in arg {
            args.push(item.to_string());
        }
    }

    if matches.get_flag("import_scripts") {
        if matches.get_flag("eject_templates") {
            return Err(anyhow!(
                "Cannot use --import-scripts and --eject-templates together."
            ));
        }
        let mut script_tasks = 0;
        let pjson_source = match fs::read_to_string("package.json") {
            Ok(source) => source,
            Err(_) => {
                return Err(anyhow!(
                    "No package.json to import found in the current project directory."
                ));
            }
        };

        let pjson: serde_json::Value = serde_json::from_str(&pjson_source)?;
        match &pjson["scripts"] {
            serde_json::Value::Object(scripts) => {
                for (name, val) in scripts.iter() {
                    if let serde_json::Value::String(run) = &val {
                        script_tasks = script_tasks + 1;
                        let mut task = ChompTaskMaybeTemplated::new();
                        task.name = Some(name.to_string());
                        task.run = Some(run.to_string());
                        chompfile.task.push(task);
                    }
                }
            }
            _ => return Err(anyhow!("Unexpected \"scripts\" type in package.json.")),
        };
        fs::write(&cfg_file, toml::to_string_pretty(&chompfile)?)?;
        println!(
            "\x1b[1;32m√\x1b[0m \x1b[1m{}\x1b[0m {}.",
            cfg_file.to_str().unwrap(),
            if created {
                format!(
                    "created with {} package.json script tasks imported",
                    script_tasks
                )
            } else {
                format!(
                    "updated with {} package.json script tasks imported",
                    script_tasks
                )
            }
        );
        return Ok(());
    }

    let cwd_str = cwd.to_string_lossy().replace('\\', "/");
    let (mut has_templates, mut template_tasks) =
        expand_template_tasks(&chompfile, &mut extension_env, &cwd_str)?;
    chompfile.task = Vec::new();
    for task in extension_env.get_tasks().drain(..) {
        has_templates = true;
        chompfile.task.push(task.into());
    }
    chompfile.task.append(&mut template_tasks);

    if matches.get_flag("list") {
        if targets.len() > 0 {
            return Err(anyhow!("--list does not take any arguments."));
        }
        if matches.get_flag("eject_templates")
            || matches.get_flag("format")
            || matches.get_flag("init")
        {
            return Err(anyhow!(
                "Cannot use --list with --eject-templates, --format or --init."
            ));
        }
        for task in &chompfile.task {
            if let Some(name) = &task.name {
                let matches_some_target = if targets.len() > 0 {
                    let mut matches_some_target = false;
                    for target in &targets {
                        if name.starts_with(target) {
                            matches_some_target = true;
                        }
                    }
                    matches_some_target
                } else {
                    true
                };
                if matches_some_target {
                    println!(" \x1b[1m▪\x1b[0m {}", name);
                }
            }
        }
        return Ok(());
    }

    if matches.get_flag("format")
        || matches.get_flag("eject_templates")
        || matches.get_flag("init")
    {
        use_default_target = false;
        if matches.get_flag("eject_templates") {
            if !has_templates {
                return Err(anyhow!(
                    "\x1b[1m{}\x1b[0m has no templates to eject",
                    cfg_file.to_str().unwrap()
                ));
            }
            chompfile.extensions = Vec::new();
            chompfile.template_options = HashMap::new();
        }

        fs::write(&cfg_file, toml::to_string_pretty(&chompfile)?)?;
        if matches.get_flag("eject_templates") {
            println!(
                "\x1b[1;32m√\x1b[0m \x1b[1m{}\x1b[0m template tasks ejected.",
                cfg_file.to_str().unwrap()
            );
        } else {
            println!(
                "\x1b[1;32m√\x1b[0m \x1b[1m{}\x1b[0m {}.",
                cfg_file.to_str().unwrap(),
                if created { "created" } else { "updated" }
            );
        }
        if matches.get_flag("eject_templates") || targets.len() == 0 {
            return Ok(());
        }
    }

    let targets = if targets.len() == 0 && use_default_target {
        vec![chompfile.default_task.to_owned().unwrap_or(String::from("build"))]
    } else {
        targets
    };

    let mut runner = Runner::new(
        &chompfile,
        &mut extension_env,
        pool_size,
        matches.get_flag("serve") || matches.get_flag("watch"),
    )?;
    let ok = runner
        .run(
            task::RunOptions {
                watch: matches.get_flag("serve") || matches.get_flag("watch"),
                force: matches.get_flag("force"),
                rerun: matches.get_flag("rerun"),
                args: if args.len() > 0 { Some(args) } else { None },
                pool_size,
                targets,
                cfg_file,
            },
            watch_event_sender,
            watch_receiver,
        )
        .await?;

    if !ok {
        eprintln!("Unable to complete all tasks.");
    }

    std::process::exit(if ok { 0 } else { 1 });
}

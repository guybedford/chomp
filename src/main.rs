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
use crate::task::Runner;
use crate::chompfile::ChompTaskMaybeTemplated;
use crate::chompfile::Chompfile;
use crate::extensions::init_js_platform;
use crate::extensions::ExtensionEnvironment;
use crate::task::expand_template_tasks;
use anyhow::{anyhow, Result};
use clap::{App, Arg};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
extern crate num_cpus;
use hyper::Uri;
use std::env;
use std::fs::canonicalize;
use crate::engines::replace_env_vars_static;

mod ansi_windows;
mod chompfile;
mod engines;
mod extensions;
mod http_client;
mod serve;
mod task;

use std::path::PathBuf;

const CHOMP_CORE: &str = "https://ga.jspm.io/npm:@chompbuild/extensions@0.1.12/";

const CHOMP_INIT: &str = r#"version = 0.1

default-task = 'build'

[[task]]
name = 'build'
run = 'echo \"Build script goes here\"'
"#;

const CHOMP_INIT_SCRIPTS: &str = "version = 0.1\n";

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
    let matches = App::new("Chomp")
        .version("0.1.0")
        .arg(
            Arg::with_name("watch")
                .short("w")
                .long("watch")
                .help("Watch the input files for changes"),
        )
        .arg(
            Arg::with_name("serve")
                .short("s")
                .long("serve")
                .help("Run a local dev server"),
        )
        .arg(
            Arg::with_name("server-root")
                .short("R")
                .long("server-root")
                .help("Server root path")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .help("Custom port to serve")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("jobs")
                .short("j")
                .long("jobs")
                .value_name("N")
                .help("Maximum number of jobs to run in parallel")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("CONFIG")
                .default_value("chompfile.toml")
                .help("Custom chompfile path")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("list")
                .short("l")
                .long("list")
                .help("List the available chompfile tasks"),
        )
        .arg(
            Arg::with_name("format")
                .short("F")
                .long("format")
                .help("Format and save the chompfile.toml"),
        )
        .arg(
            Arg::with_name("eject_templates")
                .long("eject")
                .help("Ejects templates into tasks saving the rewritten chompfile.toml"),
        )
        .arg(
            Arg::with_name("init")
                .short("i")
                .long("init")
                .help("Initialize a new chompfile.toml if it does not exist"),
        )
        .arg(
            Arg::with_name("import_scripts")
                .short("I")
                .long("import-scripts")
                .help("Import from npm \"scripts\" into the chompfile.toml"),
        )
        .arg(
            Arg::with_name("clear_cache")
                .short("C")
                .long("clear-cache")
                .help("Clear URL extension cache"),
        )
        .arg(
            Arg::with_name("rerun")
                .short("r")
                .long("rerun")
                .help("Rerun the target tasks even if cached"),
        )
        .arg(
            Arg::with_name("force")
                .short("f")
                .long("force")
                .help("Force rebuild targets"),
        )
        .arg(
            Arg::with_name("target")
                .value_name("TARGET")
                .help("Generate a target or list of targets")
                .multiple(true),
        )
        .arg(
            Arg::with_name("arg")
                .last(true)
                .value_name("ARGS")
                .help("Custom task args")
                .multiple(true),
        )
        .get_matches();

    #[cfg(target_os = "windows")]
    match ansi_windows::enable_ansi_support() {
        Ok(()) => {},
        Err(_) => {
            // TODO: handling disabling of ansi codes
        }
    };

    let mut targets: Vec<String> = Vec::new();
    match matches.values_of("target") {
        Some(target) => {
            for item in target {
                targets.push(String::from(item));
            }
        }
        None => {}
    }

    let cfg_file = PathBuf::from(matches.value_of("config").unwrap_or_default());

    let mut created = false;
    let chompfile_source = match fs::read_to_string(&cfg_file) {
        Ok(source) => source,
        Err(_) => {
            if matches.is_present("init") {
                created = true;
                if matches.is_present("import_scripts") {
                    String::from(CHOMP_INIT_SCRIPTS)
                } else {
                    String::from(CHOMP_INIT)
                }
            } else {
                return Err(anyhow!(
                    "Unable to load the Chomp configuration {}. Pass the \x1b[1m--init\x1b[0m flag to create one, or try:\n\n\x1b[36mchomp --init --import-scripts\x1b[0m\n\nto create one and import from existing package.json scripts.",
                    &cfg_file.to_str().unwrap()
                ));
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

    if matches.is_present("clear_cache") {
        http_client::clear_cache().await?;
        println!("\x1b[1;32m√\x1b[0m Cleared remote URL extension cache.");
        if targets.len() == 0 {
            return Ok(());
        }
    }

    init_js_platform();

    let pool_size = match matches.value_of("jobs") {
        Some(jobs) => jobs.parse()?,
        None => num_cpus::get(),
    };

    let mut global_env = BTreeMap::new();
    for (key, value) in env::vars() {
        global_env.insert(key.to_uppercase(), value);
    }
    for (key, value) in &chompfile.env {
        global_env.insert(key.to_uppercase(), replace_env_vars_static(value, &global_env));
    }
    if matches.is_present("eject_templates") {
        global_env.insert("CHOMP_EJECT".to_string(), "1".to_string());
    }
    global_env.insert("CHOMP_POOL_SIZE".to_string(), pool_size.to_string());
    // extend global env with the chompfile env as well
    for (key, value) in &chompfile.env_default {
        global_env.insert(key.to_uppercase(), replace_env_vars_static(value, &global_env));
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

    let mut serve_options = chompfile.server.clone();
    {
        if let Some(root) = matches.value_of("server-root") {
            serve_options.root = root.to_string();
        }
        if let Some(port) = matches.value_of("port") {
            serve_options.port = port.parse().unwrap();
        }
        if matches.is_present("serve") {
            tokio::spawn(async move {
                if let Err(e) = serve::serve(serve_options).await {
                    eprintln!("{:?}", e);
                    std::process::exit(1);
                }
            });
        }
    }

    let mut args: Vec<String> = Vec::new();
    if let Some(arg) = matches.values_of("arg") {
        for item in arg {
            args.push(String::from(item));
        }
    }

    if matches.is_present("format")
        || matches.is_present("eject_templates")
        || matches.is_present("list")
        || matches.is_present("import_scripts")
    {
        if matches.is_present("eject_templates") {
            let (mut has_templates, mut template_tasks) =
                expand_template_tasks(&chompfile, &mut extension_env)?;
            chompfile.task = Vec::new();
            for task in extension_env.get_tasks().drain(..) {
                has_templates = true;
                chompfile.task.push(ChompTaskMaybeTemplated {
                    target: task.target,
                    targets: task.targets,
                    args: task.args,
                    cwd: task.cwd,
                    dep: task.dep,
                    deps: task.deps,
                    display: task.display,
                    engine: task.engine,
                    env: task.env.unwrap_or_default(),
                    env_default: task.env_default.unwrap_or_default(),
                    env_replace: task.env_replace,
                    invalidation: task.invalidation,
                    validation: task.validation,
                    run: task.run,
                    name: task.name,
                    serial: task.serial,
                    stdio: task.stdio,
                    template: task.template,
                    template_options: task.template_options
                });
            }
            chompfile.task.append(&mut template_tasks);
            if !has_templates {
                return Err(anyhow!(
                    "\x1b[1m{}\x1b[0m has no templates to eject",
                    cfg_file.to_str().unwrap()
                ));
            }
            chompfile.extensions = Vec::new();
        }

        if matches.is_present("list") {
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
        } else {
            let mut script_tasks = 0;
            if matches.is_present("import_scripts") {
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
                                chompfile.task.push(ChompTaskMaybeTemplated {
                                    name: Some(name.to_string()),
                                    run: Some(run.to_string()),
                                    args: None,
                                    cwd: None,
                                    deps: None,
                                    dep: None,
                                    targets: None,
                                    target: None,
                                    display: None,
                                    engine: None,
                                    env_replace: None,
                                    env: HashMap::new(),
                                    env_default: HashMap::new(),
                                    invalidation: None,
                                    validation: None,
                                    serial: None,
                                    stdio: None,
                                    template: None,
                                    template_options: None,
                                })
                            }
                        }
                    }
                    _ => return Err(anyhow!("Unexpected \"scripts\" type in package.json.")),
                };
            }
            fs::write(&cfg_file, toml::to_string_pretty(&chompfile)?)?;
            if matches.is_present("eject_templates") {
                println!(
                    "\x1b[1;32m√\x1b[0m \x1b[1m{}\x1b[0m template tasks ejected.",
                    cfg_file.to_str().unwrap()
                );
            } else if matches.is_present("import_scripts") {
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
            } else {
                println!(
                    "\x1b[1;32m√\x1b[0m \x1b[1m{}\x1b[0m {}.",
                    cfg_file.to_str().unwrap(),
                    if created { "created" } else { "updated" }
                );
            }
        }
        if matches.is_present("eject_templates") || targets.len() == 0 {
            return Ok(());
        }
    }

    let mut runner = Runner::new(&chompfile, &mut extension_env, pool_size, matches.is_present("serve") || matches.is_present("watch"))?;
    let ok = runner.run(task::RunOptions {
        watch: matches.is_present("serve") || matches.is_present("watch"),
        force: matches.is_present("force"),
        rerun: matches.is_present("rerun"),
        args: if args.len() > 0 { Some(args) } else { None },
        pool_size,
        targets,
        cfg_file,
    }).await?;

    if !ok {
        eprintln!("Unable to complete all tasks.");
    }

    std::process::exit(if ok { 0 } else { 1 });
}

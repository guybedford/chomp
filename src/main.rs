extern crate clap;
#[macro_use]
extern crate lazy_static;
use std::collections::HashSet;
use crate::extensions::ExtensionEnvironment;
use crate::task::expand_template_tasks;
use crate::chompfile::Chompfile;
use clap::{App, Arg};
use anyhow::{Result, anyhow};
use tokio::fs;
use std::collections::HashMap;
use crate::extensions::init_js_platform;
extern crate num_cpus;
use hyper::Uri;
use std::env;
use std::fs::canonicalize;

mod task;
mod chompfile;
mod engines;
mod extensions;
mod http_client;
mod serve;

use std::path::PathBuf;

const CHOMP_CORE: &str = "https://ga.jspm.io/npm:@chompbuild/extensions@0.1.0/";

fn uri_parse (uri_str: &str) -> Option<Uri> {
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
                .short("r")
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
            .help("List the available chompfile tasks")
        )
        .arg(
            Arg::with_name("format")
                .short("F")
                .long("format")
                .help("Format and save the chompfile.toml")
        )
        .arg(
            Arg::with_name("eject_templates")
                .long("eject")
                .help("Ejects templates into tasks saving the rewritten chompfile.toml")
        )
        .arg(
            Arg::with_name("clear_cache")
                .short("C")
                .long("clear-cache")
                .help("Clear URL extension cache"),
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
                .multiple(true)
        )
        .arg(
            Arg::with_name("arg")
                .last(true)
                .value_name("ARGS")
                .help("Custom task args")
                .multiple(true)
        )
        .get_matches();

    let mut targets: Vec<String> = Vec::new();
    match matches.values_of("target") {
        Some(target) => {
            for item in target {
                targets.push(String::from(item));
            }
        },
        None => {}
    }

    let cfg_file = PathBuf::from(matches.value_of("config").unwrap_or_default());
    let canonical_file = {
        let unc_path = match canonicalize(&cfg_file) {
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
    let cwd = canonical_file.parent().unwrap();

    let chompfile_source = match fs::read_to_string(&cfg_file).await {
        Ok(source) => source,
        Err(_) => {
            return Err(anyhow!(
                "Unable to load the Chomp configuration {}.",
                &cfg_file.to_str().unwrap()
            ));
        }
    };
    let mut chompfile: Chompfile = toml::from_str(&chompfile_source)?;
    if chompfile.version != 0.1 {
        return Err(anyhow!(
            "Invalid chompfile version {}, only 0.1 is supported",
            chompfile.version
        ));
    }

    if matches.is_present("clear_cache") {
        println!("Clearing URL extension cache...");
        http_client::clear_cache().await?;
    }

    init_js_platform();

    let mut global_env = HashMap::new();
    for (key, value) in env::vars() {
        global_env.insert(key.to_uppercase(), value);
    }
    if matches.is_present("eject_templates") {
        global_env.insert("CHOMP_EJECT".to_string(), "1".to_string());
    }

    let mut extension_env = ExtensionEnvironment::new(&global_env);

    http_client::prep_cache().await?;
    let mut extension_set: HashSet<String> = HashSet::new();
    let mut extensions = chompfile.extensions.clone();
    let mut i = 0;
    while i < extensions.len() {
        let ext = if extensions[i].starts_with("chomp:") {
            let mut s: String = match global_env.get("CHOMP_CORE") {
                Some(path) => String::from(path),
                None => String::from(CHOMP_CORE),
            };
            if !s.ends_with("/") && !s.ends_with("\\") {
                s.push_str("/");
            }
            s.push_str(&extensions[i][6..]);
            s.push_str(".js");
            s
        } else {
            extensions[i].clone()
        };
        let (canonical, extension_source) = match uri_parse(ext.as_ref()) {
            Some(uri) => {
                if !extension_set.contains(&ext) {
                    extension_set.insert(ext.to_string());
                    (extension_set.get(&ext).unwrap(), Some(http_client::fetch_uri_cached(&ext, uri).await?))
                } else {
                    (extension_set.get(&ext).unwrap(), None)
                }
            },
            None => {
                let canonical_str: String = match canonicalize(&ext) {
                    Ok(canonical) => canonical.to_str().unwrap().replace("\\", "/"),
                    Err(_) => {
                        return Err(anyhow!("Unable to read extension file '{}'.", &ext));
                    }
                };
                if !extension_set.contains(&canonical_str) {
                    extension_set.insert(canonical_str.to_string());
                    (extension_set.get(&canonical_str).unwrap(), Some(fs::read_to_string(&ext).await?))
                } else {
                    (extension_set.get(&canonical_str).unwrap(), None)
                }
            },
        };
        if let Some(extension_source) = extension_source {
            match extension_env.add_extension(&extension_source, canonical)? {
                Some(mut new_includes) => {
                    for ext in new_includes.drain(..) {
                        // relative includes are relative to the parent
                        if ext.starts_with("./") {
                            let mut resolved_str = canonical[0..canonical.rfind("/").unwrap() + 1].to_string();
                            resolved_str.push_str(&ext[2..]);
                            extensions.push(resolved_str);
                        } else {
                            extensions.push(ext);
                        }
                    }
                },
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

    if matches.is_present("format") || matches.is_present("eject_templates") || matches.is_present("list") {
        if matches.is_present("eject_templates") {
            chompfile.task = expand_template_tasks(&chompfile, &mut extension_env)?;
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
                    } else { true };
                    if matches_some_target {
                        println!(" \x1b[1m▪\x1b[0m {}", name);
                    }
                }
            }
            return Ok(());
        } else {
            fs::write(&cfg_file, toml::to_string_pretty(&chompfile)?).await?;
            if targets.len() == 0 {
                return Ok(());
            }
        }
    }

    let pool_size = match matches.value_of("jobs") {
        Some(jobs) => jobs.parse()?,
        None => num_cpus::get(),
    };

    let ok = task::run(&chompfile, &mut extension_env, task::RunOptions {
        watch: matches.is_present("serve") || matches.is_present("watch"),
        force: matches.is_present("force"),
        args: if args.len() > 0 { Some(args) } else { None },
        pool_size,
        cwd: cwd.to_str().unwrap().to_string(),
        targets,
        cfg_file,
    }).await?;

    if !ok {
        eprintln!("Unable to complete all tasks.");
    }

    std::process::exit(if ok { 0 } else { 1 });
}

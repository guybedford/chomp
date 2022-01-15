extern crate clap;
#[macro_use]
extern crate lazy_static;
use crate::chompfile::Chompfile;
use clap::{App, Arg};
use std::io::stdout;
use anyhow::{Result, anyhow};
use async_std::fs;

use crossterm::tty::IsTty;

mod task;
mod chompfile;
mod engines;
mod ui;
mod serve;
mod js;

use std::path::PathBuf;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new("Chomp")
        .version("0.1.0")
        .about("ᗧ h o m p • ᗣ")
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
            Arg::with_name("j")
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
            Arg::with_name("force")
                .short("f")
                .long("force")
                .help("Force rebuild targets"),
        )
        .arg(
            Arg::with_name("target")
                .value_name("TARGET")
                .help("Generate a target or list of targets")
                .default_value("build")
                .multiple(true)
        )
        // .arg(
        //     Arg::with_name("arg")
        //         .last(true)
        //         .value_name("ARGS")
        //         .help("Custom task args")
        //         .multiple(true)
        // )
        .get_matches();

    let ui = ui::ChompUI::new(stdout().is_tty());
    // ui.create_box()?;

    let mut targets: Vec<String> = Vec::new();
    for item in matches.values_of("target").unwrap() {
        targets.push(String::from(item));
    }

    let cfg_file = PathBuf::from(matches.value_of("config").unwrap_or_default());

    let chompfile_source = fs::read_to_string(&cfg_file).await?;
    let mut chompfile: Chompfile = toml::from_str(&chompfile_source)?;
    {
        let mut default_chompfile: Chompfile = toml::from_str(include_str!("templates.toml")).unwrap();
        for template in default_chompfile.template.drain(..) {
            chompfile.template.push(template);
        }
        for task in default_chompfile.task.drain(..) {
            chompfile.task.push(task);
        }

        if chompfile.version != 0.1 {
            return Err(anyhow!(
                "Invalid chompfile version {}, only 0.1 is supported",
                chompfile.version
            ));
        }
    }

    let mut serve_options = chompfile.server.clone();
    {
        if let Some(root) = matches.value_of("serve-root") {
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

    // let mut args: Vec<String> = Vec::new();
    // for item in matches.values_of("arg").unwrap() {
    //     args.push(String::from(item));
    // }

    let ok = task::run(&chompfile, task::RunOptions {
        watch: matches.is_present("serve") || matches.is_present("watch"),
        force: matches.is_present("force"),
        ui: &ui,
        cwd: env::current_dir()?,
        targets,
        cfg_file,
    }).await?;

    if !ok {
        eprintln!("Unable to complete all tasks.");
    }

    std::process::exit(if ok { 0 } else { 1 });
}

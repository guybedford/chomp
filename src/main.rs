extern crate clap;
#[macro_use]
extern crate lazy_static;
use clap::{App, Arg};
use std::io::stdout;

use crossterm::tty::IsTty;

mod task;
mod engines;
mod ui;
mod serve;

use std::path::PathBuf;
use std::env;

#[derive(Debug)]
enum ChompError {
    IoError(std::io::Error),
    TaskError(task::TaskError),
}

impl From<std::io::Error> for ChompError {
    fn from(e: std::io::Error) -> ChompError {
        ChompError::IoError(e)
    }
}

impl From<task::TaskError> for ChompError {
    fn from(e: task::TaskError) -> ChompError {
        ChompError::TaskError(e)
    }
}

#[tokio::main]
async fn main() -> Result<(), ChompError> {
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
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .help("Custom port to serve")
                .default_value("8080")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("i")
                .short("i")
                .help("Initialize template via stdin")
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

    let port = matches.value_of("port").unwrap_or("8080").parse().unwrap();

    if matches.is_present("serve") {
        tokio::spawn(async move {
            if let Err(e) = serve::serve(serve::ServeOptions {
                port,
            }).await {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        });
    }

    // let mut args: Vec<String> = Vec::new();
    // for item in matches.values_of("arg").unwrap() {
    //     args.push(String::from(item));
    // }

    task::run(task::RunOptions {
        watch: matches.is_present("serve") || matches.is_present("watch"),
        ui: &ui,
        cwd: env::current_dir()?,
        targets,
        cfg_file: PathBuf::from(matches.value_of("config").unwrap_or_default()),
    }).await?;

    Ok(())
}

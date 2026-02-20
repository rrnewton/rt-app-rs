mod args;
// Syscall wrappers are building blocks for the rt-app port; not all items are
// consumed from main.rs yet, but they will be as more modules are ported.
#[allow(dead_code)]
mod syscalls;

use clap::Parser;

use crate::args::{Cli, ConfigSource};

fn main() {
    let cli = Cli::parse();

    let log_level = cli.log_level.value();

    match cli.config_source() {
        ConfigSource::File(path) => {
            log::info!("Config file: {}, log level: {}", path.display(), log_level);
        }
        ConfigSource::Stdin => {
            log::info!("Reading config from stdin, log level: {}", log_level);
        }
    }

    println!("rt-app-rs: not yet fully implemented");
}

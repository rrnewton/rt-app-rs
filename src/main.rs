use clap::Parser;

use rt_app_rs::args::{Cli, ConfigSource};

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

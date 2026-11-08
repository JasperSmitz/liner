use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = liner::cli::Cli::parse();
    let mut stdout = std::io::stdout().lock();
    match liner::cli::run(cli, &mut stdout).map_err(anyhow::Error::from) {
        Ok(outcome) => outcome.exit_code(),
        Err(err) => {
            eprintln!("liner: {err:#}");
            ExitCode::from(2)
        }
    }
}

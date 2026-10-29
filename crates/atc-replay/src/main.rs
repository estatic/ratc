use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: atc-replay <scenario.toml>");
        return ExitCode::from(2);
    };
    match atc_replay::run_scenario(&PathBuf::from(path)) {
        Ok(transcript) => {
            print!("{transcript}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("atc-replay: {e:#}");
            ExitCode::FAILURE
        }
    }
}

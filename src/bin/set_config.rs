//! Set one value in a TOML config file safely: idempotent, repairs duplicate sections.
//!
//!   sudo matrix-set-config /etc/matrix/config.toml display pwm_bits 10
//!
//! - Creates the [section] if missing, otherwise changes (or adds) just that key.
//! - Merges accidentally duplicated [sections] (later values win), keeping other lines and comments.
//! - Validates the result with a TOML parser before touching the file; keeps a .bak copy.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, section, key, value] = args.as_slice() else {
        eprintln!("usage: matrix-set-config FILE SECTION KEY VALUE\n\n{}", include_str!("set_config.rs").lines()
            .skip(2).take(7).map(|l| l.trim_start_matches("//!").trim_start()).collect::<Vec<_>>().join("\n"));
        return ExitCode::from(2);
    };
    match matrix_display::setconfig::run(Path::new(path), section, key, value) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

//! Operator commands for source discovery and data acquisition.

mod arguments;
mod commands;

fn main() -> std::process::ExitCode {
    commands::execute()
}

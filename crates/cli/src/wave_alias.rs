//! `wave`: the command's name before the rename to `synkopa`, kept so existing scripts keep working.
fn main() -> std::process::ExitCode {
    wave_cli::run_cli(std::env::args().skip(1).collect())
}

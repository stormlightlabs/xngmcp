use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;

#[path = "src/cli.rs"]
mod cli;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let assets_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies CARGO_MANIFEST_DIR")).join("assets");
    let man_dir = assets_dir.join("man");
    let completions_dir = assets_dir.join("completions");

    reset_directory(&man_dir)?;
    reset_directory(&completions_dir)?;
    generate_man_pages(&man_dir)?;
    generate_completions(&completions_dir)?;

    println!("cargo:rustc-env=XNGMCP_GENERATED_DIR={}", assets_dir.display());
    Ok(())
}

fn reset_directory(directory: &Path) -> io::Result<()> {
    if directory.exists() {
        fs::remove_dir_all(directory)?;
    }
    fs::create_dir_all(directory)
}

fn generate_man_pages(output_dir: &Path) -> io::Result<()> {
    let command = cli::Cli::command();
    Man::new(command.clone()).generate_to(output_dir)?;

    for subcommand in command.get_subcommands() {
        let name: &'static str =
            Box::leak(format!("{}-{}", command.get_name(), subcommand.get_name()).into_boxed_str());
        Man::new(subcommand.clone().name(name)).generate_to(output_dir)?;
    }

    Ok(())
}

fn generate_completions(output_dir: &Path) -> io::Result<()> {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Elvish, Shell::PowerShell] {
        let mut command = cli::Cli::command();
        generate_to(shell, &mut command, "xngmcp", output_dir)?;
    }

    Ok(())
}

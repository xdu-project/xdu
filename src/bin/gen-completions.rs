//! Generate shell completion scripts for all xdu binaries.
//!
//! Usage: gen-completions <bash_dir> <zsh_dir>

use std::fs;
use std::path::PathBuf;

use clap::CommandFactory;
use clap_complete::aot::{Bash, Zsh, generate_to};

use xdu::cli::{XduArgs, XduCpArgs, XduFindArgs, XduMvArgs, XduRmArgs, XduViewArgs};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <bash_dir> <zsh_dir>", args[0]);
        std::process::exit(1);
    }

    let bash_dir = PathBuf::from(&args[1]);
    let zsh_dir = PathBuf::from(&args[2]);

    fs::create_dir_all(&bash_dir).expect("failed to create bash completions dir");
    fs::create_dir_all(&zsh_dir).expect("failed to create zsh completions dir");

    type CommandBuilder = fn() -> clap::Command;

    // Generate for each binary
    let binaries: Vec<(&str, CommandBuilder)> = vec![
        ("xdu", XduArgs::command),
        ("xdu-find", XduFindArgs::command),
        ("xdu-view", XduViewArgs::command),
        ("xdu-rm", XduRmArgs::command),
        ("xdu-cp", XduCpArgs::command),
        ("xdu-mv", XduMvArgs::command),
    ];

    for (name, command_fn) in &binaries {
        let mut cmd = command_fn();

        generate_to(Bash, &mut cmd, *name, &bash_dir)
            .unwrap_or_else(|e| panic!("failed to generate bash completions for {name}: {e}"));

        generate_to(Zsh, &mut cmd, *name, &zsh_dir)
            .unwrap_or_else(|e| panic!("failed to generate zsh completions for {name}: {e}"));
    }

    eprintln!(
        "Generated completions: bash={}, zsh={}",
        bash_dir.display(),
        zsh_dir.display()
    );
}

use quichash::cli::{self, Command};
use quichash::commands::{self, ScanCommandOptions};
use quichash::error::HashUtilityError;
use std::io::IsTerminal;
use std::path::Path;
use std::process;

fn write_output(content: &str, output_path: Option<&Path>) -> Result<(), HashUtilityError> {
    match output_path {
        Some(path) => std::fs::write(path, content)
            .map_err(|e| HashUtilityError::from_io_error(e, "writing output", Some(path.to_path_buf()))),
        None => {
            print!("{}", content);
            Ok(())
        }
    }
}

fn run() -> Result<(), HashUtilityError> {
    let cli = cli::parse_args()?;

    if cli.command.is_none()
        && cli.file.is_none()
        && cli.text.is_none()
        && std::io::stdin().is_terminal()
    {
        use clap::CommandFactory;
        let mut cmd = cli::Cli::command();
        cmd.print_help().unwrap();
        println!();
        process::exit(0);
    }

    match cli.command {
        Some(Command::Scan {
            directory,
            algorithm,
            database,
            hdd,
            fast,
            format,
            json,
            compress,
        }) => {
            let opts = ScanCommandOptions {
                directory_pattern: &directory,
                algorithm: &algorithm,
                output: &database,
                parallel: !hdd,
                fast,
                format_str: &format,
                json,
                compress,
            };
            let content = commands::handle_scan_command(&opts)?;
            if json {
                println!("{}", content);
            }
            Ok(())
        }
        Some(Command::Verify {
            database,
            directory,
            hdd,
            json,
        }) => {
            let content = commands::handle_verify_command(&database, &directory, !hdd, json)?;
            write_output(&content, None)?;
            Ok(())
        }
        Some(Command::Benchmark { size_mb, json }) => {
            let content = commands::handle_benchmark_command(size_mb, json)?;
            write_output(&content, None)?;
            Ok(())
        }
        Some(Command::List { json }) => {
            let content = commands::handle_list_command(json)?;
            write_output(&content, None)?;
            Ok(())
        }
        Some(Command::Compare {
            database1,
            database2,
            output,
            format,
        }) => {
            let content = commands::handle_compare_command(&database1, &database2, &format)?;
            write_output(&content, output.as_deref())?;
            Ok(())
        }
        Some(Command::Version) => {
            let content = commands::handle_version_command()?;
            write_output(&content, None)?;
            Ok(())
        }
        Some(Command::Dedup {
            directory,
            fast,
            output,
            json,
        }) => {
            let content = commands::handle_dedup_command(&directory, fast, json)?;
            write_output(&content, output.as_deref())?;
            Ok(())
        }
        Some(Command::Analyze {
            database,
            json,
            output,
        }) => {
            let content = commands::handle_analyze_command(&database, json)?;
            write_output(&content, output.as_deref())?;
            Ok(())
        }
        None => {
            let content = commands::handle_hash_command(
                cli.file.as_deref(),
                cli.text.as_deref(),
                &cli.algorithms,
                cli.fast,
                cli.json,
            )?;
            write_output(&content, cli.output.as_deref())?;
            Ok(())
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

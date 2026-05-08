use crate::analyze::AnalyzeEngine;
use crate::benchmark::BenchmarkEngine;
use crate::compare::CompareEngine;
use crate::database::DatabaseFormat;
use crate::dedup::DedupEngine;
use crate::error::HashUtilityError;
use crate::hash::{HashComputer, HashRegistry};
use crate::scan::ScanEngine;
use crate::verify::VerifyEngine;
use std::path::{Path, PathBuf};

pub struct ScanCommandOptions<'a> {
    pub directory_pattern: &'a str,
    pub algorithm: &'a str,
    pub output: &'a Path,
    pub parallel: bool,
    pub fast: bool,
    pub format_str: &'a str,
    pub json: bool,
    pub compress: bool,
}

pub fn handle_hash_command(
    file_pattern: Option<&str>,
    text: Option<&str>,
    algorithms: &[String],
    fast: bool,
    json: bool,
) -> Result<String, HashUtilityError> {
    let computer = HashComputer::new();

    let results = match (file_pattern, text) {
        (Some(pattern), None) => {
            let files = crate::wildcard::expand_pattern(pattern)?;

            let show_progress = files.len() == 1;

            let mut all_results = Vec::new();
            for file_path in files {
                if fast {
                    for algorithm in algorithms {
                        all_results.push(computer.compute_hash_fast(&file_path, algorithm)?);
                    }
                } else {
                    let file_results = computer.compute_multiple_hashes_with_progress(
                        &file_path,
                        algorithms,
                        show_progress,
                    )?;
                    all_results.extend(file_results);
                }
            }
            all_results
        }
        (None, Some(text_input)) => {
            if fast {
                return Err(HashUtilityError::InvalidArguments {
                    message: "Fast mode is not supported when hashing text".to_string(),
                });
            }
            computer.compute_multiple_hashes_text(text_input, algorithms)?
        }
        (None, None) => {
            if fast {
                return Err(HashUtilityError::InvalidArguments {
                    message: "Fast mode is not supported when reading from stdin".to_string(),
                });
            }
            computer.compute_multiple_hashes_stdin(algorithms)?
        }
        (Some(_), Some(_)) => {
            return Err(HashUtilityError::InvalidArguments {
                message: "Cannot specify both file and text arguments".to_string(),
            });
        }
    };

    let output_content = if json {
        #[derive(serde::Serialize)]
        struct HashOutput {
            files: Vec<crate::hash::HashResult>,
            metadata: HashMetadata,
        }

        #[derive(serde::Serialize)]
        struct HashMetadata {
            timestamp: String,
            algorithms: Vec<String>,
            file_count: usize,
            fast_mode: bool,
        }

        let output = HashOutput {
            files: results.clone(),
            metadata: HashMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                algorithms: algorithms.to_vec(),
                file_count: {
                    use std::collections::HashSet;
                    results
                        .iter()
                        .map(|result| result.file_path.clone())
                        .collect::<HashSet<_>>()
                        .len()
                },
                fast_mode: fast,
            },
        };

        serde_json::to_string_pretty(&output).map_err(|e| HashUtilityError::InvalidArguments {
            message: format!("Failed to serialize JSON: {}", e),
        })?
    } else {
        let mut output_lines = Vec::new();

        if algorithms.len() > 1 {
            use std::collections::HashMap;
            let mut by_file: HashMap<PathBuf, Vec<&crate::hash::HashResult>> = HashMap::new();
            for result in &results {
                by_file
                    .entry(result.file_path.clone())
                    .or_default()
                    .push(result);
            }

            let num_files = by_file.len();
            for (file_path, file_results) in by_file {
                if num_files > 1 {
                    output_lines.push(format!("{}:", file_path.display()));
                }
                for result in file_results {
                    if num_files > 1 {
                        output_lines.push(format!(
                            "  {} ({})",
                            result.hash,
                            result.algorithm.to_uppercase()
                        ));
                    } else {
                        output_lines.push(format!(
                            "{} ({})  {}",
                            result.hash,
                            result.algorithm.to_uppercase(),
                            result.file_path.display()
                        ));
                    }
                }
                if num_files > 1 {
                    output_lines.push(String::new());
                }
            }
        } else {
            for result in results {
                output_lines.push(format!("{}  {}", result.hash, result.file_path.display()));
            }
        }

        output_lines.join("\n") + "\n"
    };

    Ok(output_content)
}

pub fn handle_scan_command(opts: &ScanCommandOptions) -> Result<String, HashUtilityError> {
    let format = match opts.format_str.to_lowercase().as_str() {
        "standard" => DatabaseFormat::Standard,
        "hashdeep" => DatabaseFormat::Hashdeep,
        _ => {
            return Err(HashUtilityError::InvalidArguments {
                message: format!(
                    "Invalid format '{}'. Valid formats are: standard, hashdeep",
                    opts.format_str
                ),
            });
        }
    };

    let directories = crate::wildcard::expand_pattern(opts.directory_pattern)?;

    for dir in &directories {
        if !dir.is_dir() {
            return Err(HashUtilityError::InvalidArguments {
                message: format!("Path '{}' is not a directory", dir.display()),
            });
        }
    }

    let engine = ScanEngine::with_parallel(opts.parallel)
        .with_fast_mode(opts.fast)
        .with_format(format)
        .with_quiet(opts.json);

    let mut total_stats = crate::scan::ScanStats {
        files_processed: 0,
        files_failed: 0,
        total_bytes: 0,
        duration: std::time::Duration::new(0, 0),
    };

    if directories.len() > 1 {
        std::fs::File::create(opts.output).map_err(|e| {
            HashUtilityError::from_io_error(e, "creating output file", Some(opts.output.to_path_buf()))
        })?;

        for (idx, directory) in directories.iter().enumerate() {
            let temp_output = if idx == 0 {
                opts.output.to_path_buf()
            } else {
                let temp_path = opts.output.with_extension(format!("tmp{}", idx));
                temp_path
            };

            let stats = engine.scan_directory(directory, opts.algorithm, &temp_output)?;

            if idx > 0 {
                let temp_contents = std::fs::read_to_string(&temp_output).map_err(|e| {
                    HashUtilityError::from_io_error(
                        e,
                        "reading temp file",
                        Some(temp_output.clone()),
                    )
                })?;

                use std::io::Write;
                let mut output_file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(opts.output)
                    .map_err(|e| {
                        HashUtilityError::from_io_error(
                            e,
                            "opening output file for append",
                            Some(opts.output.to_path_buf()),
                        )
                    })?;

                output_file
                    .write_all(temp_contents.as_bytes())
                    .map_err(|e| {
                        HashUtilityError::from_io_error(
                            e,
                            "appending to output file",
                            Some(opts.output.to_path_buf()),
                        )
                    })?;

                std::fs::remove_file(&temp_output).ok();
            }

            total_stats.files_processed += stats.files_processed;
            total_stats.files_failed += stats.files_failed;
            total_stats.total_bytes += stats.total_bytes;
            total_stats.duration += stats.duration;
        }
    } else {
        let stats = engine.scan_directory(&directories[0], opts.algorithm, opts.output)?;
        total_stats = stats;
    }

    let stats = total_stats;

    let final_output = if opts.compress {
        use crate::database::DatabaseHandler;

        let compressed_path = DatabaseHandler::compress_database(opts.output)?;

        std::fs::remove_file(opts.output).map_err(|e| {
            HashUtilityError::from_io_error(
                e,
                "removing uncompressed database",
                Some(opts.output.to_path_buf()),
            )
        })?;

        compressed_path
    } else {
        opts.output.to_path_buf()
    };

    if opts.json {
        #[derive(serde::Serialize)]
        struct ScanOutput {
            stats: crate::scan::ScanStats,
            metadata: ScanMetadata,
        }

        #[derive(serde::Serialize)]
        struct ScanMetadata {
            timestamp: String,
            directory_pattern: String,
            directories_scanned: Vec<PathBuf>,
            algorithm: String,
            output_file: PathBuf,
            parallel: bool,
            fast_mode: bool,
            format: String,
        }

        let output = ScanOutput {
            stats,
            metadata: ScanMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                directory_pattern: opts.directory_pattern.to_string(),
                directories_scanned: directories,
                algorithm: opts.algorithm.to_string(),
                output_file: final_output,
                parallel: opts.parallel,
                fast_mode: opts.fast,
                format: opts.format_str.to_string(),
            },
        };

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            }
        })?;

        Ok(json_output)
    } else {
        Ok("Scan complete.\n".to_string())
    }
}

pub fn handle_verify_command(
    database_pattern: &str,
    directory_pattern: &str,
    parallel: bool,
    json: bool,
) -> Result<String, HashUtilityError> {
    let engine = VerifyEngine::with_parallel(parallel).with_quiet(json);

    let databases = crate::wildcard::expand_pattern(database_pattern)?;
    let directories = crate::wildcard::expand_pattern(directory_pattern)?;

    for db in &databases {
        if !db.is_file() {
            return Err(HashUtilityError::InvalidArguments {
                message: format!("Database path '{}' is not a file", db.display()),
            });
        }
    }

    for dir in &directories {
        if !dir.is_dir() {
            return Err(HashUtilityError::InvalidArguments {
                message: format!("Path '{}' is not a directory", dir.display()),
            });
        }
    }

    let mut all_reports = Vec::new();

    for database in &databases {
        for directory in &directories {
            let report = engine.verify(database, directory)?;
            all_reports.push((database.clone(), directory.clone(), report));
        }
    }

    let (_database, _directory, report) = if all_reports.len() == 1 {
        let (db, dir, rep) = all_reports.into_iter().next().unwrap();
        (db, dir, rep)
    } else {
        let mut aggregated_report = crate::verify::VerifyReport {
            matches: 0,
            mismatches: Vec::new(),
            missing_files: Vec::new(),
            new_files: Vec::new(),
        };

        for (_, _, report) in &all_reports {
            aggregated_report.matches += report.matches;
            aggregated_report
                .mismatches
                .extend(report.mismatches.clone());
            aggregated_report
                .missing_files
                .extend(report.missing_files.clone());
            aggregated_report.new_files.extend(report.new_files.clone());
        }

        let (first_db, first_dir, _) = all_reports.into_iter().next().unwrap();
        (first_db, first_dir, aggregated_report)
    };

    if json {
        #[derive(serde::Serialize)]
        struct VerifyOutput {
            report: crate::verify::VerifyReport,
            metadata: VerifyMetadata,
        }

        #[derive(serde::Serialize)]
        struct VerifyMetadata {
            timestamp: String,
            database_pattern: String,
            directory_pattern: String,
            databases_verified: Vec<PathBuf>,
            directories_verified: Vec<PathBuf>,
        }

        let output = VerifyOutput {
            report,
            metadata: VerifyMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                database_pattern: database_pattern.to_string(),
                directory_pattern: directory_pattern.to_string(),
                databases_verified: databases,
                directories_verified: directories,
            },
        };

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            }
        })?;

        Ok(json_output)
    } else {
        Ok(report.format_display())
    }
}

pub fn handle_benchmark_command(size_mb: usize, json: bool) -> Result<String, HashUtilityError> {
    let engine = BenchmarkEngine::new();

    let results = engine.run_benchmarks(size_mb)?;

    if json {
        #[derive(serde::Serialize)]
        struct BenchmarkOutput {
            results: Vec<crate::benchmark::BenchmarkResult>,
            metadata: BenchmarkMetadata,
        }

        #[derive(serde::Serialize)]
        struct BenchmarkMetadata {
            timestamp: String,
            data_size_mb: usize,
            algorithm_count: usize,
        }

        let output = BenchmarkOutput {
            results: results.clone(),
            metadata: BenchmarkMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                data_size_mb: size_mb,
                algorithm_count: results.len(),
            },
        };

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            }
        })?;

        Ok(json_output)
    } else {
        let header = format!("Running benchmarks with {} MB of test data...\n", size_mb);
        Ok(header + &engine.format_results(&results))
    }
}

pub fn handle_list_command(json: bool) -> Result<String, HashUtilityError> {
    let algorithms = HashRegistry::list_algorithms();

    if json {
        #[derive(serde::Serialize)]
        struct ListOutput {
            algorithms: Vec<crate::hash::AlgorithmInfo>,
            metadata: ListMetadata,
        }

        #[derive(serde::Serialize)]
        struct ListMetadata {
            timestamp: String,
            algorithm_count: usize,
        }

        let output = ListOutput {
            algorithms: algorithms.clone(),
            metadata: ListMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                algorithm_count: algorithms.len(),
            },
        };

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            }
        })?;

        Ok(json_output)
    } else {
        use std::fmt::Write;
        let mut output = String::new();
        writeln!(&mut output, "\nAvailable Hash Algorithms:\n").unwrap();
        writeln!(
            &mut output,
            "{:<20} {:>12} {:>15} {:>15}",
            "Algorithm", "Output Bits", "Post-Quantum", "Cryptographic"
        )
        .unwrap();
        writeln!(&mut output, "{}", "-".repeat(65)).unwrap();

        for algo in algorithms {
            let pq_status = if algo.post_quantum { "Yes" } else { "No" };
            let crypto_status = if algo.cryptographic { "Yes" } else { "No" };
            writeln!(
                &mut output,
                "{:<20} {:>12} {:>15} {:>15}",
                algo.name, algo.output_bits, pq_status, crypto_status
            )
            .unwrap();
        }

        writeln!(&mut output).unwrap();
        Ok(output)
    }
}

pub fn handle_compare_command(
    database1: &Path,
    database2: &Path,
    format: &str,
) -> Result<String, HashUtilityError> {
    let engine = CompareEngine::new();
    let report = engine.compare(database1, database2)?;

    let output_content = match format.to_lowercase().as_str() {
        "plain-text" | "plain" | "text" => report.to_plain_text(),
        "json" => report
            .to_json()
            .map_err(|e| HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            })?,
        "hashdeep" => report.to_hashdeep(),
        _ => {
            return Err(HashUtilityError::InvalidArguments {
                message: format!(
                    "Invalid format '{}'. Valid formats are: plain-text, json, hashdeep",
                    format
                ),
            });
        }
    };

    Ok(output_content)
}

pub fn handle_version_command() -> Result<String, HashUtilityError> {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    Ok(format!("hash v{}\n", VERSION))
}

pub fn handle_dedup_command(
    directory: &Path,
    fast: bool,
    json: bool,
) -> Result<String, HashUtilityError> {
    let engine = DedupEngine::new()
        .with_fast_mode(fast)
        .with_parallel(true)
        .with_quiet(json);

    let report = engine.find_duplicates(directory)?;

    let output_content = if json {
        report
            .to_json()
            .map_err(|e| HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            })?
    } else {
        use std::fmt::Write;
        let mut output_str = String::new();

        writeln!(&mut output_str, "\n=== Duplicate Files Report ===\n").unwrap();
        writeln!(&mut output_str, "Summary:").unwrap();
        writeln!(
            &mut output_str,
            "  Files scanned:     {}",
            report.stats.files_scanned
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Files failed:      {}",
            report.stats.files_failed
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Total bytes:       {} ({:.2} MB)",
            report.stats.total_bytes,
            report.stats.total_bytes as f64 / 1_048_576.0
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Duplicate groups:  {}",
            report.stats.duplicate_groups
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Duplicate files:   {}",
            report.stats.duplicate_files
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Wasted space:      {} ({:.2} MB)",
            report.stats.wasted_space,
            report.stats.wasted_space as f64 / 1_048_576.0
        )
        .unwrap();
        writeln!(
            &mut output_str,
            "  Duration:          {:.2}s",
            report.stats.duration.as_secs_f64()
        )
        .unwrap();

        if report.stats.duration.as_secs_f64() > 0.0 {
            let throughput_mbps = (report.stats.total_bytes as f64 / 1_048_576.0)
                / report.stats.duration.as_secs_f64();
            writeln!(
                &mut output_str,
                "  Throughput:        {:.2} MB/s",
                throughput_mbps
            )
            .unwrap();
        }

        if !report.duplicate_groups.is_empty() {
            writeln!(
                &mut output_str,
                "\nDuplicate Groups (sorted by wasted space):"
            )
            .unwrap();
            for group in &report.duplicate_groups {
                writeln!(
                    &mut output_str,
                    "\n  Hash: {} ({} files, {} bytes each, {} bytes wasted)",
                    group.hash, group.count, group.file_size, group.wasted_space
                )
                .unwrap();
                for path in &group.paths {
                    writeln!(&mut output_str, "    {}", path.display()).unwrap();
                }
            }
        } else {
            writeln!(&mut output_str, "\nNo duplicate files found.").unwrap();
        }

        writeln!(&mut output_str).unwrap();
        output_str
    };

    Ok(output_content)
}

pub fn handle_analyze_command(
    database: &Path,
    json: bool,
) -> Result<String, HashUtilityError> {
    let engine = AnalyzeEngine::new();
    let report = engine.analyze(database)?;

    let output_content = if json {
        report
            .to_json()
            .map_err(|e| HashUtilityError::InvalidArguments {
                message: format!("Failed to serialize JSON: {}", e),
            })?
    } else {
        report.to_plain_text()
    };

    Ok(output_content)
}

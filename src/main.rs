
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use colored::*;
use humansize::{format_size, DECIMAL};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

#[derive(Debug, Clone, ValueEnum)]
enum HashAlgorithm {
    Sha256,
    Blake3,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(default_value = ".")]
    directory: PathBuf,

    #[arg(short = 'a', long, default_value = "blake3")]
    algorithm: HashAlgorithm,

    #[arg(long)]
    delete: bool,

    #[arg(long)]
    dry_run: bool,

    #[arg(long, default_value = "0")]
    min_size: u64,

    #[arg(long)]
    max_size: Option<u64>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    threads: Option<usize>,
}

#[derive(Debug, Clone)]
struct FileInfo {
    path: PathBuf,
    size: u64,
    hash: String,
}

struct DuplicateFinder {
    args: Args,
}

impl DuplicateFinder {
    fn new(args: Args) -> Self {
        Self { args }
    }

    fn run(&self) -> Result<()> {
        // Set thread pool size if specified
        if let Some(threads) = self.args.threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build_global()
                .context("Failed to set thread pool size")?;
        }

        println!("{}", "🔍 Scanning for files...".cyan().bold());
        let files = self.collect_files()?;
        
        if files.is_empty() {
            println!("{}", "No files found matching criteria.".yellow());
            return Ok(());
        }

        println!("{}", format!("📊 Found {} files to process", files.len()).green());
        
        println!("{}", "🔐 Computing hashes...".cyan().bold());
        let file_infos = self.compute_hashes(files)?;
        
        println!("{}", "🔍 Finding duplicates...".cyan().bold());
        let duplicates = self.find_duplicates(file_infos);
        
        self.report_duplicates(&duplicates)?;
        
        if self.args.delete || self.args.dry_run {
            self.handle_deletion(&duplicates)?;
        }

        Ok(())
    }

    fn collect_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let walker = WalkDir::new(&self.args.directory)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file());

        for entry in walker {
            let path = entry.path();
            let metadata = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let size = metadata.len();

            if size < self.args.min_size {
                continue;
            }
            
            if let Some(max_size) = self.args.max_size {
                if size > max_size {
                    continue;
                }
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    fn compute_hashes(&self, files: Vec<PathBuf>) -> Result<Vec<FileInfo>> {
        let progress = ProgressBar::new(files.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        let file_infos = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));

        files.par_iter().for_each(|path| {
            match self.compute_file_hash(path) {
                Ok(file_info) => {
                    file_infos.lock().unwrap().push(file_info);
                }
                Err(e) => {
                    errors.lock().unwrap().push((path.clone(), e));
                }
            }
            progress.inc(1);
        });

        progress.finish_with_message("Hash computation complete!");

        let errors = errors.lock().unwrap();
        if !errors.is_empty() && self.args.verbose {
            println!("{}", "⚠️  Errors encountered:".yellow().bold());
            for (path, error) in errors.iter() {
                println!("  {}: {}", path.display(), error.to_string().red());
            }
        }

        let file_infos = Arc::try_unwrap(file_infos).unwrap().into_inner().unwrap();
        Ok(file_infos)
    }

    fn compute_file_hash(&self, path: &Path) -> Result<FileInfo> {
        let content = fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        
        let size = content.len() as u64;
        
        let hash = match self.args.algorithm {
            HashAlgorithm::Sha256 => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&content);
                format!("{:x}", hasher.finalize())
            }
            HashAlgorithm::Blake3 => {
                blake3::hash(&content).to_hex().to_string()
            }
        };

        Ok(FileInfo {
            path: path.to_path_buf(),
            size,
            hash,
        })
    }

    fn find_duplicates(&self, file_infos: Vec<FileInfo>) -> HashMap<String, Vec<FileInfo>> {
        let mut hash_groups: HashMap<String, Vec<FileInfo>> = HashMap::new();
        
        for file_info in file_infos {
            hash_groups.entry(file_info.hash.clone())
                .or_insert_with(Vec::new)
                .push(file_info);
        }

        hash_groups.retain(|_, files| files.len() > 1);
        hash_groups
    }

    fn report_duplicates(&self, duplicates: &HashMap<String, Vec<FileInfo>>) -> Result<()> {
        if duplicates.is_empty() {
            println!("{}", "✅ No duplicates found!".green().bold());
            return Ok(());
        }

        let total_groups = duplicates.len();
        let total_files: usize = duplicates.values().map(|v| v.len()).sum();
        let total_duplicates = total_files - total_groups; // Subtract originals
        let wasted_space: u64 = duplicates.values()
            .map(|files| files[0].size * (files.len() - 1) as u64)
            .sum();

        println!("{}", "📋 DUPLICATE REPORT".yellow().bold());
        println!("═══════════════════════════════════════");
        println!("Duplicate groups: {}", total_groups.to_string().cyan().bold());
        println!("Total duplicate files: {}", total_duplicates.to_string().red().bold());
        println!("Wasted space: {}", format_size(wasted_space, DECIMAL).red().bold());
        println!();

        for (i, (hash, files)) in duplicates.iter().enumerate() {
            let group_size = files[0].size;
            let wasted = group_size * (files.len() - 1) as u64;
            
            println!("{} {} ({})", 
                format!("Group {}:", i + 1).yellow().bold(),
                format_size(group_size, DECIMAL).cyan(),
                format!("wastes {}", format_size(wasted, DECIMAL)).red()
            );
            
            if self.args.verbose {
                println!("  Hash: {}", hash.bright_black());
            }
            
            let mut sorted_files = files.clone();
            sorted_files.sort_by(|a, b| a.path.cmp(&b.path));
            
            for (j, file) in sorted_files.iter().enumerate() {
                let prefix = if j == 0 { "  🏆" } else { "  📋" };
                let status = if j == 0 { " (original)".green() } else { " (duplicate)".red() };
                println!("{} {}{}", prefix, file.path.display(), status);
            }
            println!();
        }

        Ok(())
    }

    fn handle_deletion(&self, duplicates: &HashMap<String, Vec<FileInfo>>) -> Result<()> {
        if duplicates.is_empty() {
            return Ok(());
        }

        let total_to_delete: usize = duplicates.values()
            .map(|files| files.len() - 1)
            .sum();

        if self.args.dry_run {
            println!("{}", "DRY RUN - No files will be deleted".yellow().bold());
        } else {
            println!("{}", "DELETING DUPLICATES".red().bold());
        }

        println!("Files to be deleted: {}", total_to_delete.to_string().red().bold());
        println!();

        let mut deleted_count = 0;
        let mut delete_errors = Vec::new();

        for files in duplicates.values() {
            // Skip the first file (keep as original), delete the rest
            for file in files.iter().skip(1) {
                if self.args.dry_run {
                    println!("  Would delete: {}", file.path.display());
                } else {
                    match fs::remove_file(&file.path) {
                        Ok(()) => {
                            deleted_count += 1;
                            if self.args.verbose {
                                println!("Deleted: {}", file.path.display());
                            }
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            delete_errors.push((file.path.clone(), e));
                            println!("Failed to delete: {} ({})", 
                                file.path.display(), error_msg.red());
                        }
                    }
                }
            }
        }

        if !self.args.dry_run {
            println!();
            println!("{}", format!("Successfully deleted {} files", deleted_count).green().bold());
            
            if !delete_errors.is_empty() {
                println!("{}", format!("Failed to delete {} files", delete_errors.len()).red().bold());
            }
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    println!("{}", "dedup-rs - Duplicate File Finder".blue().bold());
    println!("════════════════════════════════");
    
    if args.verbose {
        println!("Directory: {}", args.directory.display());
        println!("Algorithm: {:?}", args.algorithm);
        println!("Min size: {}", format_size(args.min_size, DECIMAL));
        if let Some(max_size) = args.max_size {
            println!("Max size: {}", format_size(max_size, DECIMAL));
        }
        println!();
    }

    let finder = DuplicateFinder::new(args);
    finder.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_duplicates_empty() {
        let finder = DuplicateFinder::new(Args {
            directory: PathBuf::from("."),
            algorithm: HashAlgorithm::Blake3,
            delete: false,
            dry_run: false,
            min_size: 0,
            max_size: None,
            verbose: false,
            threads: None,
        });

        let duplicates = finder.find_duplicates(vec![]);
        assert!(duplicates.is_empty());
    }

    #[test]
    fn test_hash_computation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "test content")?;

        let finder = DuplicateFinder::new(Args {
            directory: PathBuf::from("."),
            algorithm: HashAlgorithm::Blake3,
            delete: false,
            dry_run: false,
            min_size: 0,
            max_size: None,
            verbose: false,
            threads: None,
        });

        let file_info = finder.compute_file_hash(&file_path)?;
        assert_eq!(file_info.size, 12);
        assert!(!file_info.hash.is_empty());
        
        Ok(())
    }
}
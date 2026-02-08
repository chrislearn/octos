//! Clean command: remove stale task state files.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use eyre::Result;

use super::Executable;

/// Clean up stale task state and cache files.
#[derive(Debug, Args)]
pub struct CleanCommand {
    /// Working directory (defaults to current directory).
    #[arg(short, long)]
    pub cwd: Option<PathBuf>,

    /// Remove all task history (not just completed tasks).
    #[arg(long)]
    pub all: bool,

    /// Dry run - show what would be deleted without actually deleting.
    #[arg(long)]
    pub dry_run: bool,
}

impl Executable for CleanCommand {
    fn execute(self) -> Result<()> {
        println!("{}", "crew-rs clean".cyan().bold());
        println!();

        let cwd = self.cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
        let data_dir = cwd.join(".crew");

        if !data_dir.exists() {
            println!("{}", "No .crew directory found.".yellow());
            return Ok(());
        }

        let mut files_to_remove = Vec::new();
        let mut total_size: u64 = 0;

        // Find task state files
        let tasks_dir = data_dir.join("tasks");
        if tasks_dir.exists() {
            for entry in std::fs::read_dir(&tasks_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() {
                    // If --all, remove everything; otherwise only remove .json files
                    if self.all || path.extension().is_some_and(|e| e == "json") {
                        if let Ok(meta) = entry.metadata() {
                            total_size += meta.len();
                        }
                        files_to_remove.push(path);
                    }
                }
            }
        }

        // Find database files if --all
        if self.all {
            for entry in std::fs::read_dir(&data_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() {
                    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
                    // Remove .redb database files
                    if ext.as_deref() == Some("redb") {
                        if let Ok(meta) = entry.metadata() {
                            total_size += meta.len();
                        }
                        files_to_remove.push(path);
                    }
                }
            }
        }

        if files_to_remove.is_empty() {
            println!("{}", "Nothing to clean.".green());
            return Ok(());
        }

        // Format size
        let size_str = if total_size > 1024 * 1024 {
            format!("{:.1} MB", total_size as f64 / (1024.0 * 1024.0))
        } else if total_size > 1024 {
            format!("{:.1} KB", total_size as f64 / 1024.0)
        } else {
            format!("{} bytes", total_size)
        };

        println!(
            "{} {} files ({}):",
            if self.dry_run {
                "Would remove"
            } else {
                "Removing"
            },
            files_to_remove.len(),
            size_str
        );
        println!();

        for path in &files_to_remove {
            let relative = path.strip_prefix(&cwd).unwrap_or(path);
            println!("  {}", relative.display());
        }
        println!();

        if self.dry_run {
            println!("{}", "Dry run - no files were deleted.".yellow());
            println!("Run without --dry-run to actually delete files.");
        } else {
            // Actually delete files
            for path in &files_to_remove {
                std::fs::remove_file(path)?;
            }

            // Remove empty tasks directory
            if tasks_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&tasks_dir) {
                    if entries.count() == 0 {
                        std::fs::remove_dir(&tasks_dir)?;
                    }
                }
            }

            println!(
                "{} {} files, freed {}",
                "Cleaned".green(),
                files_to_remove.len(),
                size_str
            );
        }

        Ok(())
    }
}

//! Edit file tool for making precise text replacements.

use std::path::PathBuf;

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use serde::Deserialize;

use super::{Tool, ToolResult};

/// Tool for editing files via string replacement.
pub struct EditFileTool {
    /// Base directory for resolving relative paths.
    base_dir: PathBuf,
}

impl EditFileTool {
    /// Create a new edit file tool.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EditFileInput {
    path: String,
    old_string: String,
    new_string: String,
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string with a new string. The old_string must match exactly (including whitespace and indentation)."
    }

    fn tags(&self) -> &[&str] {
        &["fs", "code"]
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The string to replace it with"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: EditFileInput =
            serde_json::from_value(args.clone()).wrap_err("invalid edit_file tool input")?;

        // Resolve path (with traversal protection)
        let path = match super::resolve_path(&self.base_dir, &input.path) {
            Ok(p) => p,
            Err(_) => {
                return Ok(ToolResult {
                    output: format!("Path outside working directory: {}", input.path),
                    success: false,
                    ..Default::default()
                });
            }
        };

        // Read current content (O_NOFOLLOW atomically rejects symlinks)
        let content = match super::read_no_follow(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(super::file_io_error(e, &input.path)),
        };

        // Check if old_string exists
        let count = content.matches(&input.old_string).count();

        if count == 0 {
            return Ok(ToolResult {
                output: format!(
                    "String not found in file. Make sure the old_string matches exactly.\n\nSearched for:\n```\n{}\n```",
                    input.old_string
                ),
                success: false,
                ..Default::default()
            });
        }

        if count > 1 {
            return Ok(ToolResult {
                output: format!(
                    "Found {} occurrences of the string. Please provide more context to make the match unique.",
                    count
                ),
                success: false,
                ..Default::default()
            });
        }

        // Perform replacement
        let new_content = content.replacen(&input.old_string, &input.new_string, 1);

        // Write back (O_NOFOLLOW)
        if let Err(e) = super::write_no_follow(&path, new_content.as_bytes()).await {
            return Ok(super::file_io_error(e, &input.path));
        }

        Ok(ToolResult {
            output: format!("Successfully edited {}", input.path),
            success: true,
            file_modified: Some(path),
            ..Default::default()
        })
    }
}

//! Cron tool for scheduling tasks via the agent.
//!
//! Lives in crew-cli (not crew-agent) to avoid a crew-agent -> crew-bus dependency.

use std::sync::Arc;

use async_trait::async_trait;
use crew_agent::tools::{Tool, ToolResult};
use crew_bus::{CronPayload, CronSchedule, CronService};
use eyre::{Result, WrapErr};
use serde::Deserialize;

pub struct CronTool {
    service: Arc<CronService>,
}

impl CronTool {
    pub fn new(service: Arc<CronService>) -> Self {
        Self { service }
    }
}

#[derive(Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    every_seconds: Option<i64>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        "cron"
    }

    fn description(&self) -> &str {
        "Schedule recurring or one-time tasks. Actions: add, list, remove."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "list", "remove"],
                    "description": "The action to perform"
                },
                "message": {
                    "type": "string",
                    "description": "Message to deliver when the job fires (required for 'add')"
                },
                "every_seconds": {
                    "type": "integer",
                    "description": "Interval in seconds for recurring jobs (required for 'add')"
                },
                "name": {
                    "type": "string",
                    "description": "Optional name for the job"
                },
                "channel": {
                    "type": "string",
                    "description": "Channel to deliver to (e.g. 'telegram')"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Chat ID to deliver to"
                },
                "job_id": {
                    "type": "string",
                    "description": "Job ID (required for 'remove')"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: Input =
            serde_json::from_value(args.clone()).wrap_err("invalid cron tool input")?;

        match input.action.as_str() {
            "add" => self.handle_add(input),
            "list" => Ok(self.handle_list()),
            "remove" => Ok(self.handle_remove(input)),
            other => Ok(ToolResult {
                output: format!("Unknown action: {other}. Use 'add', 'list', or 'remove'."),
                success: false,
                ..Default::default()
            }),
        }
    }
}

impl CronTool {
    fn handle_add(&self, input: Input) -> Result<ToolResult> {
        let message = match input.message {
            Some(m) => m,
            None => {
                return Ok(ToolResult {
                    output: "'message' is required for 'add' action.".into(),
                    success: false,
                    ..Default::default()
                });
            }
        };

        let every_seconds = match input.every_seconds {
            Some(s) if s > 0 => s,
            _ => {
                return Ok(ToolResult {
                    output: "'every_seconds' must be a positive integer for 'add' action.".into(),
                    success: false,
                    ..Default::default()
                });
            }
        };

        let schedule = CronSchedule::Every {
            every_ms: every_seconds * 1000,
        };
        let payload = CronPayload {
            message,
            deliver: input.channel.is_some(),
            channel: input.channel,
            chat_id: input.chat_id,
        };

        let name = input.name.unwrap_or_else(|| "unnamed".into());
        let job = self.service.add_job(name, schedule, payload)?;

        Ok(ToolResult {
            output: format!(
                "Created job '{}' (id: {}), runs every {}s.",
                job.name, job.id, every_seconds
            ),
            success: true,
            ..Default::default()
        })
    }

    fn handle_list(&self) -> ToolResult {
        let jobs = self.service.list_jobs();
        if jobs.is_empty() {
            return ToolResult {
                output: "No scheduled jobs.".into(),
                success: true,
                ..Default::default()
            };
        }

        let mut out = format!("{} scheduled job(s):\n\n", jobs.len());
        for (i, job) in jobs.iter().enumerate() {
            let schedule_desc = match &job.schedule {
                CronSchedule::At { at_ms } => format!("once at {at_ms}"),
                CronSchedule::Every { every_ms } => format!("every {}s", every_ms / 1000),
            };
            out.push_str(&format!(
                "{}. [{}] {} — {} (msg: \"{}\")\n",
                i + 1,
                job.id,
                job.name,
                schedule_desc,
                truncate(&job.payload.message, 60),
            ));
        }

        ToolResult {
            output: out,
            success: true,
            ..Default::default()
        }
    }

    fn handle_remove(&self, input: Input) -> ToolResult {
        let id = match input.job_id {
            Some(id) => id,
            None => {
                return ToolResult {
                    output: "'job_id' is required for 'remove' action.".into(),
                    success: false,
                    ..Default::default()
                };
            }
        };

        if self.service.remove_job(&id) {
            ToolResult {
                output: format!("Removed job {id}."),
                success: true,
                ..Default::default()
            }
        } else {
            ToolResult {
                output: format!("Job {id} not found."),
                success: false,
                ..Default::default()
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn make_service(
        dir: &std::path::Path,
    ) -> (Arc<CronService>, mpsc::Receiver<crew_core::InboundMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let service = Arc::new(CronService::new(dir.join("cron.json"), tx));
        (service, rx)
    }

    #[tokio::test]
    async fn test_list_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (service, _rx) = make_service(dir.path());
        let tool = CronTool::new(service);

        let result = tool
            .execute(&serde_json::json!({"action": "list"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No scheduled"));
    }

    #[tokio::test]
    async fn test_add_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let (service, _rx) = make_service(dir.path());
        let tool = CronTool::new(service);

        let result = tool
            .execute(&serde_json::json!({
                "action": "add",
                "message": "check status",
                "every_seconds": 300,
                "name": "status-check"
            }))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("status-check"));

        let list = tool
            .execute(&serde_json::json!({"action": "list"}))
            .await
            .unwrap();
        assert!(list.success);
        assert!(list.output.contains("status-check"));
        assert!(list.output.contains("every 300s"));
    }

    #[tokio::test]
    async fn test_add_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let (service, _rx) = make_service(dir.path());
        let tool = CronTool::new(service);

        let add_result = tool
            .execute(&serde_json::json!({
                "action": "add",
                "message": "temp",
                "every_seconds": 60
            }))
            .await
            .unwrap();
        assert!(add_result.success);

        // Extract job ID from output
        let id = add_result
            .output
            .split("id: ")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();

        let remove = tool
            .execute(&serde_json::json!({"action": "remove", "job_id": id}))
            .await
            .unwrap();
        assert!(remove.success);
        assert!(remove.output.contains("Removed"));
    }
}

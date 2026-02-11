//! Message bus, channels, and session management for crew-rs gateway.

pub mod bus;
pub mod channel;
pub mod cli_channel;
pub mod cron_service;
pub mod cron_types;
pub mod session;

#[cfg(feature = "telegram")]
pub mod telegram_channel;
#[cfg(feature = "discord")]
pub mod discord_channel;

pub use bus::{AgentHandle, BusPublisher, create_bus};
pub use channel::{Channel, ChannelManager};
pub use cli_channel::CliChannel;
pub use cron_service::CronService;
pub use cron_types::{CronJob, CronPayload, CronSchedule, CronStore};
pub use session::{Session, SessionManager};

#[cfg(feature = "telegram")]
pub use telegram_channel::TelegramChannel;
#[cfg(feature = "discord")]
pub use discord_channel::DiscordChannel;

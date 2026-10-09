pub const SURF_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub const DEFAULT_IMAGE: &str = "ghcr.io/aureuma/surf-browser:local";
pub const DEFAULT_CONTAINER: &str = "surf-playwright-mcp-headed";
pub const DEFAULT_NETWORK: &str = "surf-shared";
pub const DEFAULT_HOST_MCP_BIND: &str = "127.0.0.1";
pub const DEFAULT_HOST_NOVNC_BIND: &str = "0.0.0.0";
pub const DEFAULT_MCP_PORT: i32 = 8931;
pub const DEFAULT_HOST_MCP_PORT: i32 = 8932;
pub const DEFAULT_NOVNC_PORT: i32 = 6080;
pub const DEFAULT_HOST_NOVNC_PORT: i32 = 6080;
pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const DEFAULT_TUNNEL_NAME: &str = "surf-cloudflared";
pub const DEFAULT_CLOUDFLARED_IMAGE: &str = "cloudflare/cloudflared:latest";
pub const PROFILE_VOLUME_PREFIX: &str = "volume:";

pub const SURF_SETTINGS_SCHEMA_VERSION: i32 = 1;
pub const DEFAULT_SURF_CONFIG_ROOT: &str = "~/.si/surf";
pub const DEFAULT_SURF_CONFIG_FILE: &str = "~/.si/surf/settings.toml";
pub const DEFAULT_SURF_STATE_DIR_PATH: &str = "~/.surf";

pub const DEFAULT_AGENT_NAME: &str = "default";
pub const MAX_AGENTS: usize = 8;

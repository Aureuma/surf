pub const SURF_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub const SURF_WRAPPER_ENV_NAME: &str = "SI_SURF_WRAPPED";
pub const SURF_STANDALONE_BYPASS: &str = "SURF_STANDALONE_UNSAFE";

pub const DEFAULT_IMAGE: &str = "ghcr.io/aureuma/surf-browser:local";
pub const DEFAULT_CONTAINER: &str = "surf-playwright-mcp-headed";
pub const DEFAULT_NETWORK: &str = "surf-shared";
pub const DEFAULT_HOST_BIND: &str = "127.0.0.1";
pub const DEFAULT_MCP_PORT: i32 = 8931;
pub const DEFAULT_HOST_MCP_PORT: i32 = 8932;
pub const DEFAULT_NOVNC_PORT: i32 = 6080;
pub const DEFAULT_HOST_NOVNC_PORT: i32 = 6080;
pub const DEFAULT_MCP_VERSION: &str = "0.0.64";
pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const DEFAULT_HOST_CDP_PORT: i32 = 18800;
pub const DEFAULT_TUNNEL_NAME: &str = "surf-cloudflared";
pub const DEFAULT_CLOUDFLARED_IMAGE: &str = "cloudflare/cloudflared:latest";
pub const PROFILE_VOLUME_PREFIX: &str = "volume:";

pub const SURF_SETTINGS_SCHEMA_VERSION: i32 = 1;
pub const DEFAULT_SURF_CONFIG_ROOT: &str = "~/.si/surf";
pub const DEFAULT_SURF_CONFIG_FILE: &str = "~/.si/surf/settings.toml";
pub const DEFAULT_SURF_STATE_DIR_PATH: &str = "~/.surf";

pub const SESSION_MODE_READ_ONLY: &str = "read_only";
pub const SESSION_MODE_INTERACTIVE: &str = "interactive";

pub const USAGE_TEXT: &str = r#"surf <command> [args]

Commands:
  build        Build browser runtime image
  start        Start browser runtime container
  stop         Stop/remove runtime container
  status       Check runtime health
  logs         Stream runtime logs
  session      Attach and act on existing browser sessions
  config       Manage surf settings file
  proxy        Start MCP path-compat proxy
  host         Manage headed host browser (macOS/Linux)
  tunnel       Manage noVNC cloud tunnel
  extension    Manage Chrome extension scaffold
  version      Print version

Examples:
  surf config show --json
  surf config set --key tunnel.mode --value token
  surf build
  surf start --profile default
  surf status --json
  surf session discover
  surf session attach --id <target-id>
  surf session act --session <name> --action title
  surf host start --profile work
  surf tunnel start --mode quick
  surf tunnel start --mode token --fort-key SURF_CLOUDFLARE_TUNNEL_TOKEN --fort-repo surf --fort-env dev
  surf extension install
"#;

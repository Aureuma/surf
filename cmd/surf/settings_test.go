package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadSurfSettingsCreatesDefaultFile(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")

	settings, err := loadSurfSettings()
	if err != nil {
		t.Fatalf("loadSurfSettings: %v", err)
	}
	if settings.SchemaVersion != surfSettingsSchemaVersion {
		t.Fatalf("schema_version=%d", settings.SchemaVersion)
	}
	path := filepath.Join(home, ".si", "surf", "settings.toml")
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected settings file at %s: %v", path, err)
	}
}

func TestSetSurfConfigValue(t *testing.T) {
	settings := defaultSurfSettings()
	if err := setSurfConfigValue(&settings, "tunnel.mode", "token"); err != nil {
		t.Fatalf("set tunnel.mode: %v", err)
	}
	if settings.Tunnel.Mode != "token" {
		t.Fatalf("tunnel.mode=%q", settings.Tunnel.Mode)
	}
	if err := setSurfConfigValue(&settings, "browser.host_mcp_port", "9999"); err != nil {
		t.Fatalf("set browser.host_mcp_port: %v", err)
	}
	if settings.Browser.HostMCPPort != 9999 {
		t.Fatalf("host_mcp_port=%d", settings.Browser.HostMCPPort)
	}
	if err := setSurfConfigValue(&settings, "tunnel.mode", "bad"); err == nil {
		t.Fatalf("expected invalid mode error")
	}
	if err := setSurfConfigValue(&settings, "existing_session.mode", "interactive"); err != nil {
		t.Fatalf("set existing_session.mode: %v", err)
	}
	if settings.ExistingSession.Mode != sessionModeInteractive {
		t.Fatalf("existing_session.mode=%q", settings.ExistingSession.Mode)
	}
	if err := setSurfConfigValue(&settings, "existing_session.default_browser", "safari"); err != nil {
		t.Fatalf("set existing_session.default_browser: %v", err)
	}
	if settings.ExistingSession.DefaultBrowser != "safari" {
		t.Fatalf("existing_session.default_browser=%q", settings.ExistingSession.DefaultBrowser)
	}
	if err := setSurfConfigValue(&settings, "existing_session.chrome_cdp_port", "17777"); err != nil {
		t.Fatalf("set existing_session.chrome_cdp_port: %v", err)
	}
	if settings.ExistingSession.ChromeCDPPort != 17777 {
		t.Fatalf("existing_session.chrome_cdp_port=%d", settings.ExistingSession.ChromeCDPPort)
	}
}

func TestSetSurfConfigValueTunnelTargetURL(t *testing.T) {
	settings := defaultSurfSettings()
	target := "http://127.0.0.1:6081/vnc.html?autoconnect=1&resize=scale"
	if err := setSurfConfigValue(&settings, "tunnel.target_url", target); err != nil {
		t.Fatalf("set tunnel.target_url: %v", err)
	}
	if settings.Tunnel.TargetURL != target {
		t.Fatalf("tunnel.target_url=%q", settings.Tunnel.TargetURL)
	}
}

func TestDefaultConfigUsesSurfSettings(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")
	t.Setenv("SURF_IMAGE", "")
	t.Setenv("SURF_CONTAINER", "")
	t.Setenv("SURF_NETWORK", "")
	t.Setenv("SURF_PROFILE", "")
	t.Setenv("SURF_PROFILE_DIR", "")
	t.Setenv("SURF_HOST_BIND", "")
	t.Setenv("SURF_HOST_MCP_PORT", "")
	t.Setenv("SURF_HOST_NOVNC_PORT", "")
	t.Setenv("SURF_MCP_PORT", "")
	t.Setenv("SURF_NOVNC_PORT", "")
	t.Setenv("SURF_VNC_PASSWORD", "")
	t.Setenv("SURF_MCP_VERSION", "")
	t.Setenv("SURF_BROWSER_CHANNEL", "")
	t.Setenv("SURF_ALLOWED_HOSTS", "")

	settings := defaultSurfSettings()
	settings.Browser.ImageName = "test/surf:1"
	settings.Browser.ContainerName = "surf-test"
	settings.Browser.Network = "test-net"
	settings.Browser.ProfileName = "work"
	settings.Browser.HostMCPPort = 9999
	settings.Browser.HostNoVNCPort = 6090
	settings.Browser.MCPPort = 9900
	settings.Browser.NoVNCPort = 6091
	settings.Browser.VNCPassword = "topsecret"
	settings.Browser.MCPVersion = "9.9.9"
	settings.Browser.BrowserChannel = "chrome"
	settings.Browser.AllowedHosts = "example.com"
	if err := saveSurfSettings(settings); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}

	cfg := defaultConfig()
	if cfg.ImageName != "test/surf:1" || cfg.ContainerName != "surf-test" || cfg.Network != "test-net" {
		t.Fatalf("unexpected cfg identity: %#v", cfg)
	}
	if cfg.ProfileName != "work" || cfg.HostMCPPort != 9999 || cfg.HostNoVNCPort != 6090 {
		t.Fatalf("unexpected cfg ports/profile: %#v", cfg)
	}
	if cfg.VNCPassword != "topsecret" || cfg.MCPVersion != "9.9.9" {
		t.Fatalf("unexpected cfg creds/version: %#v", cfg)
	}
}

func TestSurfStateDirUsesSettings(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")
	t.Setenv("SURF_STATE_DIR", "")

	settings := defaultSurfSettings()
	settings.Paths.StateDir = filepath.Join(home, "custom-surf-state")
	if err := saveSurfSettings(settings); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}
	if got := surfStateDir(); got != filepath.Join(home, "custom-surf-state") {
		t.Fatalf("surfStateDir=%q", got)
	}
}

func TestTunnelSettingsRoundTrip(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")

	want := defaultSurfSettings()
	want.Tunnel.ContainerName = "surf-cloudflared-test"
	want.Tunnel.TargetURL = "http://127.0.0.1:6081/vnc.html?autoconnect=1&resize=scale"
	want.Tunnel.Mode = "token"
	want.Tunnel.Image = "cloudflare/cloudflared:2026.2.0"
	want.Tunnel.VaultKey = "SURF_CLOUDFLARE_TUNNEL_TOKEN"
	if err := saveSurfSettings(want); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}

	got, err := loadSurfSettings()
	if err != nil {
		t.Fatalf("loadSurfSettings: %v", err)
	}
	if got.Tunnel.ContainerName != want.Tunnel.ContainerName {
		t.Fatalf("tunnel.container_name=%q", got.Tunnel.ContainerName)
	}
	if got.Tunnel.TargetURL != want.Tunnel.TargetURL {
		t.Fatalf("tunnel.target_url=%q", got.Tunnel.TargetURL)
	}
	if got.Tunnel.Mode != want.Tunnel.Mode {
		t.Fatalf("tunnel.mode=%q", got.Tunnel.Mode)
	}
	if got.Tunnel.Image != want.Tunnel.Image {
		t.Fatalf("tunnel.image=%q", got.Tunnel.Image)
	}
	if got.Tunnel.VaultKey != want.Tunnel.VaultKey {
		t.Fatalf("tunnel.vault_key=%q", got.Tunnel.VaultKey)
	}
}

func TestExistingSessionSettingsRoundTrip(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")

	want := defaultSurfSettings()
	want.ExistingSession.Enabled = true
	want.ExistingSession.DefaultBrowser = "chrome"
	want.ExistingSession.Mode = sessionModeInteractive
	want.ExistingSession.ChromeHost = "127.0.0.1"
	want.ExistingSession.ChromeCDPPort = 19922
	want.ExistingSession.AttachTimeoutSec = 12
	want.ExistingSession.ActionTimeoutSec = 20
	if err := saveSurfSettings(want); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}

	got, err := loadSurfSettings()
	if err != nil {
		t.Fatalf("loadSurfSettings: %v", err)
	}
	if got.ExistingSession.DefaultBrowser != "chrome" {
		t.Fatalf("existing_session.default_browser=%q", got.ExistingSession.DefaultBrowser)
	}
	if got.ExistingSession.Mode != sessionModeInteractive {
		t.Fatalf("existing_session.mode=%q", got.ExistingSession.Mode)
	}
	if got.ExistingSession.ChromeCDPPort != 19922 {
		t.Fatalf("existing_session.chrome_cdp_port=%d", got.ExistingSession.ChromeCDPPort)
	}
	if got.ExistingSession.AttachTimeoutSec != 12 {
		t.Fatalf("existing_session.attach_timeout_seconds=%d", got.ExistingSession.AttachTimeoutSec)
	}
	if got.ExistingSession.ActionTimeoutSec != 20 {
		t.Fatalf("existing_session.action_timeout_seconds=%d", got.ExistingSession.ActionTimeoutSec)
	}
}

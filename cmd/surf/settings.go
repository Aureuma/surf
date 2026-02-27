package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/pelletier/go-toml/v2"
)

const (
	surfSettingsSchemaVersion = 1
	defaultSurfConfigRoot     = "~/.si/surf"
	defaultSurfConfigFile     = "~/.si/surf/settings.toml"
	defaultSurfStateDirPath   = "~/.surf"
)

type surfSettings struct {
	SchemaVersion int                  `toml:"schema_version" json:"schema_version"`
	Paths         surfSettingsPaths    `toml:"paths" json:"paths"`
	Browser       surfBrowserSettings  `toml:"browser" json:"browser"`
	Tunnel        surfTunnelSettings   `toml:"tunnel" json:"tunnel"`
	Metadata      surfSettingsMetadata `toml:"metadata,omitempty" json:"metadata,omitempty"`
}

type surfSettingsMetadata struct {
	UpdatedAt string `toml:"updated_at,omitempty" json:"updated_at,omitempty"`
}

type surfSettingsPaths struct {
	Root         string `toml:"root,omitempty" json:"root,omitempty"`
	SettingsFile string `toml:"settings_file,omitempty" json:"settings_file,omitempty"`
	StateDir     string `toml:"state_dir,omitempty" json:"state_dir,omitempty"`
}

type surfBrowserSettings struct {
	ImageName      string `toml:"image_name,omitempty" json:"image_name,omitempty"`
	ContainerName  string `toml:"container_name,omitempty" json:"container_name,omitempty"`
	Network        string `toml:"network,omitempty" json:"network,omitempty"`
	ProfileName    string `toml:"profile_name,omitempty" json:"profile_name,omitempty"`
	ProfileDir     string `toml:"profile_dir,omitempty" json:"profile_dir,omitempty"`
	HostBind       string `toml:"host_bind,omitempty" json:"host_bind,omitempty"`
	HostMCPPort    int    `toml:"host_mcp_port,omitempty" json:"host_mcp_port,omitempty"`
	HostNoVNCPort  int    `toml:"host_novnc_port,omitempty" json:"host_novnc_port,omitempty"`
	MCPPort        int    `toml:"mcp_port,omitempty" json:"mcp_port,omitempty"`
	NoVNCPort      int    `toml:"novnc_port,omitempty" json:"novnc_port,omitempty"`
	VNCPassword    string `toml:"vnc_password,omitempty" json:"vnc_password,omitempty"`
	MCPVersion     string `toml:"mcp_version,omitempty" json:"mcp_version,omitempty"`
	BrowserChannel string `toml:"browser_channel,omitempty" json:"browser_channel,omitempty"`
	AllowedHosts   string `toml:"allowed_hosts,omitempty" json:"allowed_hosts,omitempty"`
}

type surfTunnelSettings struct {
	ContainerName string `toml:"container_name,omitempty" json:"container_name,omitempty"`
	TargetURL     string `toml:"target_url,omitempty" json:"target_url,omitempty"`
	Mode          string `toml:"mode,omitempty" json:"mode,omitempty"`
	Image         string `toml:"image,omitempty" json:"image,omitempty"`
	VaultKey      string `toml:"vault_key,omitempty" json:"vault_key,omitempty"`
}

func defaultSurfSettings() surfSettings {
	return surfSettings{
		SchemaVersion: surfSettingsSchemaVersion,
		Paths: surfSettingsPaths{
			Root:         defaultSurfConfigRoot,
			SettingsFile: defaultSurfConfigFile,
			StateDir:     defaultSurfStateDirPath,
		},
		Browser: surfBrowserSettings{
			ImageName:      defaultImage,
			ContainerName:  defaultContainer,
			Network:        defaultNetwork,
			ProfileName:    defaultProfileName,
			HostBind:       defaultHostBind,
			HostMCPPort:    defaultHostMCPPort,
			HostNoVNCPort:  defaultHostNoVNCPort,
			MCPPort:        defaultMCPPort,
			NoVNCPort:      defaultNoVNCPort,
			VNCPassword:    "surf",
			MCPVersion:     defaultMCPVersion,
			BrowserChannel: "chromium",
			AllowedHosts:   "*",
		},
		Tunnel: surfTunnelSettings{
			ContainerName: defaultTunnelName,
			Mode:          "quick",
			Image:         defaultCloudflaredImg,
		},
	}
}

func applySurfSettingsDefaults(settings *surfSettings) {
	if settings == nil {
		return
	}
	if settings.SchemaVersion <= 0 {
		settings.SchemaVersion = surfSettingsSchemaVersion
	}
	if strings.TrimSpace(settings.Paths.Root) == "" {
		settings.Paths.Root = defaultSurfConfigRoot
	}
	if strings.TrimSpace(settings.Paths.SettingsFile) == "" {
		settings.Paths.SettingsFile = defaultSurfConfigFile
	}
	if strings.TrimSpace(settings.Paths.StateDir) == "" {
		settings.Paths.StateDir = defaultSurfStateDirPath
	}
	if strings.TrimSpace(settings.Browser.ImageName) == "" {
		settings.Browser.ImageName = defaultImage
	}
	if strings.TrimSpace(settings.Browser.ContainerName) == "" {
		settings.Browser.ContainerName = defaultContainer
	}
	if strings.TrimSpace(settings.Browser.Network) == "" {
		settings.Browser.Network = defaultNetwork
	}
	if strings.TrimSpace(settings.Browser.ProfileName) == "" {
		settings.Browser.ProfileName = defaultProfileName
	}
	if strings.TrimSpace(settings.Browser.HostBind) == "" {
		settings.Browser.HostBind = defaultHostBind
	}
	if settings.Browser.HostMCPPort <= 0 {
		settings.Browser.HostMCPPort = defaultHostMCPPort
	}
	if settings.Browser.HostNoVNCPort <= 0 {
		settings.Browser.HostNoVNCPort = defaultHostNoVNCPort
	}
	if settings.Browser.MCPPort <= 0 {
		settings.Browser.MCPPort = defaultMCPPort
	}
	if settings.Browser.NoVNCPort <= 0 {
		settings.Browser.NoVNCPort = defaultNoVNCPort
	}
	if strings.TrimSpace(settings.Browser.VNCPassword) == "" {
		settings.Browser.VNCPassword = "surf"
	}
	if strings.TrimSpace(settings.Browser.MCPVersion) == "" {
		settings.Browser.MCPVersion = defaultMCPVersion
	}
	if strings.TrimSpace(settings.Browser.BrowserChannel) == "" {
		settings.Browser.BrowserChannel = "chromium"
	}
	if strings.TrimSpace(settings.Browser.AllowedHosts) == "" {
		settings.Browser.AllowedHosts = "*"
	}
	if strings.TrimSpace(settings.Tunnel.ContainerName) == "" {
		settings.Tunnel.ContainerName = defaultTunnelName
	}
	if strings.TrimSpace(settings.Tunnel.Mode) == "" {
		settings.Tunnel.Mode = "quick"
	}
	mode := strings.ToLower(strings.TrimSpace(settings.Tunnel.Mode))
	if mode != "quick" && mode != "token" {
		settings.Tunnel.Mode = "quick"
	}
	if strings.TrimSpace(settings.Tunnel.Image) == "" {
		settings.Tunnel.Image = defaultCloudflaredImg
	}
}

func surfSettingsPath() string {
	if explicit := strings.TrimSpace(os.Getenv("SURF_SETTINGS_FILE")); explicit != "" {
		return filepath.Clean(expandTilde(explicit))
	}
	return filepath.Join(surfSettingsRootDir(), "settings.toml")
}

func surfSettingsRootDir() string {
	if explicit := strings.TrimSpace(os.Getenv("SURF_SETTINGS_DIR")); explicit != "" {
		return filepath.Clean(expandTilde(explicit))
	}
	if homeOverride := strings.TrimSpace(os.Getenv("SURF_SETTINGS_HOME")); homeOverride != "" {
		return filepath.Join(filepath.Clean(expandTilde(homeOverride)), ".si", "surf")
	}
	home, err := os.UserHomeDir()
	if err != nil || strings.TrimSpace(home) == "" {
		return filepath.Clean(expandTilde(defaultSurfConfigRoot))
	}
	return filepath.Join(home, ".si", "surf")
}

func loadSurfSettings() (surfSettings, error) {
	path := surfSettingsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		fallback := defaultSurfSettings()
		applySurfSettingsDefaults(&fallback)
		return fallback, err
	}
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			settings := defaultSurfSettings()
			applySurfSettingsDefaults(&settings)
			if saveErr := saveSurfSettings(settings); saveErr != nil {
				return settings, saveErr
			}
			return settings, nil
		}
		fallback := defaultSurfSettings()
		applySurfSettingsDefaults(&fallback)
		return fallback, fmt.Errorf("read surf settings %s: %w", path, err)
	}
	settings := defaultSurfSettings()
	if err := toml.Unmarshal(data, &settings); err != nil {
		fallback := defaultSurfSettings()
		applySurfSettingsDefaults(&fallback)
		return fallback, fmt.Errorf("parse surf settings %s: %w", path, err)
	}
	applySurfSettingsDefaults(&settings)
	if saveErr := saveSurfSettings(settings); saveErr != nil {
		return settings, saveErr
	}
	return settings, nil
}

func loadSurfSettingsOrDefault() surfSettings {
	settings, err := loadSurfSettings()
	if err != nil {
		fmt.Fprintf(os.Stderr, "warning: surf settings load failed: %v\n", err)
		fallback := defaultSurfSettings()
		applySurfSettingsDefaults(&fallback)
		return fallback
	}
	return settings
}

func saveSurfSettings(settings surfSettings) error {
	path := surfSettingsPath()
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	applySurfSettingsDefaults(&settings)
	settings.Paths.Root = defaultSurfConfigRoot
	settings.Paths.SettingsFile = defaultSurfConfigFile
	settings.Metadata.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
	data, err := toml.Marshal(settings)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, "settings-*.toml")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Chmod(tmp.Name(), 0o600); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}

func setSurfConfigValue(settings *surfSettings, key, value string) error {
	if settings == nil {
		return errors.New("settings is nil")
	}
	k := strings.ToLower(strings.TrimSpace(key))
	v := strings.TrimSpace(value)
	switch k {
	case "paths.state_dir":
		settings.Paths.StateDir = v
	case "browser.image_name":
		settings.Browser.ImageName = v
	case "browser.container_name":
		settings.Browser.ContainerName = v
	case "browser.network":
		settings.Browser.Network = v
	case "browser.profile_name":
		settings.Browser.ProfileName = sanitizeProfileName(v)
	case "browser.profile_dir":
		settings.Browser.ProfileDir = v
	case "browser.host_bind":
		settings.Browser.HostBind = v
	case "browser.host_mcp_port":
		port, err := strconv.Atoi(v)
		if err != nil {
			return fmt.Errorf("invalid int for %s: %w", key, err)
		}
		settings.Browser.HostMCPPort = port
	case "browser.host_novnc_port":
		port, err := strconv.Atoi(v)
		if err != nil {
			return fmt.Errorf("invalid int for %s: %w", key, err)
		}
		settings.Browser.HostNoVNCPort = port
	case "browser.mcp_port":
		port, err := strconv.Atoi(v)
		if err != nil {
			return fmt.Errorf("invalid int for %s: %w", key, err)
		}
		settings.Browser.MCPPort = port
	case "browser.novnc_port":
		port, err := strconv.Atoi(v)
		if err != nil {
			return fmt.Errorf("invalid int for %s: %w", key, err)
		}
		settings.Browser.NoVNCPort = port
	case "browser.vnc_password":
		settings.Browser.VNCPassword = v
	case "browser.mcp_version":
		settings.Browser.MCPVersion = v
	case "browser.browser_channel":
		settings.Browser.BrowserChannel = v
	case "browser.allowed_hosts":
		settings.Browser.AllowedHosts = v
	case "tunnel.container_name":
		settings.Tunnel.ContainerName = v
	case "tunnel.target_url":
		settings.Tunnel.TargetURL = v
	case "tunnel.mode":
		mode := strings.ToLower(v)
		if mode != "quick" && mode != "token" {
			return fmt.Errorf("invalid mode %q (expected quick|token)", v)
		}
		settings.Tunnel.Mode = mode
	case "tunnel.image":
		settings.Tunnel.Image = v
	case "tunnel.vault_key":
		settings.Tunnel.VaultKey = v
	default:
		return fmt.Errorf("unsupported key: %s", key)
	}
	applySurfSettingsDefaults(settings)
	return nil
}

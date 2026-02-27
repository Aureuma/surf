package main

import (
	"flag"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHostConnect(t *testing.T) {
	if got := hostConnect(""); got != "127.0.0.1" {
		t.Fatalf("hostConnect empty got %q", got)
	}
	if got := hostConnect("0.0.0.0"); got != "127.0.0.1" {
		t.Fatalf("hostConnect any got %q", got)
	}
	if got := hostConnect("192.168.1.20"); got != "192.168.1.20" {
		t.Fatalf("hostConnect explicit got %q", got)
	}
}

func TestExtractTryCloudflareURL(t *testing.T) {
	logs := "random\nhttps://alpha.trycloudflare.com\nother\nhttps://beta.trycloudflare.com\n"
	if got := extractTryCloudflareURL(logs); got != "https://beta.trycloudflare.com" {
		t.Fatalf("extractTryCloudflareURL got %q", got)
	}
}

func TestMCPURL(t *testing.T) {
	cfg := browserConfig{HostBind: "127.0.0.1", HostMCPPort: 8932}
	if got := mcpURL(cfg); got != "http://127.0.0.1:8932/mcp" {
		t.Fatalf("mcpURL got %q", got)
	}
}

func TestProfilePathLayout(t *testing.T) {
	t.Setenv("SURF_STATE_DIR", "/tmp/surf-state")
	c := containerProfileDir("Work")
	h := hostProfileDir("work")
	if c != filepath.Clean("/tmp/surf-state/browser/profiles/container/work") {
		t.Fatalf("container profile path mismatch: %s", c)
	}
	if h != filepath.Clean("/tmp/surf-state/browser/profiles/host/work") {
		t.Fatalf("host profile path mismatch: %s", h)
	}
}

func TestHostLaunchArgsBase(t *testing.T) {
	args := hostLaunchArgs(18800, "/tmp/profile", false, ":99")
	joined := strings.Join(args, " ")
	if !strings.Contains(joined, "--remote-debugging-port=18800") {
		t.Fatalf("missing cdp port in args: %v", args)
	}
	if !strings.Contains(joined, "--user-data-dir=/tmp/profile") {
		t.Fatalf("missing profile dir in args: %v", args)
	}
	if strings.Contains(joined, "--no-sandbox") {
		t.Fatalf("unexpected --no-sandbox for non-root launch: %v", args)
	}
	if strings.Contains(joined, "--headless=new") {
		t.Fatalf("unexpected headless flag when DISPLAY is present: %v", args)
	}
	if args[len(args)-1] != "about:blank" {
		t.Fatalf("expected about:blank terminal arg, got: %v", args)
	}
}

func TestHostLaunchArgsRootNoDisplay(t *testing.T) {
	args := hostLaunchArgs(18800, "/tmp/profile", true, "")
	joined := strings.Join(args, " ")
	if !strings.Contains(joined, "--no-sandbox") || !strings.Contains(joined, "--disable-setuid-sandbox") {
		t.Fatalf("expected root sandbox flags: %v", args)
	}
	if !strings.Contains(joined, "--headless=new") {
		t.Fatalf("expected headless flag when DISPLAY is empty: %v", args)
	}
}

func TestDefaultHostProfileNameFromSettings(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")
	t.Setenv("SURF_HOST_PROFILE", "")

	settings := defaultSurfSettings()
	settings.Browser.ProfileName = "lingospeak"
	if err := saveSurfSettings(settings); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}

	if got := defaultHostProfileName(); got != "lingospeak" {
		t.Fatalf("defaultHostProfileName()=%q", got)
	}
}

func TestDefaultHostProfileNameEnvOverride(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", "")
	t.Setenv("SURF_HOST_PROFILE", "prod-main")

	settings := defaultSurfSettings()
	settings.Browser.ProfileName = "lingospeak"
	if err := saveSurfSettings(settings); err != nil {
		t.Fatalf("saveSurfSettings: %v", err)
	}

	if got := defaultHostProfileName(); got != "prod-main" {
		t.Fatalf("defaultHostProfileName()=%q", got)
	}
}

func TestDefaultHostProfileNameFallback(t *testing.T) {
	home := t.TempDir()
	t.Setenv("SURF_SETTINGS_HOME", home)
	t.Setenv("SURF_SETTINGS_FILE", filepath.Join(home, "missing", "settings.toml"))
	t.Setenv("SURF_HOST_PROFILE", "")
	if err := os.RemoveAll(filepath.Join(home, "missing")); err != nil {
		t.Fatalf("remove missing dir: %v", err)
	}

	if got := defaultHostProfileName(); got != "default" {
		t.Fatalf("defaultHostProfileName()=%q", got)
	}
}

func TestResolveProfileMountBindPath(t *testing.T) {
	mount, hostPath, bind := resolveProfileMount("~/state/profile")
	if !bind {
		t.Fatalf("expected bind mount")
	}
	if !strings.HasSuffix(hostPath, "/state/profile") {
		t.Fatalf("hostPath=%q", hostPath)
	}
	if !strings.Contains(mount, ":/home/pwuser/.playwright-mcp-profile") {
		t.Fatalf("mount=%q", mount)
	}
}

func TestResolveProfileMountVolume(t *testing.T) {
	mount, hostPath, bind := resolveProfileMount("volume:ls")
	if bind {
		t.Fatalf("expected docker volume mount")
	}
	if hostPath != "" {
		t.Fatalf("hostPath=%q", hostPath)
	}
	if mount != "surf-profile-ls:/home/pwuser/.playwright-mcp-profile" {
		t.Fatalf("mount=%q", mount)
	}
}

func TestApplyContainerProfileDefaultKeepsConfiguredProfileDir(t *testing.T) {
	fs := flag.NewFlagSet("test", flag.ContinueOnError)
	cfg := &browserConfig{
		ProfileName: "ls",
		ProfileDir:  "volume:ls",
	}
	applyContainerProfileDefault(fs, cfg)
	if cfg.ProfileDir != "volume:ls" {
		t.Fatalf("profile_dir=%q", cfg.ProfileDir)
	}
}

func TestApplyContainerProfileDefaultSetsPathWhenEmpty(t *testing.T) {
	t.Setenv("SURF_STATE_DIR", "/tmp/surf-state")
	fs := flag.NewFlagSet("test", flag.ContinueOnError)
	cfg := &browserConfig{
		ProfileName: "work",
		ProfileDir:  "",
	}
	applyContainerProfileDefault(fs, cfg)
	if cfg.ProfileDir != filepath.Clean("/tmp/surf-state/browser/profiles/container/work") {
		t.Fatalf("profile_dir=%q", cfg.ProfileDir)
	}
}

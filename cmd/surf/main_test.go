package main

import (
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

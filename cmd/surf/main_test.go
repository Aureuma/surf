package main

import "testing"

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

package main

import (
	"fmt"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestSessionE2EChromeHeadless(t *testing.T) {
	if strings.TrimSpace(os.Getenv("SURF_E2E_CHROME")) != "1" {
		t.Skip("set SURF_E2E_CHROME=1 to run chrome e2e")
	}
	chromeBin := findChromeBinary()
	if chromeBin == "" {
		t.Skip("chrome/chromium binary not found")
	}

	port := reserveTCPPort(t)
	profileDir, err := os.MkdirTemp("", "surf-e2e-profile-*")
	if err != nil {
		t.Fatalf("mktemp profile: %v", err)
	}

	url := "about:blank"
	cmd := exec.Command(chromeBin,
		"--headless=new",
		fmt.Sprintf("--remote-debugging-port=%d", port),
		fmt.Sprintf("--user-data-dir=%s", profileDir),
		"--disable-gpu",
		"--no-sandbox",
		"--no-first-run",
		"--no-default-browser-check",
		url,
	)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		t.Fatalf("start chrome: %v", err)
	}
	defer func() {
		_ = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
		for i := 0; i < 20; i++ {
			if err := os.RemoveAll(profileDir); err == nil {
				break
			}
			time.Sleep(100 * time.Millisecond)
		}
	}()

	waitForCDP(t, port, 15*time.Second)
	targets, err := discoverChromeTargets("127.0.0.1", port, 5*time.Second)
	if err != nil {
		t.Fatalf("discover: %v", err)
	}
	target, err := chooseChromeTarget(targets, "", "", "")
	if err != nil {
		t.Fatalf("choose target: %v", err)
	}
	s := attachedSession{
		Session:  "e2e",
		Browser:  "chrome",
		Mode:     sessionModeInteractive,
		TargetID: target.ID,
		Title:    target.Title,
		URL:      target.URL,
		WSURL:    target.WebSocketDebuggerURL,
		CDPHost:  "127.0.0.1",
		CDPPort:  port,
	}
	t.Setenv("SURF_STATE_DIR", t.TempDir())
	if err := writeAttachedSession(s); err != nil {
		t.Fatalf("write session: %v", err)
	}

	if _, err := runSessionAction(s, sessionActionRequest{
		Action: "eval",
		Expr:   "document.title='SurfE2E'; document.body.innerHTML = \"<input id='q' value=''><button id='btn' onclick=\\\"window.clicked='yes'\\\">go</button><p id='txt'>hello</p>\"; 'ok'",
	}); err != nil {
		t.Fatalf("bootstrap dom action: %v", err)
	}
	titleResult, err := runSessionAction(s, sessionActionRequest{Action: "title"})
	if err != nil {
		t.Fatalf("title action: %v", err)
	}
	if titleResult.Value != "SurfE2E" {
		t.Fatalf("title value=%v", titleResult.Value)
	}
	if _, err := runSessionAction(s, sessionActionRequest{Action: "type", Selector: "#q", Text: "hello"}); err != nil {
		t.Fatalf("type action: %v", err)
	}
	valueResult, err := runSessionAction(s, sessionActionRequest{Action: "eval", Expr: "document.querySelector('#q').value"})
	if err != nil {
		t.Fatalf("eval value action: %v", err)
	}
	if valueResult.Value != "hello" {
		t.Fatalf("input value=%v", valueResult.Value)
	}
	if _, err := runSessionAction(s, sessionActionRequest{Action: "click", Selector: "#btn"}); err != nil {
		t.Fatalf("click action: %v", err)
	}
	clickedResult, err := runSessionAction(s, sessionActionRequest{Action: "eval", Expr: "window.clicked || ''"})
	if err != nil {
		t.Fatalf("eval click marker action: %v", err)
	}
	if clickedResult.Value != "yes" {
		t.Fatalf("clicked marker=%v", clickedResult.Value)
	}
	shot := filepath.Join(t.TempDir(), "e2e.png")
	if _, err := runSessionAction(s, sessionActionRequest{Action: "screenshot", Out: shot}); err != nil {
		t.Fatalf("screenshot action: %v", err)
	}
	info, err := os.Stat(shot)
	if err != nil {
		t.Fatalf("stat screenshot: %v", err)
	}
	if info.Size() == 0 {
		t.Fatalf("screenshot is empty")
	}
}

func waitForCDP(t *testing.T, port int, timeout time.Duration) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		resp, err := http.Get(fmt.Sprintf("http://127.0.0.1:%d/json/list", port))
		if err == nil {
			_ = resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				return
			}
		}
		time.Sleep(200 * time.Millisecond)
	}
	t.Fatalf("cdp endpoint not ready on port %d", port)
}

func reserveTCPPort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("reserve tcp port: %v", err)
	}
	defer ln.Close()
	return ln.Addr().(*net.TCPAddr).Port
}

func findChromeBinary() string {
	if explicit := strings.TrimSpace(os.Getenv("CHROME_BIN")); explicit != "" {
		return explicit
	}
	for _, name := range []string{"google-chrome", "google-chrome-stable", "chromium", "chromium-browser"} {
		if p, err := exec.LookPath(name); err == nil {
			return p
		}
	}
	return ""
}

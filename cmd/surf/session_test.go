package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"nhooyr.io/websocket"
	"nhooyr.io/websocket/wsjson"
)

func TestChooseChromeTarget(t *testing.T) {
	targets := []chromeTarget{
		{ID: "a1", Title: "Inbox", URL: "https://mail.example.com", WebSocketDebuggerURL: "ws://127.0.0.1:1/devtools/page/a1"},
		{ID: "b2", Title: "Docs", URL: "https://docs.example.com", WebSocketDebuggerURL: "ws://127.0.0.1:1/devtools/page/b2"},
	}
	got, err := chooseChromeTarget(targets, "b2", "", "")
	if err != nil {
		t.Fatalf("choose by id: %v", err)
	}
	if got.ID != "b2" {
		t.Fatalf("choose by id got %q", got.ID)
	}
	got, err = chooseChromeTarget(targets, "", "mail", "")
	if err != nil {
		t.Fatalf("choose by url filter: %v", err)
	}
	if got.ID != "a1" {
		t.Fatalf("choose by url got %q", got.ID)
	}
	if _, err := chooseChromeTarget(targets, "", "", ""); err == nil {
		t.Fatalf("expected ambiguity error")
	}
}

func TestDiscoverChromeTargets(t *testing.T) {
	jsonList := []chromeTarget{
		{ID: "page-1", Type: "page", Title: "One", URL: "https://one.example", WebSocketDebuggerURL: "ws://127.0.0.1:123/devtools/page/page-1"},
		{ID: "worker-1", Type: "worker", Title: "Worker", URL: "", WebSocketDebuggerURL: "ws://127.0.0.1:123/devtools/page/worker-1"},
		{ID: "page-2", Type: "page", Title: "Two", URL: "https://two.example", WebSocketDebuggerURL: ""},
	}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/json/list" {
			http.NotFound(w, r)
			return
		}
		_ = json.NewEncoder(w).Encode(jsonList)
	}))
	defer srv.Close()

	host, port, err := splitHostPortFromURL(srv.URL)
	if err != nil {
		t.Fatalf("split host/port: %v", err)
	}
	targets, err := discoverChromeTargets(host, port, 2*time.Second)
	if err != nil {
		t.Fatalf("discoverChromeTargets: %v", err)
	}
	if len(targets) != 1 {
		t.Fatalf("expected 1 filtered target, got %d", len(targets))
	}
	if targets[0].ID != "page-1" {
		t.Fatalf("unexpected target id: %s", targets[0].ID)
	}
}

func TestSessionStateRoundTrip(t *testing.T) {
	t.Setenv("SURF_STATE_DIR", t.TempDir())
	in := attachedSession{
		Session:   "alpha",
		Browser:   "chrome",
		Mode:      sessionModeReadOnly,
		TargetID:  "tab-1",
		Title:     "Example",
		URL:       "https://example.com",
		WSURL:     "ws://127.0.0.1:9222/devtools/page/tab-1",
		CDPHost:   "127.0.0.1",
		CDPPort:   9222,
		CreatedAt: time.Now().UTC().Format(time.RFC3339),
	}
	if err := writeAttachedSession(in); err != nil {
		t.Fatalf("writeAttachedSession: %v", err)
	}
	got, err := readAttachedSession("alpha")
	if err != nil {
		t.Fatalf("readAttachedSession: %v", err)
	}
	if got.Session != "alpha" || got.TargetID != "tab-1" {
		t.Fatalf("round trip mismatch: %+v", got)
	}
	list, err := listAttachedSessions()
	if err != nil {
		t.Fatalf("listAttachedSessions: %v", err)
	}
	if len(list) != 1 || list[0].Session != "alpha" {
		t.Fatalf("unexpected session list: %+v", list)
	}
	if err := deleteAttachedSession("alpha"); err != nil {
		t.Fatalf("deleteAttachedSession: %v", err)
	}
	if _, err := readAttachedSession("alpha"); err == nil {
		t.Fatalf("expected error after delete")
	}
}

func TestRunSessionActionReadOnlyBlocksWrite(t *testing.T) {
	s := attachedSession{Session: "ro", Browser: "chrome", Mode: sessionModeReadOnly, WSURL: "ws://127.0.0.1:1/devtools/page/x"}
	_, err := runSessionAction(s, sessionActionRequest{Action: "click", Selector: "#x"})
	if err == nil || !strings.Contains(err.Error(), "requires interactive mode") {
		t.Fatalf("expected interactive mode error, got: %v", err)
	}
}

func TestRunSessionActionChromeCDP(t *testing.T) {
	wsURL, calls, closeFn := startMockCDPTargetServer(t)
	defer closeFn()

	s := attachedSession{
		Session: "work",
		Browser: "chrome",
		Mode:    sessionModeInteractive,
		WSURL:   wsURL,
	}

	titleRes, err := runSessionAction(s, sessionActionRequest{Action: "title"})
	if err != nil {
		t.Fatalf("title action: %v", err)
	}
	if titleRes.Value != "Mock Title" {
		t.Fatalf("title=%v", titleRes.Value)
	}
	urlRes, err := runSessionAction(s, sessionActionRequest{Action: "url"})
	if err != nil {
		t.Fatalf("url action: %v", err)
	}
	if urlRes.Value != "https://example.test/page" {
		t.Fatalf("url=%v", urlRes.Value)
	}
	textRes, err := runSessionAction(s, sessionActionRequest{Action: "text"})
	if err != nil {
		t.Fatalf("text action: %v", err)
	}
	if textRes.Value != "Example Body" {
		t.Fatalf("text=%v", textRes.Value)
	}
	evalRes, err := runSessionAction(s, sessionActionRequest{Action: "eval", Expr: "2+2"})
	if err != nil {
		t.Fatalf("eval action: %v", err)
	}
	if evalRes.Value != float64(4) {
		t.Fatalf("eval=%v", evalRes.Value)
	}
	clickRes, err := runSessionAction(s, sessionActionRequest{Action: "click", Selector: "#go"})
	if err != nil {
		t.Fatalf("click action: %v", err)
	}
	clickMap, ok := clickRes.Value.(map[string]any)
	if !ok || clickMap["ok"] != true {
		t.Fatalf("click value=%v", clickRes.Value)
	}
	shot := filepath.Join(t.TempDir(), "shot.png")
	screenshotRes, err := runSessionAction(s, sessionActionRequest{Action: "screenshot", Out: shot})
	if err != nil {
		t.Fatalf("screenshot action: %v", err)
	}
	if screenshotRes.Output != shot {
		t.Fatalf("screenshot output=%q", screenshotRes.Output)
	}
	data, err := os.ReadFile(shot)
	if err != nil {
		t.Fatalf("read screenshot: %v", err)
	}
	if string(data) != "PNGDATA" {
		t.Fatalf("unexpected screenshot payload %q", string(data))
	}
	if len(*calls) == 0 {
		t.Fatalf("expected CDP calls to be captured")
	}
}

func startMockCDPTargetServer(t *testing.T) (string, *[]string, func()) {
	t.Helper()

	var (
		mu    sync.Mutex
		calls []string
	)

	mux := http.NewServeMux()
	srv := httptest.NewServer(mux)
	path := "/devtools/page/mock-1"

	mux.HandleFunc(path, func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			t.Fatalf("websocket accept: %v", err)
		}
		defer conn.Close(websocket.StatusNormalClosure, "done")

		ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
		defer cancel()
		for {
			var req cdpRequest
			if err := wsjson.Read(ctx, conn, &req); err != nil {
				return
			}
			mu.Lock()
			calls = append(calls, req.Method)
			mu.Unlock()

			result := map[string]any{}
			switch req.Method {
			case "Runtime.evaluate":
				expr := expressionFromParams(req.Params)
				result = map[string]any{"result": map[string]any{"type": "string", "value": ""}}
				switch {
				case strings.Contains(expr, "document.title"):
					result = map[string]any{"result": map[string]any{"type": "string", "value": "Mock Title"}}
				case strings.Contains(expr, "location.href"):
					result = map[string]any{"result": map[string]any{"type": "string", "value": "https://example.test/page"}}
				case strings.Contains(expr, "innerText"):
					result = map[string]any{"result": map[string]any{"type": "string", "value": "Example Body"}}
				case strings.Contains(expr, "el.click"):
					result = map[string]any{"result": map[string]any{"type": "object", "value": map[string]any{"ok": true}}}
				case strings.Contains(expr, "el.value"):
					result = map[string]any{"result": map[string]any{"type": "object", "value": map[string]any{"ok": true, "length": 5}}}
				default:
					result = map[string]any{"result": map[string]any{"type": "number", "value": 4}}
				}
			case "Page.captureScreenshot":
				result = map[string]any{"data": base64.StdEncoding.EncodeToString([]byte("PNGDATA"))}
			}
			resp := map[string]any{"id": req.ID, "result": result}
			if err := wsjson.Write(ctx, conn, resp); err != nil {
				return
			}
		}
	})

	wsURL := "ws" + strings.TrimPrefix(srv.URL, "http") + path
	return wsURL, &calls, srv.Close
}

func expressionFromParams(params any) string {
	m, ok := params.(map[string]any)
	if !ok {
		return ""
	}
	expr, _ := m["expression"].(string)
	return expr
}

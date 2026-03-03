package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"nhooyr.io/websocket"
	"nhooyr.io/websocket/wsjson"
)

const (
	sessionModeReadOnly    = "read_only"
	sessionModeInteractive = "interactive"
)

type chromeTarget struct {
	ID                   string `json:"id"`
	Type                 string `json:"type"`
	Title                string `json:"title"`
	URL                  string `json:"url"`
	WebSocketDebuggerURL string `json:"webSocketDebuggerUrl"`
}

type attachedSession struct {
	Session   string `json:"session"`
	Browser   string `json:"browser"`
	Mode      string `json:"mode"`
	TargetID  string `json:"target_id"`
	Title     string `json:"title"`
	URL       string `json:"url"`
	WSURL     string `json:"ws_url"`
	CDPHost   string `json:"cdp_host"`
	CDPPort   int    `json:"cdp_port"`
	CreatedAt string `json:"created_at"`
}

type sessionActionRequest struct {
	Action   string
	Selector string
	Text     string
	Expr     string
	Out      string
}

type sessionActionResult struct {
	Action  string `json:"action"`
	Session string `json:"session"`
	Mode    string `json:"mode"`
	Value   any    `json:"value,omitempty"`
	Output  string `json:"output,omitempty"`
}

type cdpRequest struct {
	ID     int64  `json:"id"`
	Method string `json:"method"`
	Params any    `json:"params,omitempty"`
}

type cdpResponse struct {
	ID     int64           `json:"id"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *cdpError       `json:"error,omitempty"`
	Method string          `json:"method,omitempty"`
}

type cdpError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func cmdSession(args []string) {
	if len(args) == 0 {
		fatal(errors.New("usage: surf session <discover|attach|list|detach|act> [args]"))
	}
	sub := strings.ToLower(strings.TrimSpace(args[0]))
	rest := args[1:]
	switch sub {
	case "discover", "scan":
		cmdSessionDiscover(rest)
	case "attach":
		cmdSessionAttach(rest)
	case "list", "ls":
		cmdSessionList(rest)
	case "detach", "rm", "remove":
		cmdSessionDetach(rest)
	case "act", "action":
		cmdSessionAct(rest)
	default:
		fatal(fmt.Errorf("unknown session command: %s", sub))
	}
}

func cmdSessionDiscover(args []string) {
	fs := flag.NewFlagSet("session discover", flag.ExitOnError)
	browser := fs.String("browser", defaultSessionBrowser(), "browser adapter (chrome|safari)")
	host := fs.String("host", defaultSessionChromeHost(), "CDP host")
	cdpPort := fs.Int("cdp-port", defaultSessionChromePort(), "CDP port")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf session discover [--browser chrome|safari] [--host <host>] [--cdp-port <n>] [--json]"))
	}
	adapter := strings.ToLower(strings.TrimSpace(*browser))
	switch adapter {
	case "chrome", "chromium":
		targets, err := discoverChromeTargets(strings.TrimSpace(*host), *cdpPort, defaultSessionAttachTimeout())
		if err != nil {
			fatal(err)
		}
		if *jsonOut {
			printJSON(map[string]any{"ok": true, "browser": "chrome", "targets": targets})
			return
		}
		if len(targets) == 0 {
			fmt.Println("no browser targets discovered")
			return
		}
		fmt.Println("available browser targets:")
		for idx, target := range targets {
			title := strings.TrimSpace(target.Title)
			if title == "" {
				title = "(untitled)"
			}
			fmt.Printf("%d. [%s] %s\n", idx+1, target.ID, title)
			fmt.Printf("   %s\n", strings.TrimSpace(target.URL))
		}
	case "safari":
		fatal(errors.New("safari existing-session discovery is not implemented yet; use chrome/chromium"))
	default:
		fatal(fmt.Errorf("unsupported browser %q (expected chrome|safari)", adapter))
	}
}

func cmdSessionAttach(args []string) {
	fs := flag.NewFlagSet("session attach", flag.ExitOnError)
	browser := fs.String("browser", defaultSessionBrowser(), "browser adapter (chrome|safari)")
	host := fs.String("host", defaultSessionChromeHost(), "CDP host")
	cdpPort := fs.Int("cdp-port", defaultSessionChromePort(), "CDP port")
	targetID := fs.String("id", "", "target id to attach")
	urlContains := fs.String("url-contains", "", "match target URL contains text")
	titleContains := fs.String("title-contains", "", "match target title contains text")
	sessionName := fs.String("session", "", "session name override")
	mode := fs.String("mode", defaultSessionMode(), "session mode (read_only|interactive)")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf session attach [--browser chrome|safari] [--host <host>] [--cdp-port <n>] [--id <target-id>] [--url-contains <text>] [--title-contains <text>] [--session <name>] [--mode read_only|interactive] [--json]"))
	}
	adapter := strings.ToLower(strings.TrimSpace(*browser))
	resolvedMode, err := normalizeSessionMode(*mode)
	if err != nil {
		fatal(err)
	}
	switch adapter {
	case "chrome", "chromium":
		targets, err := discoverChromeTargets(strings.TrimSpace(*host), *cdpPort, defaultSessionAttachTimeout())
		if err != nil {
			fatal(err)
		}
		target, err := chooseChromeTarget(targets, strings.TrimSpace(*targetID), strings.TrimSpace(*urlContains), strings.TrimSpace(*titleContains))
		if err != nil {
			fatal(err)
		}
		name := sanitizeProfileName(strings.TrimSpace(*sessionName))
		if strings.TrimSpace(*sessionName) == "" {
			name = sanitizeProfileName(firstNonEmpty(target.ID, target.Title))
		}
		session := attachedSession{
			Session:   name,
			Browser:   "chrome",
			Mode:      resolvedMode,
			TargetID:  target.ID,
			Title:     target.Title,
			URL:       target.URL,
			WSURL:     target.WebSocketDebuggerURL,
			CDPHost:   strings.TrimSpace(*host),
			CDPPort:   *cdpPort,
			CreatedAt: time.Now().UTC().Format(time.RFC3339),
		}
		if err := writeAttachedSession(session); err != nil {
			fatal(err)
		}
		if *jsonOut {
			printJSON(map[string]any{"ok": true, "session": session})
			return
		}
		fmt.Printf("attached: %s (%s)\n", session.Session, session.Mode)
		fmt.Printf("  target_id=%s\n", session.TargetID)
		fmt.Printf("  title=%s\n", strings.TrimSpace(session.Title))
		fmt.Printf("  url=%s\n", strings.TrimSpace(session.URL))
	case "safari":
		fatal(errors.New("safari existing-session attach is not implemented yet; use chrome/chromium"))
	default:
		fatal(fmt.Errorf("unsupported browser %q (expected chrome|safari)", adapter))
	}
}

func cmdSessionList(args []string) {
	fs := flag.NewFlagSet("session list", flag.ExitOnError)
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf session list [--json]"))
	}
	sessions, err := listAttachedSessions()
	if err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "sessions": sessions})
		return
	}
	if len(sessions) == 0 {
		fmt.Println("no attached sessions")
		return
	}
	for idx, session := range sessions {
		fmt.Printf("%d. %s [%s/%s]\n", idx+1, session.Session, session.Browser, session.Mode)
		fmt.Printf("   %s\n", strings.TrimSpace(session.URL))
	}
}

func cmdSessionDetach(args []string) {
	fs := flag.NewFlagSet("session detach", flag.ExitOnError)
	sessionName := fs.String("session", "", "attached session name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf session detach --session <name> [--json]"))
	}
	resolved := sanitizeProfileName(strings.TrimSpace(*sessionName))
	if strings.TrimSpace(*sessionName) == "" {
		fatal(errors.New("--session is required"))
	}
	if err := deleteAttachedSession(resolved); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "session": resolved})
		return
	}
	fmt.Printf("detached: %s\n", resolved)
}

func cmdSessionAct(args []string) {
	fs := flag.NewFlagSet("session act", flag.ExitOnError)
	sessionName := fs.String("session", "", "attached session name")
	action := fs.String("action", "", "action (title|url|text|screenshot|click|type|eval)")
	selector := fs.String("selector", "", "css selector for click/type")
	text := fs.String("text", "", "text for type")
	expr := fs.String("expr", "", "javascript expression for eval")
	out := fs.String("out", "", "output path for screenshot")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf session act --session <name> --action <name> [--selector <css>] [--text <value>] [--expr <js>] [--out <path>] [--json]"))
	}
	if strings.TrimSpace(*sessionName) == "" {
		fatal(errors.New("--session is required"))
	}
	if strings.TrimSpace(*action) == "" {
		fatal(errors.New("--action is required"))
	}
	session, err := readAttachedSession(sanitizeProfileName(*sessionName))
	if err != nil {
		fatal(err)
	}
	result, err := runSessionAction(session, sessionActionRequest{
		Action:   strings.ToLower(strings.TrimSpace(*action)),
		Selector: strings.TrimSpace(*selector),
		Text:     *text,
		Expr:     strings.TrimSpace(*expr),
		Out:      strings.TrimSpace(*out),
	})
	if err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "result": result})
		return
	}
	switch result.Action {
	case "screenshot":
		fmt.Printf("screenshot: %s\n", result.Output)
	default:
		fmt.Printf("%s: %v\n", result.Action, result.Value)
	}
}

func defaultSessionBrowser() string {
	settings := loadSurfSettingsOrDefault()
	browser := strings.ToLower(strings.TrimSpace(settings.ExistingSession.DefaultBrowser))
	if browser == "" {
		return "chrome"
	}
	return browser
}

func defaultSessionMode() string {
	settings := loadSurfSettingsOrDefault()
	mode := strings.ToLower(strings.TrimSpace(settings.ExistingSession.Mode))
	if mode == "" {
		return sessionModeReadOnly
	}
	return mode
}

func defaultSessionChromeHost() string {
	settings := loadSurfSettingsOrDefault()
	host := strings.TrimSpace(settings.ExistingSession.ChromeHost)
	if host == "" {
		return "127.0.0.1"
	}
	return host
}

func defaultSessionChromePort() int {
	settings := loadSurfSettingsOrDefault()
	if settings.ExistingSession.ChromeCDPPort > 0 {
		return settings.ExistingSession.ChromeCDPPort
	}
	return defaultHostCDPPort
}

func defaultSessionAttachTimeout() time.Duration {
	settings := loadSurfSettingsOrDefault()
	seconds := settings.ExistingSession.AttachTimeoutSec
	if seconds <= 0 {
		seconds = 8
	}
	return time.Duration(seconds) * time.Second
}

func defaultSessionActionTimeout() time.Duration {
	settings := loadSurfSettingsOrDefault()
	seconds := settings.ExistingSession.ActionTimeoutSec
	if seconds <= 0 {
		seconds = 15
	}
	return time.Duration(seconds) * time.Second
}

func normalizeSessionMode(raw string) (string, error) {
	mode := strings.ToLower(strings.TrimSpace(raw))
	switch mode {
	case "", sessionModeReadOnly:
		return sessionModeReadOnly, nil
	case sessionModeInteractive:
		return sessionModeInteractive, nil
	default:
		return "", fmt.Errorf("invalid session mode %q (expected read_only|interactive)", raw)
	}
}

func sessionStateDir() string {
	return filepath.Join(surfStateDir(), "browser", "sessions")
}

func sessionStatePath(name string) string {
	return filepath.Join(sessionStateDir(), sanitizeProfileName(name)+".json")
}

func writeAttachedSession(session attachedSession) error {
	if strings.TrimSpace(session.Session) == "" {
		return errors.New("session name is required")
	}
	if strings.TrimSpace(session.WSURL) == "" {
		return errors.New("target websocket URL is required")
	}
	if err := os.MkdirAll(sessionStateDir(), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(session, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(sessionStatePath(session.Session), data, 0o600)
}

func readAttachedSession(name string) (attachedSession, error) {
	path := sessionStatePath(name)
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return attachedSession{}, fmt.Errorf("session not found: %s", sanitizeProfileName(name))
		}
		return attachedSession{}, err
	}
	var session attachedSession
	if err := json.Unmarshal(data, &session); err != nil {
		return attachedSession{}, err
	}
	if strings.TrimSpace(session.Session) == "" {
		session.Session = sanitizeProfileName(name)
	}
	if strings.TrimSpace(session.Mode) == "" {
		session.Mode = sessionModeReadOnly
	}
	return session, nil
}

func listAttachedSessions() ([]attachedSession, error) {
	entries, err := os.ReadDir(sessionStateDir())
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	items := make([]attachedSession, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(strings.ToLower(entry.Name()), ".json") {
			continue
		}
		name := strings.TrimSuffix(entry.Name(), filepath.Ext(entry.Name()))
		session, err := readAttachedSession(name)
		if err != nil {
			continue
		}
		items = append(items, session)
	}
	sort.SliceStable(items, func(i, j int) bool {
		return items[i].Session < items[j].Session
	})
	return items, nil
}

func deleteAttachedSession(name string) error {
	path := sessionStatePath(name)
	if err := os.Remove(path); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("session not found: %s", sanitizeProfileName(name))
		}
		return err
	}
	return nil
}

func chooseChromeTarget(targets []chromeTarget, id, urlContains, titleContains string) (chromeTarget, error) {
	if len(targets) == 0 {
		return chromeTarget{}, errors.New("no attachable browser targets discovered")
	}
	if id != "" {
		for _, target := range targets {
			if strings.EqualFold(strings.TrimSpace(target.ID), id) {
				return target, nil
			}
		}
		return chromeTarget{}, fmt.Errorf("target id not found: %s", id)
	}
	if urlContains != "" || titleContains != "" {
		for _, target := range targets {
			urlOK := urlContains == "" || strings.Contains(strings.ToLower(target.URL), strings.ToLower(urlContains))
			titleOK := titleContains == "" || strings.Contains(strings.ToLower(target.Title), strings.ToLower(titleContains))
			if urlOK && titleOK {
				return target, nil
			}
		}
		return chromeTarget{}, errors.New("no target matched --url-contains/--title-contains")
	}
	if len(targets) == 1 {
		return targets[0], nil
	}
	return chromeTarget{}, errors.New("multiple targets discovered; provide --id or filter with --url-contains/--title-contains")
}

func discoverChromeTargets(host string, cdpPort int, timeout time.Duration) ([]chromeTarget, error) {
	resolvedHost := strings.TrimSpace(host)
	if resolvedHost == "" {
		resolvedHost = "127.0.0.1"
	}
	if cdpPort <= 0 {
		cdpPort = defaultHostCDPPort
	}
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	endpoint := fmt.Sprintf("http://%s:%d/json/list", resolvedHost, cdpPort)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("discover chrome targets from %s: %w", endpoint, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("discover chrome targets failed: status=%d", resp.StatusCode)
	}
	var targets []chromeTarget
	if err := json.NewDecoder(resp.Body).Decode(&targets); err != nil {
		return nil, err
	}
	filtered := make([]chromeTarget, 0, len(targets))
	for _, target := range targets {
		if strings.TrimSpace(target.WebSocketDebuggerURL) == "" {
			continue
		}
		if strings.TrimSpace(target.Type) != "" && strings.TrimSpace(target.Type) != "page" {
			continue
		}
		filtered = append(filtered, target)
	}
	return filtered, nil
}

func runSessionAction(session attachedSession, req sessionActionRequest) (sessionActionResult, error) {
	action := strings.ToLower(strings.TrimSpace(req.Action))
	if action == "" {
		return sessionActionResult{}, errors.New("action is required")
	}
	if session.Browser != "chrome" {
		return sessionActionResult{}, fmt.Errorf("browser %q is not supported for session actions yet", session.Browser)
	}
	if session.Mode == "" {
		session.Mode = sessionModeReadOnly
	}
	if session.Mode == sessionModeReadOnly {
		switch action {
		case "click", "type":
			return sessionActionResult{}, fmt.Errorf("action %q requires interactive mode (session is read_only)", action)
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), defaultSessionActionTimeout())
	defer cancel()
	conn, err := dialSessionCDP(ctx, session)
	if err != nil {
		return sessionActionResult{}, err
	}
	defer conn.Close(websocket.StatusNormalClosure, "done")

	nextID := int64(1)
	call := func(method string, params any) (json.RawMessage, error) {
		resp, err := cdpCall(ctx, conn, nextID, method, params)
		nextID++
		return resp, err
	}

	result := sessionActionResult{Action: action, Session: session.Session, Mode: session.Mode}
	switch action {
	case "title":
		value, err := cdpEvalValue(ctx, call, "document.title")
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "url":
		value, err := cdpEvalValue(ctx, call, "location.href")
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "text":
		value, err := cdpEvalValue(ctx, call, "document.body ? document.body.innerText : ''")
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "eval":
		if strings.TrimSpace(req.Expr) == "" {
			return sessionActionResult{}, errors.New("--expr is required for eval action")
		}
		value, err := cdpEvalValue(ctx, call, req.Expr)
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "click":
		if strings.TrimSpace(req.Selector) == "" {
			return sessionActionResult{}, errors.New("--selector is required for click action")
		}
		expr := fmt.Sprintf(`(() => {
  const el = document.querySelector(%s);
  if (!el) return { ok: false, error: "selector not found" };
  el.click();
  return { ok: true };
})()`, jsStringLiteral(req.Selector))
		value, err := cdpEvalValue(ctx, call, expr)
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "type":
		if strings.TrimSpace(req.Selector) == "" {
			return sessionActionResult{}, errors.New("--selector is required for type action")
		}
		expr := fmt.Sprintf(`(() => {
  const el = document.querySelector(%s);
  if (!el) return { ok: false, error: "selector not found" };
  if (!("value" in el)) return { ok: false, error: "target is not a value element" };
  el.focus();
  el.value = %s;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return { ok: true, length: ("" + el.value).length };
})()`, jsStringLiteral(req.Selector), jsStringLiteral(req.Text))
		value, err := cdpEvalValue(ctx, call, expr)
		if err != nil {
			return sessionActionResult{}, err
		}
		result.Value = value
		return result, nil
	case "screenshot":
		if _, err := call("Page.enable", map[string]any{}); err != nil {
			return sessionActionResult{}, err
		}
		raw, err := call("Page.captureScreenshot", map[string]any{"format": "png"})
		if err != nil {
			return sessionActionResult{}, err
		}
		var payload struct {
			Data string `json:"data"`
		}
		if err := json.Unmarshal(raw, &payload); err != nil {
			return sessionActionResult{}, err
		}
		if strings.TrimSpace(payload.Data) == "" {
			return sessionActionResult{}, errors.New("capture screenshot returned empty payload")
		}
		bin, err := base64.StdEncoding.DecodeString(payload.Data)
		if err != nil {
			return sessionActionResult{}, err
		}
		output := strings.TrimSpace(req.Out)
		if output == "" {
			output = filepath.Join(sessionStateDir(), fmt.Sprintf("%s-%d.png", sanitizeProfileName(session.Session), time.Now().Unix()))
		}
		output = filepath.Clean(expandTilde(output))
		if err := os.MkdirAll(filepath.Dir(output), 0o755); err != nil {
			return sessionActionResult{}, err
		}
		if err := os.WriteFile(output, bin, 0o644); err != nil {
			return sessionActionResult{}, err
		}
		result.Output = output
		return result, nil
	default:
		return sessionActionResult{}, fmt.Errorf("unsupported action %q", action)
	}
}

func dialSessionCDP(ctx context.Context, session attachedSession) (*websocket.Conn, error) {
	wsURL := strings.TrimSpace(session.WSURL)
	if wsURL == "" {
		if strings.TrimSpace(session.TargetID) == "" || strings.TrimSpace(session.CDPHost) == "" || session.CDPPort <= 0 {
			return nil, errors.New("session missing websocket debugger URL and endpoint details")
		}
		wsURL = fmt.Sprintf("ws://%s:%d/devtools/page/%s", session.CDPHost, session.CDPPort, session.TargetID)
	}
	parsed, err := url.Parse(wsURL)
	if err != nil {
		return nil, err
	}
	if parsed.Scheme != "ws" && parsed.Scheme != "wss" {
		return nil, fmt.Errorf("invalid websocket scheme in %s", wsURL)
	}
	conn, _, err := websocket.Dial(ctx, wsURL, nil)
	if err != nil {
		return nil, fmt.Errorf("connect to browser target: %w", err)
	}
	return conn, nil
}

func cdpCall(ctx context.Context, conn *websocket.Conn, id int64, method string, params any) (json.RawMessage, error) {
	req := cdpRequest{ID: id, Method: method, Params: params}
	if err := wsjson.Write(ctx, conn, req); err != nil {
		return nil, err
	}
	for {
		var resp cdpResponse
		if err := wsjson.Read(ctx, conn, &resp); err != nil {
			return nil, err
		}
		if resp.ID == 0 {
			continue
		}
		if resp.ID != id {
			continue
		}
		if resp.Error != nil {
			return nil, fmt.Errorf("cdp %s failed (%d): %s", method, resp.Error.Code, resp.Error.Message)
		}
		return resp.Result, nil
	}
}

func cdpEvalValue(ctx context.Context, call func(string, any) (json.RawMessage, error), expr string) (any, error) {
	raw, err := call("Runtime.evaluate", map[string]any{
		"expression":    expr,
		"returnByValue": true,
		"awaitPromise":  true,
	})
	if err != nil {
		return nil, err
	}
	var payload struct {
		Result struct {
			Type        string `json:"type"`
			Value       any    `json:"value"`
			Description string `json:"description"`
		} `json:"result"`
		ExceptionDetails map[string]any `json:"exceptionDetails"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return nil, err
	}
	if len(payload.ExceptionDetails) > 0 {
		return nil, fmt.Errorf("javascript evaluation failed: %v", payload.ExceptionDetails)
	}
	if payload.Result.Value != nil {
		return payload.Result.Value, nil
	}
	return payload.Result.Description, nil
}

func jsStringLiteral(raw string) string {
	data, _ := json.Marshal(raw)
	return string(data)
}

func splitHostPortFromURL(rawURL string) (string, int, error) {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return "", 0, err
	}
	host := parsed.Hostname()
	portStr := parsed.Port()
	if host == "" || portStr == "" {
		return "", 0, fmt.Errorf("url missing host/port: %s", rawURL)
	}
	port, err := net.LookupPort("tcp", portStr)
	if err != nil {
		return "", 0, err
	}
	return host, port, nil
}

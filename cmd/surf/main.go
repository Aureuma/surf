package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/pelletier/go-toml/v2"
)

const usageText = `surf <command> [args]

Commands:
  build        Build browser runtime image
  start        Start browser runtime container
  stop         Stop/remove runtime container
  status       Check runtime health
  logs         Stream runtime logs
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
  surf host start --profile work
  surf tunnel start --mode quick
  surf tunnel start --mode token --vault-key SURF_CLOUDFLARE_TUNNEL_TOKEN
  surf extension install
`

const (
	defaultImage          = "ghcr.io/aureuma/surf-browser:local"
	defaultContainer      = "surf-playwright-mcp-headed"
	defaultNetwork        = "si"
	defaultHostBind       = "127.0.0.1"
	defaultMCPPort        = 8931
	defaultHostMCPPort    = 8932
	defaultNoVNCPort      = 6080
	defaultHostNoVNCPort  = 6080
	defaultMCPVersion     = "0.0.64"
	defaultProfileName    = "default"
	defaultHostCDPPort    = 18800
	defaultTunnelName     = "surf-cloudflared"
	defaultCloudflaredImg = "cloudflare/cloudflared:latest"
	profileVolumePrefix   = "volume:"
)

type browserConfig struct {
	ImageName      string `json:"image_name"`
	ContainerName  string `json:"container_name"`
	Network        string `json:"network"`
	ProfileName    string `json:"profile_name"`
	ProfileDir     string `json:"profile_dir"`
	HostBind       string `json:"host_bind"`
	HostMCPPort    int    `json:"host_mcp_port"`
	HostNoVNCPort  int    `json:"host_novnc_port"`
	MCPPort        int    `json:"mcp_port"`
	NoVNCPort      int    `json:"novnc_port"`
	VNCPassword    string `json:"vnc_password,omitempty"`
	MCPVersion     string `json:"mcp_version"`
	BrowserChannel string `json:"browser_channel"`
	AllowedHosts   string `json:"allowed_hosts"`
}

type statusPayload struct {
	OK                  bool   `json:"ok"`
	ContainerName       string `json:"container_name"`
	ContainerRunning    bool   `json:"container_running"`
	ContainerStatusLine string `json:"container_status_line,omitempty"`
	MCPURL              string `json:"mcp_url"`
	NoVNCURL            string `json:"novnc_url"`
	MCPHostCode         int    `json:"mcp_host_code,omitempty"`
	MCPContainerCode    int    `json:"mcp_container_code,omitempty"`
	NoVNCHostCode       int    `json:"novnc_host_code,omitempty"`
	NoVNCContainerCode  int    `json:"novnc_container_code,omitempty"`
	MCPReady            bool   `json:"mcp_ready"`
	NoVNCReady          bool   `json:"novnc_ready"`
	Error               string `json:"error,omitempty"`
}

type tunnelStatusPayload struct {
	OK            bool   `json:"ok"`
	ContainerName string `json:"container_name"`
	Running       bool   `json:"running"`
	URL           string `json:"url,omitempty"`
	Mode          string `json:"mode,omitempty"`
	Error         string `json:"error,omitempty"`
}

type hostProcessState struct {
	Profile     string `json:"profile"`
	PID         int    `json:"pid"`
	BrowserPath string `json:"browser_path"`
	CDPPort     int    `json:"cdp_port"`
	ProfileDir  string `json:"profile_dir"`
	StartedAt   string `json:"started_at"`
	LogFile     string `json:"log_file"`
}

func main() {
	if len(os.Args) < 2 {
		// Best-effort bootstrap so a standalone surf install immediately has
		// ~/.si/surf/settings.toml available for future configuration.
		_, _ = loadSurfSettings()
		fmt.Print(usageText)
		os.Exit(1)
	}
	cmd := strings.ToLower(strings.TrimSpace(os.Args[1]))
	args := os.Args[2:]

	if cmd != "help" && cmd != "-h" && cmd != "--help" && cmd != "version" && cmd != "--version" && cmd != "-v" {
		if _, err := loadSurfSettings(); err != nil {
			fatal(err)
		}
	}

	switch cmd {
	case "help", "-h", "--help":
		fmt.Print(usageText)
	case "version", "--version", "-v":
		fmt.Println(surfVersion)
	case "config":
		cmdConfig(args)
	case "build":
		cmdBuild(args)
	case "start":
		cmdStart(args)
	case "stop":
		cmdStop(args)
	case "status":
		cmdStatus(args)
	case "logs":
		cmdLogs(args)
	case "proxy":
		cmdProxy(args)
	case "host":
		cmdHost(args)
	case "tunnel":
		cmdTunnel(args)
	case "extension":
		cmdExtension(args)
	default:
		fatal(fmt.Errorf("unknown command: %s", cmd))
	}
}

func defaultConfig() browserConfig {
	settings := loadSurfSettingsOrDefault()
	profile := envOr("SURF_PROFILE", strings.TrimSpace(settings.Browser.ProfileName))
	if profile == "" {
		profile = defaultProfileName
	}
	vncPassword := strings.TrimSpace(os.Getenv("SURF_VNC_PASSWORD"))
	if vncPassword == "" {
		vncPassword = strings.TrimSpace(settings.Browser.VNCPassword)
	}
	if vncPassword == "" {
		vncPassword = "surf"
	}
	profileDir := envOr("SURF_PROFILE_DIR", strings.TrimSpace(settings.Browser.ProfileDir))
	if profileDir == "" {
		profileDir = containerProfileDir(profile)
	}
	return browserConfig{
		ImageName:      envOr("SURF_IMAGE", firstNonEmpty(strings.TrimSpace(settings.Browser.ImageName), defaultImage)),
		ContainerName:  envOr("SURF_CONTAINER", firstNonEmpty(strings.TrimSpace(settings.Browser.ContainerName), defaultContainer)),
		Network:        envOr("SURF_NETWORK", firstNonEmpty(strings.TrimSpace(settings.Browser.Network), defaultNetwork)),
		ProfileName:    profile,
		ProfileDir:     profileDir,
		HostBind:       envOr("SURF_HOST_BIND", firstNonEmpty(strings.TrimSpace(settings.Browser.HostBind), defaultHostBind)),
		HostMCPPort:    envOrInt("SURF_HOST_MCP_PORT", intOrFallback(settings.Browser.HostMCPPort, defaultHostMCPPort)),
		HostNoVNCPort:  envOrInt("SURF_HOST_NOVNC_PORT", intOrFallback(settings.Browser.HostNoVNCPort, defaultHostNoVNCPort)),
		MCPPort:        envOrInt("SURF_MCP_PORT", intOrFallback(settings.Browser.MCPPort, defaultMCPPort)),
		NoVNCPort:      envOrInt("SURF_NOVNC_PORT", intOrFallback(settings.Browser.NoVNCPort, defaultNoVNCPort)),
		VNCPassword:    vncPassword,
		MCPVersion:     envOr("SURF_MCP_VERSION", firstNonEmpty(strings.TrimSpace(settings.Browser.MCPVersion), defaultMCPVersion)),
		BrowserChannel: envOr("SURF_BROWSER_CHANNEL", firstNonEmpty(strings.TrimSpace(settings.Browser.BrowserChannel), "chromium")),
		AllowedHosts:   envOr("SURF_ALLOWED_HOSTS", firstNonEmpty(strings.TrimSpace(settings.Browser.AllowedHosts), "*")),
	}
}

func cmdBuild(args []string) {
	fs := flag.NewFlagSet("build", flag.ExitOnError)
	cfg := defaultConfig()
	repo := fs.String("repo", "", "surf repository root path")
	contextDir := fs.String("context", "", "docker build context path")
	dockerfile := fs.String("dockerfile", "", "dockerfile path")
	image := fs.String("image", cfg.ImageName, "docker image name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf build [--image <name>] [--repo <path>] [--context <path>] [--dockerfile <path>] [--json]"))
	}
	mustHaveCommand("docker")

	cfg.ImageName = strings.TrimSpace(*image)
	assetsRoot, err := resolveAssetsRoot(strings.TrimSpace(*repo))
	if err != nil {
		fatal(err)
	}

	resolvedContext := strings.TrimSpace(*contextDir)
	if resolvedContext == "" {
		resolvedContext = assetsRoot
	}
	resolvedDockerfile := strings.TrimSpace(*dockerfile)
	if resolvedDockerfile == "" {
		resolvedDockerfile = filepath.Join(assetsRoot, "Dockerfile")
	}

	cmd := exec.Command("docker", "build", "-t", cfg.ImageName, "-f", resolvedDockerfile, resolvedContext)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	if err := cmd.Run(); err != nil {
		fatal(err)
	}

	if *jsonOut {
		printJSON(map[string]any{
			"ok":         true,
			"command":    "build",
			"image":      cfg.ImageName,
			"dockerfile": resolvedDockerfile,
			"context":    resolvedContext,
		})
		return
	}
	fmt.Printf("surf build: image built %s\n", cfg.ImageName)
}

func cmdStart(args []string) {
	fs := flag.NewFlagSet("start", flag.ExitOnError)
	cfg := defaultConfig()
	repo := fs.String("repo", "", "surf repository root path")
	skipBuild := fs.Bool("skip-build", false, "skip docker image build")
	jsonOut := fs.Bool("json", false, "output json")
	registerConfigFlags(fs, &cfg)
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf start [--skip-build] [--repo <path>] [--image <name>] [--name <container>] [--network <name>] [--profile <name>] [--profile-dir <path>] [--host-bind <addr>] [--host-mcp-port <n>] [--host-novnc-port <n>] [--mcp-port <n>] [--novnc-port <n>] [--vnc-password <pwd>] [--mcp-version <ver>] [--browser <name>] [--allowed-hosts <list>] [--json]"))
	}
	mustHaveCommand("docker")
	applyContainerProfileDefault(fs, &cfg)
	profileMount, profileHostPath, bindMount := resolveProfileMount(cfg.ProfileDir)

	if bindMount {
		if err := os.MkdirAll(profileHostPath, 0o700); err != nil {
			fatal(err)
		}
	}
	if !*skipBuild {
		cmdBuild([]string{"--repo", strings.TrimSpace(*repo), "--image", cfg.ImageName})
	}
	if err := removeDockerContainer(cfg.ContainerName); err != nil {
		fatal(err)
	}
	if err := ensureDockerNetwork(cfg.Network); err != nil {
		fatal(err)
	}

	runArgs := []string{
		"run", "-d",
		"--name", cfg.ContainerName,
		"--restart", "unless-stopped",
		"--init",
		"--ipc=host",
		"--network", cfg.Network,
		"--user", "pwuser",
		"-e", "VNC_PASSWORD=" + cfg.VNCPassword,
		"-e", "MCP_VERSION=" + cfg.MCPVersion,
		"-e", "BROWSER_CHANNEL=" + cfg.BrowserChannel,
		"-e", "ALLOWED_HOSTS=" + cfg.AllowedHosts,
		"-e", "MCP_PORT=" + strconv.Itoa(cfg.MCPPort),
		"-e", "NOVNC_PORT=" + strconv.Itoa(cfg.NoVNCPort),
		"-p", fmt.Sprintf("%s:%d:%d", cfg.HostBind, cfg.HostMCPPort, cfg.MCPPort),
		"-p", fmt.Sprintf("%s:%d:%d", cfg.HostBind, cfg.HostNoVNCPort, cfg.NoVNCPort),
		"-v", profileMount,
		cfg.ImageName,
	}
	if _, err := runDockerOutput(runArgs...); err != nil {
		fatal(fmt.Errorf("docker run failed: %w", err))
	}
	status, err := waitForStatus(cfg, 15, time.Second)
	if err != nil {
		fatal(err)
	}

	if *jsonOut {
		printJSON(map[string]any{
			"ok":             true,
			"command":        "start",
			"config":         cfg,
			"status":         status,
			"mcp_url":        mcpURL(cfg),
			"novnc_url":      novncURL(cfg),
			"container_name": cfg.ContainerName,
		})
		return
	}
	fmt.Printf("surf start\n")
	fmt.Printf("  container=%s image=%s network=%s\n", cfg.ContainerName, cfg.ImageName, cfg.Network)
	fmt.Printf("  profile=%s profile_dir=%s\n", cfg.ProfileName, cfg.ProfileDir)
	fmt.Printf("  mcp_url=%s\n", mcpURL(cfg))
	fmt.Printf("  novnc_url=%s\n", novncURL(cfg))
}

func cmdStop(args []string) {
	fs := flag.NewFlagSet("stop", flag.ExitOnError)
	cfg := defaultConfig()
	jsonOut := fs.Bool("json", false, "output json")
	fs.StringVar(&cfg.ContainerName, "name", cfg.ContainerName, "container name")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf stop [--name <container>] [--json]"))
	}
	mustHaveCommand("docker")

	if err := removeDockerContainer(cfg.ContainerName); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "command": "stop", "container_name": cfg.ContainerName})
		return
	}
	fmt.Printf("surf stop: removed %s\n", cfg.ContainerName)
}

func cmdStatus(args []string) {
	fs := flag.NewFlagSet("status", flag.ExitOnError)
	cfg := defaultConfig()
	jsonOut := fs.Bool("json", false, "output json")
	registerConfigFlags(fs, &cfg)
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf status [--name <container>] [--profile <name>] [--profile-dir <path>] [--json]"))
	}
	mustHaveCommand("docker")
	applyContainerProfileDefault(fs, &cfg)

	status, err := evaluateStatus(cfg)
	if err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": status.OK, "command": "status", "status": status})
		if !status.OK {
			os.Exit(1)
		}
		return
	}
	fmt.Printf("surf status\n")
	fmt.Printf("  container=%s running=%t\n", status.ContainerName, status.ContainerRunning)
	fmt.Printf("  profile=%s profile_dir=%s\n", cfg.ProfileName, cfg.ProfileDir)
	if strings.TrimSpace(status.ContainerStatusLine) != "" {
		fmt.Printf("  docker_ps=%s\n", status.ContainerStatusLine)
	}
	fmt.Printf("  mcp_url=%s\n", status.MCPURL)
	fmt.Printf("  novnc_url=%s\n", status.NoVNCURL)
	fmt.Printf("  mcp_ready=%t host_code=%d container_code=%d\n", status.MCPReady, status.MCPHostCode, status.MCPContainerCode)
	fmt.Printf("  novnc_ready=%t host_code=%d container_code=%d\n", status.NoVNCReady, status.NoVNCHostCode, status.NoVNCContainerCode)
	if strings.TrimSpace(status.Error) != "" {
		fmt.Printf("  error=%s\n", status.Error)
	}
	if !status.OK {
		os.Exit(1)
	}
}

func cmdLogs(args []string) {
	fs := flag.NewFlagSet("logs", flag.ExitOnError)
	cfg := defaultConfig()
	tail := fs.Int("tail", 200, "tail line count")
	follow := fs.Bool("follow", true, "follow logs")
	fs.StringVar(&cfg.ContainerName, "name", cfg.ContainerName, "container name")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf logs [--name <container>] [--tail <n>] [--follow] [--follow=false]"))
	}
	mustHaveCommand("docker")

	logArgs := []string{"logs", "--tail", strconv.Itoa(*tail)}
	if *follow {
		logArgs = append(logArgs, "-f")
	}
	logArgs = append(logArgs, cfg.ContainerName)
	cmd := exec.Command("docker", logArgs...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	if err := cmd.Run(); err != nil {
		fatal(err)
	}
}

func cmdProxy(args []string) {
	fs := flag.NewFlagSet("proxy", flag.ExitOnError)
	bind := fs.String("bind", "127.0.0.1", "proxy bind host")
	port := fs.Int("port", 8931, "proxy bind port")
	upstream := fs.String("upstream", "http://127.0.0.1:8932", "upstream MCP base URL")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf proxy [--bind <host>] [--port <n>] [--upstream <url>]"))
	}

	upstreamURL, err := url.Parse(strings.TrimSpace(*upstream))
	if err != nil {
		fatal(fmt.Errorf("invalid --upstream: %w", err))
	}
	proxy := httputil.NewSingleHostReverseProxy(upstreamURL)
	baseDirector := proxy.Director
	proxy.Director = func(req *http.Request) {
		baseDirector(req)
		if req.Method != http.MethodGet {
			return
		}
		if req.URL.Path == "/mcp" {
			req.URL.Path = "/sse"
			return
		}
		if strings.HasPrefix(req.URL.Path, "/mcp/") {
			req.URL.Path = "/sse" + strings.TrimPrefix(req.URL.Path, "/mcp")
		}
	}
	proxy.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = io.WriteString(w, "proxy error: "+err.Error())
	}

	addr := fmt.Sprintf("%s:%d", strings.TrimSpace(*bind), *port)
	fmt.Printf("surf proxy: listening on http://%s -> %s\n", addr, upstreamURL.String())
	if err := http.ListenAndServe(addr, proxy); err != nil {
		fatal(err)
	}
}

func cmdHost(args []string) {
	if len(args) == 0 {
		fatal(errors.New("usage: surf host <start|stop|status|logs> [args]"))
	}
	sub := strings.ToLower(strings.TrimSpace(args[0]))
	rest := args[1:]
	switch sub {
	case "start":
		cmdHostStart(rest)
	case "stop":
		cmdHostStop(rest)
	case "status":
		cmdHostStatus(rest)
	case "logs":
		cmdHostLogs(rest)
	default:
		fatal(fmt.Errorf("unknown host command: %s", sub))
	}
}

func cmdHostStart(args []string) {
	if runtime.GOOS != "linux" && runtime.GOOS != "darwin" {
		fatal(fmt.Errorf("host browser mode is only supported on linux and darwin"))
	}
	fs := flag.NewFlagSet("host start", flag.ExitOnError)
	profile := fs.String("profile", defaultHostProfileName(), "host browser profile name")
	profileDir := fs.String("profile-dir", "", "host browser profile directory")
	browserPath := fs.String("browser-path", envOr("SURF_HOST_BROWSER_PATH", ""), "browser executable path")
	cdpPort := fs.Int("cdp-port", envOrInt("SURF_HOST_CDP_PORT", defaultHostCDPPort), "CDP port")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf host start [--profile <name>] [--profile-dir <path>] [--browser-path <path>] [--cdp-port <n>] [--json]"))
	}

	pname := sanitizeProfileName(*profile)
	pdir := strings.TrimSpace(*profileDir)
	if pdir == "" {
		pdir = hostProfileDir(pname)
	}
	if err := os.MkdirAll(pdir, 0o700); err != nil {
		fatal(err)
	}
	if pid, _ := readHostPID(pname); pid > 0 && processAlive(pid) {
		fatal(fmt.Errorf("host browser profile %s already running (pid=%d)", pname, pid))
	}
	bin := strings.TrimSpace(*browserPath)
	if bin == "" {
		resolved, err := detectHostBrowserBinary()
		if err != nil {
			fatal(err)
		}
		bin = resolved
	}
	if _, err := os.Stat(bin); err != nil {
		fatal(fmt.Errorf("browser binary not found: %s", bin))
	}

	logPath := hostLogPath(pname)
	if err := os.MkdirAll(filepath.Dir(logPath), 0o755); err != nil {
		fatal(err)
	}
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		fatal(err)
	}
	defer logFile.Close()

	launchArgs := hostLaunchArgs(*cdpPort, pdir, os.Geteuid() == 0, os.Getenv("DISPLAY"))
	cmd := exec.Command(bin, launchArgs...)
	cmd.Stdout = logFile
	cmd.Stderr = logFile
	cmd.Stdin = nil
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		fatal(err)
	}
	pid := cmd.Process.Pid
	_ = cmd.Process.Release()

	state := hostProcessState{
		Profile:     pname,
		PID:         pid,
		BrowserPath: bin,
		CDPPort:     *cdpPort,
		ProfileDir:  pdir,
		StartedAt:   time.Now().UTC().Format(time.RFC3339),
		LogFile:     logPath,
	}
	if err := writeHostState(state); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "state": state, "cdp_url": fmt.Sprintf("http://127.0.0.1:%d", *cdpPort)})
		return
	}
	fmt.Printf("surf host start\n")
	fmt.Printf("  profile=%s pid=%d\n", pname, pid)
	fmt.Printf("  browser=%s\n", bin)
	fmt.Printf("  profile_dir=%s\n", pdir)
	fmt.Printf("  cdp_url=http://127.0.0.1:%d\n", *cdpPort)
	fmt.Printf("  log=%s\n", logPath)
}

func hostLaunchArgs(cdpPort int, profileDir string, isRoot bool, display string) []string {
	args := []string{
		"--remote-debugging-port=" + strconv.Itoa(cdpPort),
		"--user-data-dir=" + profileDir,
		"--no-first-run",
		"--no-default-browser-check",
	}
	if runtime.GOOS == "linux" && isRoot {
		args = append(args, "--no-sandbox", "--disable-setuid-sandbox")
	}
	if strings.TrimSpace(display) == "" {
		args = append(args, "--headless=new")
	}
	args = append(args, "about:blank")
	return args
}

func cmdHostStop(args []string) {
	fs := flag.NewFlagSet("host stop", flag.ExitOnError)
	profile := fs.String("profile", defaultHostProfileName(), "host browser profile name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf host stop [--profile <name>] [--json]"))
	}
	pname := sanitizeProfileName(*profile)
	state, err := readHostState(pname)
	if err != nil {
		fatal(err)
	}
	if state.PID <= 0 {
		fatal(fmt.Errorf("invalid pid for profile %s", pname))
	}
	_ = syscall.Kill(state.PID, syscall.SIGTERM)
	for i := 0; i < 20; i++ {
		if !processAlive(state.PID) {
			break
		}
		time.Sleep(100 * time.Millisecond)
	}
	if processAlive(state.PID) {
		_ = syscall.Kill(state.PID, syscall.SIGKILL)
	}
	_ = os.Remove(hostStatePath(pname))
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "profile": pname})
		return
	}
	fmt.Printf("surf host stop: profile=%s pid=%d\n", pname, state.PID)
}

func cmdHostStatus(args []string) {
	fs := flag.NewFlagSet("host status", flag.ExitOnError)
	profile := fs.String("profile", defaultHostProfileName(), "host browser profile name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf host status [--profile <name>] [--json]"))
	}
	pname := sanitizeProfileName(*profile)
	state, err := readHostState(pname)
	if err != nil {
		if os.IsNotExist(err) {
			if *jsonOut {
				printJSON(map[string]any{"ok": false, "profile": pname, "error": "not running"})
				os.Exit(1)
			}
			fatal(fmt.Errorf("host profile %s is not running", pname))
		}
		fatal(err)
	}
	alive := processAlive(state.PID)
	cdpURL := fmt.Sprintf("http://127.0.0.1:%d/json/version", state.CDPPort)
	cdpCode := probeHTTPStatus(cdpURL)
	ok := alive && cdpCode == 200
	payload := map[string]any{
		"ok":         ok,
		"profile":    pname,
		"pid":        state.PID,
		"alive":      alive,
		"cdp_port":   state.CDPPort,
		"cdp_status": cdpCode,
		"cdp_url":    cdpURL,
		"state":      state,
	}
	if *jsonOut {
		printJSON(payload)
		if !ok {
			os.Exit(1)
		}
		return
	}
	fmt.Printf("surf host status\n")
	fmt.Printf("  profile=%s pid=%d alive=%t\n", pname, state.PID, alive)
	fmt.Printf("  cdp_url=%s status=%d\n", cdpURL, cdpCode)
	fmt.Printf("  profile_dir=%s\n", state.ProfileDir)
	if !ok {
		os.Exit(1)
	}
}

func cmdHostLogs(args []string) {
	fs := flag.NewFlagSet("host logs", flag.ExitOnError)
	profile := fs.String("profile", defaultHostProfileName(), "host browser profile name")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf host logs [--profile <name>]"))
	}
	pname := sanitizeProfileName(*profile)
	state, err := readHostState(pname)
	if err != nil {
		fatal(err)
	}
	data, err := os.ReadFile(state.LogFile)
	if err != nil {
		fatal(err)
	}
	fmt.Print(string(data))
}

func cmdConfig(args []string) {
	if len(args) == 0 {
		fatal(errors.New("usage: surf config <show|set|path|init> [args]"))
	}
	sub := strings.ToLower(strings.TrimSpace(args[0]))
	rest := args[1:]
	switch sub {
	case "show", "get":
		cmdConfigShow(rest)
	case "set":
		cmdConfigSet(rest)
	case "path":
		cmdConfigPath(rest)
	case "init":
		cmdConfigInit(rest)
	default:
		fatal(fmt.Errorf("unknown config command: %s", sub))
	}
}

func cmdConfigShow(args []string) {
	fs := flag.NewFlagSet("config show", flag.ExitOnError)
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf config show [--json]"))
	}
	settings := loadSurfSettingsOrDefault()
	if *jsonOut {
		printJSON(settings)
		return
	}
	data, err := toml.Marshal(settings)
	if err != nil {
		fatal(err)
	}
	fmt.Print(string(data))
}

func cmdConfigSet(args []string) {
	fs := flag.NewFlagSet("config set", flag.ExitOnError)
	key := fs.String("key", "", "setting key (for example tunnel.mode)")
	value := fs.String("value", "", "setting value")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf config set --key <path> --value <value> [--json]"))
	}
	if strings.TrimSpace(*key) == "" {
		fatal(errors.New("--key is required"))
	}
	settings := loadSurfSettingsOrDefault()
	if err := setSurfConfigValue(&settings, *key, *value); err != nil {
		fatal(err)
	}
	if err := saveSurfSettings(settings); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{
			"ok":            true,
			"key":           strings.TrimSpace(*key),
			"settings_file": surfSettingsPath(),
		})
		return
	}
	fmt.Printf("surf config set: %s\n", strings.TrimSpace(*key))
}

func cmdConfigPath(args []string) {
	fs := flag.NewFlagSet("config path", flag.ExitOnError)
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf config path"))
	}
	fmt.Println(surfSettingsPath())
}

func cmdConfigInit(args []string) {
	fs := flag.NewFlagSet("config init", flag.ExitOnError)
	force := fs.Bool("force", false, "overwrite existing settings file with defaults")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf config init [--force] [--json]"))
	}
	path := surfSettingsPath()
	if !*force {
		if _, err := os.Stat(path); err == nil {
			if *jsonOut {
				printJSON(map[string]any{"ok": true, "settings_file": path, "created": false})
				return
			}
			fmt.Printf("surf config init: already exists at %s\n", path)
			return
		}
	}
	settings := defaultSurfSettings()
	applySurfSettingsDefaults(&settings)
	if err := saveSurfSettings(settings); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "settings_file": path, "created": true})
		return
	}
	fmt.Printf("surf config init: wrote %s\n", path)
}

func cmdTunnel(args []string) {
	if len(args) == 0 {
		fatal(errors.New("usage: surf tunnel <start|stop|status|logs> [args]"))
	}
	sub := strings.ToLower(strings.TrimSpace(args[0]))
	rest := args[1:]
	switch sub {
	case "start":
		cmdTunnelStart(rest)
	case "stop":
		cmdTunnelStop(rest)
	case "status":
		cmdTunnelStatus(rest)
	case "logs":
		cmdTunnelLogs(rest)
	default:
		fatal(fmt.Errorf("unknown tunnel command: %s", sub))
	}
}

func cmdTunnelStart(args []string) {
	fs := flag.NewFlagSet("tunnel start", flag.ExitOnError)
	cfg := defaultConfig()
	settings := loadSurfSettingsOrDefault()
	tunnelCfg := settings.Tunnel
	defaultTargetURL := strings.TrimSpace(tunnelCfg.TargetURL)
	if defaultTargetURL == "" {
		defaultTargetURL = novncURL(cfg)
	}
	name := fs.String("name", envOr("SURF_TUNNEL_NAME", firstNonEmpty(strings.TrimSpace(tunnelCfg.ContainerName), defaultTunnelName)), "container name")
	target := fs.String("target-url", envOr("SURF_TUNNEL_TARGET_URL", defaultTargetURL), "target URL to expose")
	mode := fs.String("mode", envOr("SURF_TUNNEL_MODE", firstNonEmpty(strings.TrimSpace(tunnelCfg.Mode), "quick")), "tunnel mode: quick|token")
	token := fs.String("token", "", "cloudflare tunnel token")
	vaultKey := fs.String("vault-key", envOr("SURF_TUNNEL_VAULT_KEY", strings.TrimSpace(tunnelCfg.VaultKey)), "si vault key containing cloudflare tunnel token")
	image := fs.String("image", envOr("SURF_CLOUDFLARED_IMAGE", firstNonEmpty(strings.TrimSpace(tunnelCfg.Image), defaultCloudflaredImg)), "cloudflared image")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf tunnel start [--name <container>] [--target-url <url>] [--mode quick|token] [--token <value>] [--vault-key <key>] [--image <name>] [--json]"))
	}
	mustHaveCommand("docker")

	resolvedMode := strings.ToLower(strings.TrimSpace(*mode))
	if resolvedMode != "quick" && resolvedMode != "token" {
		fatal(fmt.Errorf("invalid --mode %q (expected quick|token)", *mode))
	}

	_ = removeDockerContainer(*name)
	runArgs := []string{"run", "-d", "--name", strings.TrimSpace(*name), "--restart", "unless-stopped", strings.TrimSpace(*image), "tunnel", "--no-autoupdate"}
	if resolvedMode == "quick" {
		runArgs = append(runArgs, "--url", strings.TrimSpace(*target))
	} else {
		resolvedToken, err := resolveToken(strings.TrimSpace(*token), strings.TrimSpace(*vaultKey))
		if err != nil {
			fatal(err)
		}
		if resolvedToken == "" {
			fatal(errors.New("tunnel token mode requires token value (use --token, SURF_CLOUDFLARE_TUNNEL_TOKEN, or --vault-key)"))
		}
		runArgs = append(runArgs, "run", "--token", resolvedToken)
	}

	if _, err := runDockerOutput(runArgs...); err != nil {
		fatal(err)
	}
	url := extractTryCloudflareURL(fetchContainerLogs(*name, 200))
	payload := tunnelStatusPayload{OK: true, ContainerName: *name, Running: true, URL: url, Mode: resolvedMode}
	if *jsonOut {
		printJSON(payload)
		return
	}
	fmt.Printf("surf tunnel start\n")
	fmt.Printf("  container=%s mode=%s\n", *name, resolvedMode)
	if resolvedMode == "quick" {
		fmt.Printf("  target=%s\n", strings.TrimSpace(*target))
	}
	if strings.TrimSpace(url) != "" {
		fmt.Printf("  public_url=%s\n", url)
	} else {
		fmt.Printf("  public_url=pending (run `surf tunnel status`)\n")
	}
}

func cmdTunnelStop(args []string) {
	fs := flag.NewFlagSet("tunnel stop", flag.ExitOnError)
	name := fs.String("name", defaultTunnelName, "container name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf tunnel stop [--name <container>] [--json]"))
	}
	mustHaveCommand("docker")

	if err := removeDockerContainer(*name); err != nil {
		fatal(err)
	}
	if *jsonOut {
		printJSON(map[string]any{"ok": true, "container_name": *name})
		return
	}
	fmt.Printf("surf tunnel stop: removed %s\n", *name)
}

func cmdTunnelStatus(args []string) {
	fs := flag.NewFlagSet("tunnel status", flag.ExitOnError)
	name := fs.String("name", defaultTunnelName, "container name")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf tunnel status [--name <container>] [--json]"))
	}
	mustHaveCommand("docker")

	running, line, err := isContainerRunning(*name)
	if err != nil {
		fatal(err)
	}
	logs := fetchContainerLogs(*name, 200)
	url := extractTryCloudflareURL(logs)
	payload := tunnelStatusPayload{OK: running, ContainerName: *name, Running: running, URL: url}
	if !running {
		payload.Error = "tunnel container not running"
	}
	if *jsonOut {
		printJSON(payload)
		if !payload.OK {
			os.Exit(1)
		}
		return
	}
	fmt.Printf("surf tunnel status\n")
	fmt.Printf("  container=%s running=%t\n", *name, running)
	if strings.TrimSpace(line) != "" {
		fmt.Printf("  docker_ps=%s\n", line)
	}
	if strings.TrimSpace(url) != "" {
		fmt.Printf("  public_url=%s\n", url)
	}
	if !running {
		os.Exit(1)
	}
}

func cmdTunnelLogs(args []string) {
	fs := flag.NewFlagSet("tunnel logs", flag.ExitOnError)
	name := fs.String("name", defaultTunnelName, "container name")
	tail := fs.Int("tail", 200, "tail line count")
	follow := fs.Bool("follow", true, "follow logs")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf tunnel logs [--name <container>] [--tail <n>] [--follow] [--follow=false]"))
	}
	mustHaveCommand("docker")

	logArgs := []string{"logs", "--tail", strconv.Itoa(*tail)}
	if *follow {
		logArgs = append(logArgs, "-f")
	}
	logArgs = append(logArgs, strings.TrimSpace(*name))
	cmd := exec.Command("docker", logArgs...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	if err := cmd.Run(); err != nil {
		fatal(err)
	}
}

func cmdExtension(args []string) {
	if len(args) == 0 {
		fatal(errors.New("usage: surf extension <install|path|doctor> [args]"))
	}
	sub := strings.ToLower(strings.TrimSpace(args[0]))
	rest := args[1:]
	switch sub {
	case "install":
		cmdExtensionInstall(rest)
	case "path":
		cmdExtensionPath(rest)
	case "doctor":
		cmdExtensionDoctor(rest)
	default:
		fatal(fmt.Errorf("unknown extension command: %s", sub))
	}
}

func cmdExtensionInstall(args []string) {
	fs := flag.NewFlagSet("extension install", flag.ExitOnError)
	repo := fs.String("repo", "", "surf repository root path")
	dest := fs.String("dest", defaultExtensionInstallPath(), "destination directory")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf extension install [--repo <path>] [--dest <path>] [--json]"))
	}
	sourceRoot, err := resolveRepoRoot(strings.TrimSpace(*repo))
	if err != nil {
		fatal(err)
	}
	sourceDir := filepath.Join(sourceRoot, "extensions", "chrome")
	if _, err := os.Stat(filepath.Join(sourceDir, "manifest.json")); err != nil {
		fatal(fmt.Errorf("extension template missing at %s", sourceDir))
	}

	destination := expandTilde(strings.TrimSpace(*dest))
	if err := os.RemoveAll(destination); err != nil {
		fatal(err)
	}
	if err := copyDir(sourceDir, destination); err != nil {
		fatal(err)
	}

	if *jsonOut {
		printJSON(map[string]any{"ok": true, "path": destination})
		return
	}
	fmt.Printf("surf extension installed: %s\n", destination)
	fmt.Printf("Load unpacked in chrome://extensions\n")
}

func cmdExtensionPath(args []string) {
	fs := flag.NewFlagSet("extension path", flag.ExitOnError)
	repo := fs.String("repo", "", "surf repository root path")
	source := fs.Bool("source", false, "print source template path instead of installed path")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf extension path [--repo <path>] [--source]"))
	}
	if *source {
		root, err := resolveRepoRoot(strings.TrimSpace(*repo))
		if err != nil {
			fatal(err)
		}
		fmt.Println(filepath.Join(root, "extensions", "chrome"))
		return
	}
	fmt.Println(defaultExtensionInstallPath())
}

func cmdExtensionDoctor(args []string) {
	fs := flag.NewFlagSet("extension doctor", flag.ExitOnError)
	path := fs.String("path", defaultExtensionInstallPath(), "installed extension path")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf extension doctor [--path <path>] [--json]"))
	}
	manifest := filepath.Join(expandTilde(strings.TrimSpace(*path)), "manifest.json")
	_, err := os.Stat(manifest)
	ok := err == nil
	if *jsonOut {
		printJSON(map[string]any{"ok": ok, "path": strings.TrimSpace(*path), "manifest": manifest})
		if !ok {
			os.Exit(1)
		}
		return
	}
	if !ok {
		fatal(fmt.Errorf("extension not installed at %s (run `surf extension install`)", strings.TrimSpace(*path)))
	}
	fmt.Printf("surf extension doctor: ok\n")
	fmt.Printf("  path=%s\n", strings.TrimSpace(*path))
	fmt.Printf("  next=chrome://extensions -> Load unpacked -> %s\n", strings.TrimSpace(*path))
}

func registerConfigFlags(fs *flag.FlagSet, cfg *browserConfig) {
	fs.StringVar(&cfg.ImageName, "image", cfg.ImageName, "docker image name")
	fs.StringVar(&cfg.ContainerName, "name", cfg.ContainerName, "container name")
	fs.StringVar(&cfg.Network, "network", cfg.Network, "docker network name")
	fs.StringVar(&cfg.ProfileName, "profile", cfg.ProfileName, "browser profile name")
	fs.StringVar(&cfg.ProfileDir, "profile-dir", cfg.ProfileDir, "host profile directory")
	fs.StringVar(&cfg.HostBind, "host-bind", cfg.HostBind, "host bind address")
	fs.IntVar(&cfg.HostMCPPort, "host-mcp-port", cfg.HostMCPPort, "host MCP port")
	fs.IntVar(&cfg.HostNoVNCPort, "host-novnc-port", cfg.HostNoVNCPort, "host noVNC port")
	fs.IntVar(&cfg.MCPPort, "mcp-port", cfg.MCPPort, "container MCP port")
	fs.IntVar(&cfg.NoVNCPort, "novnc-port", cfg.NoVNCPort, "container noVNC port")
	fs.StringVar(&cfg.VNCPassword, "vnc-password", cfg.VNCPassword, "VNC password")
	fs.StringVar(&cfg.MCPVersion, "mcp-version", cfg.MCPVersion, "@playwright/mcp version")
	fs.StringVar(&cfg.BrowserChannel, "browser", cfg.BrowserChannel, "browser channel")
	fs.StringVar(&cfg.AllowedHosts, "allowed-hosts", cfg.AllowedHosts, "allowed hosts list")
}

func applyContainerProfileDefault(fs *flag.FlagSet, cfg *browserConfig) {
	cfg.ProfileName = sanitizeProfileName(cfg.ProfileName)
	if !flagPassed(fs, "profile-dir") && strings.TrimSpace(cfg.ProfileDir) == "" {
		cfg.ProfileDir = containerProfileDir(cfg.ProfileName)
	}
}

func flagPassed(fs *flag.FlagSet, name string) bool {
	found := false
	fs.Visit(func(f *flag.Flag) {
		if f.Name == name {
			found = true
		}
	})
	return found
}

func evaluateStatus(cfg browserConfig) (statusPayload, error) {
	status := statusPayload{
		ContainerName: cfg.ContainerName,
		MCPURL:        mcpURL(cfg),
		NoVNCURL:      novncURL(cfg),
	}
	psOut, err := runDockerOutput("ps", "--filter", "name=^/"+cfg.ContainerName+"$", "--format", "{{.Names}}\t{{.Status}}\t{{.Ports}}")
	if err != nil {
		return status, err
	}
	status.ContainerStatusLine = strings.TrimSpace(psOut)
	status.ContainerRunning = strings.TrimSpace(psOut) != ""
	if !status.ContainerRunning {
		status.Error = "container not running"
		return status, nil
	}

	status.MCPHostCode = probeHTTPStatus(status.MCPURL)
	if status.MCPHostCode == 200 || status.MCPHostCode == 400 {
		status.MCPReady = true
	}
	status.NoVNCHostCode = probeHTTPStatus(status.NoVNCURL)
	if status.NoVNCHostCode >= 200 && status.NoVNCHostCode < 400 {
		status.NoVNCReady = true
	}

	if !status.MCPReady {
		status.MCPContainerCode = probeContainerHTTPStatus(cfg.ContainerName, fmt.Sprintf("http://127.0.0.1:%d/mcp", cfg.MCPPort))
		if status.MCPContainerCode == 200 || status.MCPContainerCode == 400 {
			status.MCPReady = true
		}
	}
	if !status.NoVNCReady {
		status.NoVNCContainerCode = probeContainerHTTPStatus(cfg.ContainerName, fmt.Sprintf("http://127.0.0.1:%d/vnc.html", cfg.NoVNCPort))
		if status.NoVNCContainerCode >= 200 && status.NoVNCContainerCode < 400 {
			status.NoVNCReady = true
		}
	}

	status.OK = status.ContainerRunning && status.MCPReady && status.NoVNCReady
	if !status.OK {
		status.Error = "endpoint checks failed"
	}
	return status, nil
}

func waitForStatus(cfg browserConfig, retries int, delay time.Duration) (statusPayload, error) {
	var last statusPayload
	for i := 0; i < retries; i++ {
		status, err := evaluateStatus(cfg)
		if err != nil {
			return statusPayload{}, err
		}
		last = status
		if status.OK {
			return status, nil
		}
		time.Sleep(delay)
	}
	if strings.TrimSpace(last.Error) == "" {
		last.Error = "browser health checks did not pass in time"
	}
	return last, errors.New(last.Error)
}

func resolveAssetsRoot(repo string) (string, error) {
	root, err := resolveRepoRoot(repo)
	if err != nil {
		return "", err
	}
	assets := filepath.Join(root, "runtime", "browser")
	if _, err := os.Stat(filepath.Join(assets, "Dockerfile")); err != nil {
		return "", fmt.Errorf("browser assets not found at %s", assets)
	}
	return assets, nil
}

func resolveRepoRoot(repo string) (string, error) {
	if strings.TrimSpace(repo) != "" {
		resolved := filepath.Clean(expandTilde(repo))
		if _, err := os.Stat(resolved); err != nil {
			return "", err
		}
		return resolved, nil
	}
	if env := strings.TrimSpace(os.Getenv("SURF_REPO")); env != "" {
		resolved := filepath.Clean(expandTilde(env))
		if _, err := os.Stat(resolved); err == nil {
			return resolved, nil
		}
	}
	if cwd, err := os.Getwd(); err == nil && hasSurfLayout(cwd) {
		return filepath.Clean(cwd), nil
	}
	if exe, err := os.Executable(); err == nil {
		cand := filepath.Clean(filepath.Join(filepath.Dir(exe), "..", ".."))
		if hasSurfLayout(cand) {
			return cand, nil
		}
	}
	return "", errors.New("unable to locate surf repository root; pass --repo")
}

func hasSurfLayout(root string) bool {
	_, errA := os.Stat(filepath.Join(root, "runtime", "browser", "Dockerfile"))
	_, errB := os.Stat(filepath.Join(root, "cmd", "surf"))
	return errA == nil && errB == nil
}

func runDockerOutput(args ...string) (string, error) {
	cmd := exec.Command("docker", args...)
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = err.Error()
		}
		return strings.TrimSpace(stdout.String()), errors.New(msg)
	}
	return strings.TrimSpace(stdout.String()), nil
}

func ensureDockerNetwork(name string) error {
	resolved := strings.TrimSpace(name)
	if resolved == "" {
		return errors.New("network name is required")
	}
	if _, err := runDockerOutput("network", "inspect", resolved); err == nil {
		return nil
	}
	if _, err := runDockerOutput("network", "create", resolved); err != nil {
		return fmt.Errorf("ensure docker network %s: %w", resolved, err)
	}
	return nil
}

func removeDockerContainer(name string) error {
	resolved := strings.TrimSpace(name)
	if resolved == "" {
		return nil
	}
	_, err := runDockerOutput("rm", "-f", resolved)
	if err != nil {
		msg := strings.ToLower(strings.TrimSpace(err.Error()))
		if strings.Contains(msg, "no such container") {
			return nil
		}
		return err
	}
	return nil
}

func probeHTTPStatus(rawURL string) int {
	client := http.Client{Timeout: 2 * time.Second}
	resp, err := client.Get(rawURL)
	if err != nil {
		return 0
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, resp.Body)
	return resp.StatusCode
}

func probeContainerHTTPStatus(containerName, rawURL string) int {
	out, err := runDockerOutput("exec", containerName, "sh", "-lc", "curl -sS -o /dev/null -w '%{http_code}' "+singleQuote(rawURL))
	if err != nil {
		return 0
	}
	code, convErr := strconv.Atoi(strings.TrimSpace(out))
	if convErr != nil {
		return 0
	}
	return code
}

func hostConnect(bind string) string {
	resolved := strings.TrimSpace(bind)
	if resolved == "" || resolved == "0.0.0.0" {
		return "127.0.0.1"
	}
	return resolved
}

func mcpURL(cfg browserConfig) string {
	return fmt.Sprintf("http://%s:%d/mcp", hostConnect(cfg.HostBind), cfg.HostMCPPort)
}

func novncURL(cfg browserConfig) string {
	return fmt.Sprintf("http://%s:%d/vnc.html?autoconnect=1&resize=scale", hostConnect(cfg.HostBind), cfg.HostNoVNCPort)
}

func resolveToken(explicit, vaultKey string) (string, error) {
	if strings.TrimSpace(explicit) != "" {
		return strings.TrimSpace(explicit), nil
	}
	if v := strings.TrimSpace(os.Getenv("SURF_CLOUDFLARE_TUNNEL_TOKEN")); v != "" {
		return v, nil
	}
	if strings.TrimSpace(vaultKey) == "" {
		return "", nil
	}
	return vaultGet(strings.TrimSpace(vaultKey))
}

func resolveProfileMount(profileDir string) (mountArg, hostPath string, bindMount bool) {
	resolved := strings.TrimSpace(profileDir)
	lower := strings.ToLower(resolved)
	if strings.HasPrefix(lower, profileVolumePrefix) {
		volumeName := sanitizeProfileName(strings.TrimSpace(resolved[len(profileVolumePrefix):]))
		if volumeName == "" {
			volumeName = "default"
		}
		return fmt.Sprintf("%s:/home/pwuser/.playwright-mcp-profile", "surf-profile-"+volumeName), "", false
	}
	cleaned := filepath.Clean(expandTilde(resolved))
	return fmt.Sprintf("%s:/home/pwuser/.playwright-mcp-profile", cleaned), cleaned, true
}

func defaultHostProfileName() string {
	if profile := strings.TrimSpace(os.Getenv("SURF_HOST_PROFILE")); profile != "" {
		return sanitizeProfileName(profile)
	}
	settings := loadSurfSettingsOrDefault()
	if profile := strings.TrimSpace(settings.Browser.ProfileName); profile != "" {
		return sanitizeProfileName(profile)
	}
	return defaultProfileName
}

func vaultGet(key string) (string, error) {
	mustHaveCommand("si")
	cmd := exec.Command("si", "vault", "get", key)
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = err.Error()
		}
		return "", fmt.Errorf("si vault get %s failed: %s", key, msg)
	}
	return strings.TrimSpace(stdout.String()), nil
}

func surfStateDir() string {
	if p := strings.TrimSpace(os.Getenv("SURF_STATE_DIR")); p != "" {
		return filepath.Clean(expandTilde(p))
	}
	settings := loadSurfSettingsOrDefault()
	if p := strings.TrimSpace(settings.Paths.StateDir); p != "" {
		return filepath.Clean(expandTilde(p))
	}
	home, err := os.UserHomeDir()
	if err != nil || strings.TrimSpace(home) == "" {
		return "/tmp/.surf"
	}
	return filepath.Join(home, ".surf")
}

func intOrFallback(value, fallback int) int {
	if value <= 0 {
		return fallback
	}
	return value
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		trimmed := strings.TrimSpace(value)
		if trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func containerProfileDir(profile string) string {
	return filepath.Join(surfStateDir(), "browser", "profiles", "container", sanitizeProfileName(profile))
}

func hostProfileDir(profile string) string {
	return filepath.Join(surfStateDir(), "browser", "profiles", "host", sanitizeProfileName(profile))
}

func hostRuntimeDir() string {
	return filepath.Join(surfStateDir(), "browser", "host")
}

func hostStatePath(profile string) string {
	return filepath.Join(hostRuntimeDir(), sanitizeProfileName(profile)+".json")
}

func hostLogPath(profile string) string {
	return filepath.Join(hostRuntimeDir(), sanitizeProfileName(profile)+".log")
}

func readHostPID(profile string) (int, error) {
	state, err := readHostState(profile)
	if err != nil {
		return 0, err
	}
	return state.PID, nil
}

func readHostState(profile string) (hostProcessState, error) {
	path := hostStatePath(profile)
	data, err := os.ReadFile(path)
	if err != nil {
		return hostProcessState{}, err
	}
	var state hostProcessState
	if err := json.Unmarshal(data, &state); err != nil {
		return hostProcessState{}, err
	}
	return state, nil
}

func writeHostState(state hostProcessState) error {
	path := hostStatePath(state.Profile)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0o600)
}

func processAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	err := syscall.Kill(pid, 0)
	return err == nil
}

func detectHostBrowserBinary() (string, error) {
	if p := strings.TrimSpace(os.Getenv("SURF_HOST_BROWSER_PATH")); p != "" {
		return p, nil
	}
	if runtime.GOOS == "darwin" {
		candidates := []string{
			"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
			"/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
			"/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
			"/Applications/Chromium.app/Contents/MacOS/Chromium",
			filepath.Join(os.Getenv("HOME"), "Applications", "Google Chrome.app", "Contents", "MacOS", "Google Chrome"),
		}
		for _, candidate := range candidates {
			if candidate == "" {
				continue
			}
			if _, err := os.Stat(candidate); err == nil {
				return candidate, nil
			}
		}
	}
	linuxBins := []string{"google-chrome", "google-chrome-stable", "brave-browser", "microsoft-edge", "chromium", "chromium-browser"}
	for _, b := range linuxBins {
		if p, err := exec.LookPath(b); err == nil {
			return p, nil
		}
	}
	return "", errors.New("no supported Chromium-based browser found; set --browser-path or SURF_HOST_BROWSER_PATH")
}

func sanitizeProfileName(raw string) string {
	name := strings.TrimSpace(raw)
	if name == "" {
		return defaultProfileName
	}
	name = strings.ToLower(name)
	re := regexp.MustCompile(`[^a-z0-9_-]+`)
	name = re.ReplaceAllString(name, "-")
	name = strings.Trim(name, "-")
	if name == "" {
		return defaultProfileName
	}
	return name
}

func defaultExtensionInstallPath() string {
	return filepath.Join(surfStateDir(), "extensions", "chrome-relay")
}

func copyDir(source, dest string) error {
	if err := os.MkdirAll(dest, 0o755); err != nil {
		return err
	}
	entries, err := os.ReadDir(source)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		srcPath := filepath.Join(source, entry.Name())
		dstPath := filepath.Join(dest, entry.Name())
		if entry.IsDir() {
			if err := copyDir(srcPath, dstPath); err != nil {
				return err
			}
			continue
		}
		if err := copyFile(srcPath, dstPath); err != nil {
			return err
		}
	}
	return nil
}

func copyFile(source, dest string) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return err
	}
	out, err := os.Create(dest)
	if err != nil {
		return err
	}
	defer func() { _ = out.Close() }()
	if _, err := io.Copy(out, in); err != nil {
		return err
	}
	return out.Chmod(0o644)
}

func isContainerRunning(name string) (bool, string, error) {
	psOut, err := runDockerOutput("ps", "--filter", "name=^/"+strings.TrimSpace(name)+"$", "--format", "{{.Names}}\t{{.Status}}")
	if err != nil {
		return false, "", err
	}
	line := strings.TrimSpace(psOut)
	return line != "", line, nil
}

func fetchContainerLogs(name string, tail int) string {
	out, _ := runDockerOutput("logs", "--tail", strconv.Itoa(tail), strings.TrimSpace(name))
	return out
}

func extractTryCloudflareURL(logText string) string {
	re := regexp.MustCompile(`https://[a-zA-Z0-9.-]+\.trycloudflare\.com`)
	matches := re.FindAllString(logText, -1)
	if len(matches) == 0 {
		return ""
	}
	return matches[len(matches)-1]
}

func printJSON(v any) {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	if err := enc.Encode(v); err != nil {
		fatal(err)
	}
}

func fatal(err error) {
	fmt.Fprintf(os.Stderr, "%v\n", err)
	os.Exit(1)
}

func mustHaveCommand(cmd string) {
	if _, err := exec.LookPath(cmd); err != nil {
		fatal(fmt.Errorf("missing required command: %s", cmd))
	}
}

func envOr(key, fallback string) string {
	if v := strings.TrimSpace(os.Getenv(key)); v != "" {
		return v
	}
	return fallback
}

func envOrInt(key string, fallback int) int {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(raw)
	if err != nil {
		return fallback
	}
	return parsed
}

func expandTilde(path string) string {
	trimmed := strings.TrimSpace(path)
	if trimmed == "" || !strings.HasPrefix(trimmed, "~") {
		return trimmed
	}
	home, err := os.UserHomeDir()
	if err != nil || strings.TrimSpace(home) == "" {
		return trimmed
	}
	if trimmed == "~" {
		return home
	}
	if strings.HasPrefix(trimmed, "~/") {
		return filepath.Join(home, trimmed[2:])
	}
	return trimmed
}

func singleQuote(s string) string {
	return "'" + strings.ReplaceAll(s, "'", "'\\''") + "'"
}

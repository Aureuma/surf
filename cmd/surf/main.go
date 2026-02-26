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
	"strconv"
	"strings"
	"time"
)

const usageText = `surf <command> [args]

Commands:
  build        Build browser runtime image
  start        Start browser runtime container
  stop         Stop/remove runtime container
  status       Check runtime health
  logs         Stream runtime logs
  proxy        Start MCP path-compat proxy
  tunnel       Manage noVNC cloud tunnel
  extension    Manage Chrome extension scaffold
  version      Print version

Examples:
  surf build
  surf start
  surf status --json
  surf tunnel start
  surf extension install
`

const (
	defaultImage         = "ghcr.io/aureuma/surf-browser:local"
	defaultContainer     = "surf-playwright-mcp-headed"
	defaultNetwork       = "si"
	defaultHostBind      = "127.0.0.1"
	defaultMCPPort       = 8931
	defaultHostMCPPort   = 8932
	defaultNoVNCPort     = 6080
	defaultHostNoVNCPort = 6080
	defaultMCPVersion    = "0.0.64"
	version              = "0.1.0"
)

type browserConfig struct {
	ImageName      string `json:"image_name"`
	ContainerName  string `json:"container_name"`
	Network        string `json:"network"`
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
	Error         string `json:"error,omitempty"`
}

func main() {
	if len(os.Args) < 2 {
		fmt.Print(usageText)
		os.Exit(1)
	}
	cmd := strings.ToLower(strings.TrimSpace(os.Args[1]))
	args := os.Args[2:]

	switch cmd {
	case "help", "-h", "--help":
		fmt.Print(usageText)
	case "version", "--version", "-v":
		fmt.Println(version)
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
	case "tunnel":
		cmdTunnel(args)
	case "extension":
		cmdExtension(args)
	default:
		fatal(fmt.Errorf("unknown command: %s", cmd))
	}
}

func defaultConfig() browserConfig {
	home, _ := os.UserHomeDir()
	profileDir := "/tmp/.surf-browser-profile"
	if strings.TrimSpace(home) != "" {
		profileDir = filepath.Join(home, ".surf", "browser", "profile")
	}
	vncPassword := strings.TrimSpace(os.Getenv("SURF_VNC_PASSWORD"))
	if vncPassword == "" {
		vncPassword = "surf"
	}
	return browserConfig{
		ImageName:      envOr("SURF_IMAGE", defaultImage),
		ContainerName:  envOr("SURF_CONTAINER", defaultContainer),
		Network:        envOr("SURF_NETWORK", defaultNetwork),
		ProfileDir:     envOr("SURF_PROFILE_DIR", profileDir),
		HostBind:       envOr("SURF_HOST_BIND", defaultHostBind),
		HostMCPPort:    envOrInt("SURF_HOST_MCP_PORT", defaultHostMCPPort),
		HostNoVNCPort:  envOrInt("SURF_HOST_NOVNC_PORT", defaultHostNoVNCPort),
		MCPPort:        envOrInt("SURF_MCP_PORT", defaultMCPPort),
		NoVNCPort:      envOrInt("SURF_NOVNC_PORT", defaultNoVNCPort),
		VNCPassword:    vncPassword,
		MCPVersion:     envOr("SURF_MCP_VERSION", defaultMCPVersion),
		BrowserChannel: envOr("SURF_BROWSER_CHANNEL", "chromium"),
		AllowedHosts:   envOr("SURF_ALLOWED_HOSTS", "*"),
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
		fatal(errors.New("usage: surf start [--skip-build] [--repo <path>] [--image <name>] [--name <container>] [--network <name>] [--profile-dir <path>] [--host-bind <addr>] [--host-mcp-port <n>] [--host-novnc-port <n>] [--mcp-port <n>] [--novnc-port <n>] [--vnc-password <pwd>] [--mcp-version <ver>] [--browser <name>] [--allowed-hosts <list>] [--json]"))
	}
	mustHaveCommand("docker")

	if strings.TrimSpace(cfg.ProfileDir) == "" {
		fatal(errors.New("profile dir is required"))
	}
	if err := os.MkdirAll(cfg.ProfileDir, 0o700); err != nil {
		fatal(err)
	}

	if !*skipBuild {
		buildArgs := []string{"--repo", strings.TrimSpace(*repo), "--image", cfg.ImageName}
		cmdBuild(buildArgs)
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
		"-v", fmt.Sprintf("%s:/home/pwuser/.playwright-mcp-profile", cfg.ProfileDir),
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
	fmt.Printf("  mcp_url=%s\n", mcpURL(cfg))
	fmt.Printf("  novnc_url=%s\n", novncURL(cfg))
	fmt.Printf("  profile_dir=%s\n", cfg.ProfileDir)
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
		printJSON(map[string]any{
			"ok":             true,
			"command":        "stop",
			"container_name": cfg.ContainerName,
		})
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
		fatal(errors.New("usage: surf status [--image <name>] [--name <container>] [--network <name>] [--host-bind <addr>] [--host-mcp-port <n>] [--host-novnc-port <n>] [--mcp-port <n>] [--novnc-port <n>] [--json]"))
	}
	mustHaveCommand("docker")

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
		if req.Method == http.MethodGet {
			if req.URL.Path == "/mcp" {
				req.URL.Path = "/sse"
			} else if strings.HasPrefix(req.URL.Path, "/mcp/") {
				req.URL.Path = "/sse" + strings.TrimPrefix(req.URL.Path, "/mcp")
			}
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
	name := fs.String("name", "surf-cloudflared", "container name")
	target := fs.String("target-url", novncURL(cfg), "target URL to expose")
	jsonOut := fs.Bool("json", false, "output json")
	_ = fs.Parse(args)
	if fs.NArg() > 0 {
		fatal(errors.New("usage: surf tunnel start [--name <container>] [--target-url <url>] [--json]"))
	}
	mustHaveCommand("docker")

	_ = removeDockerContainer(*name)
	_, err := runDockerOutput(
		"run", "-d",
		"--name", strings.TrimSpace(*name),
		"--restart", "unless-stopped",
		"cloudflare/cloudflared:latest",
		"tunnel", "--no-autoupdate", "--url", strings.TrimSpace(*target),
	)
	if err != nil {
		fatal(err)
	}
	url := extractTryCloudflareURL(fetchContainerLogs(*name, 200))
	payload := tunnelStatusPayload{OK: true, ContainerName: *name, Running: true, URL: url}
	if *jsonOut {
		printJSON(payload)
		return
	}
	fmt.Printf("surf tunnel start: %s\n", *name)
	if strings.TrimSpace(url) != "" {
		fmt.Printf("  public_url=%s\n", url)
	} else {
		fmt.Printf("  public_url=pending (run `surf tunnel status`)\n")
	}
}

func cmdTunnelStop(args []string) {
	fs := flag.NewFlagSet("tunnel stop", flag.ExitOnError)
	name := fs.String("name", "surf-cloudflared", "container name")
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
	name := fs.String("name", "surf-cloudflared", "container name")
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
	name := fs.String("name", "surf-cloudflared", "container name")
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
	cwd, err := os.Getwd()
	if err == nil {
		candidate := filepath.Clean(cwd)
		if hasSurfLayout(candidate) {
			return candidate, nil
		}
	}
	exe, err := os.Executable()
	if err == nil {
		dir := filepath.Dir(exe)
		cand := filepath.Clean(filepath.Join(dir, "..", ".."))
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

func defaultExtensionInstallPath() string {
	home, _ := os.UserHomeDir()
	if strings.TrimSpace(home) == "" {
		return "/tmp/.surf/extensions/chrome-relay"
	}
	return filepath.Join(home, ".surf", "extensions", "chrome-relay")
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

use std::fs;
use std::io::Write;
#[cfg(test)]
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tungstenite::handshake::HandshakeError;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket, connect};
use url::Url;

use crate::constants::{SESSION_MODE_INTERACTIVE, SESSION_MODE_READ_ONLY};
use crate::paths::sanitize_profile_name;
use crate::settings::{load_surf_settings_or_default, surf_state_dir};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeTarget {
    pub id: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl", default)]
    pub websocket_debugger_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachedSession {
    pub session: String,
    pub browser: String,
    pub mode: String,
    pub target_id: String,
    pub title: String,
    pub url: String,
    pub ws_url: String,
    pub cdp_host: String,
    pub cdp_port: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionActionRequest {
    pub action: String,
    pub selector: String,
    pub text: String,
    pub expr: String,
    pub out: String,
    pub delta_y: i32,
    pub steps: i32,
    pub human: SessionHumanOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionActionResult {
    pub action: String,
    pub session: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub output: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionHumanOptions {
    pub enabled: bool,
    pub min_delay_ms: i32,
    pub max_delay_ms: i32,
    pub type_min_delay: i32,
    pub type_max_delay: i32,
    pub mouse_steps: i32,
    pub scroll_step_px: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct CdpRequest {
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Value::is_null", default)]
    params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct CdpError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CdpResponse {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<CdpError>,
}

pub fn default_session_browser() -> String {
    let settings = load_surf_settings_or_default();
    let browser = settings
        .existing_session
        .default_browser
        .trim()
        .to_lowercase();
    if browser.is_empty() {
        "chrome".to_owned()
    } else {
        browser
    }
}

pub fn default_session_mode() -> String {
    let settings = load_surf_settings_or_default();
    let mode = settings.existing_session.mode.trim().to_lowercase();
    if mode.is_empty() {
        SESSION_MODE_READ_ONLY.to_owned()
    } else {
        mode
    }
}

pub fn default_session_chrome_host() -> String {
    let settings = load_surf_settings_or_default();
    let host = settings.existing_session.chrome_host.trim();
    if host.is_empty() {
        "127.0.0.1".to_owned()
    } else {
        host.to_owned()
    }
}

pub fn default_session_chrome_port() -> i32 {
    let settings = load_surf_settings_or_default();
    if settings.existing_session.chrome_cdp_port > 0 {
        settings.existing_session.chrome_cdp_port
    } else {
        crate::constants::DEFAULT_HOST_CDP_PORT
    }
}

pub fn default_session_attach_timeout() -> Duration {
    let settings = load_surf_settings_or_default();
    Duration::from_secs(settings.existing_session.attach_timeout_seconds.max(8) as u64)
}

pub fn default_session_action_timeout() -> Duration {
    let settings = load_surf_settings_or_default();
    Duration::from_secs(settings.existing_session.action_timeout_seconds.max(15) as u64)
}

pub fn default_session_humanize() -> bool {
    load_surf_settings_or_default().existing_session.humanize
}

pub fn default_session_human_min_delay_ms() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_min_delay_ms
        .max(40)
}

pub fn default_session_human_max_delay_ms() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_max_delay_ms
        .max(180)
}

pub fn default_session_human_type_min_delay_ms() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_type_min_delay_ms
        .max(35)
}

pub fn default_session_human_type_max_delay_ms() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_type_max_delay_ms
        .max(130)
}

pub fn default_session_human_mouse_steps() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_mouse_steps
        .max(12)
}

pub fn default_session_human_scroll_step_px() -> i32 {
    load_surf_settings_or_default()
        .existing_session
        .human_scroll_step_px
        .max(280)
}

pub fn normalize_session_mode(raw: &str) -> Result<String> {
    let mode = raw.trim().to_lowercase();
    match mode.as_str() {
        "" | SESSION_MODE_READ_ONLY => Ok(SESSION_MODE_READ_ONLY.to_owned()),
        SESSION_MODE_INTERACTIVE => Ok(SESSION_MODE_INTERACTIVE.to_owned()),
        _ => bail!("invalid session mode {raw:?} (expected read_only|interactive)"),
    }
}

pub fn session_state_dir() -> PathBuf {
    surf_state_dir().join("browser").join("sessions")
}

pub fn session_state_path(name: &str) -> PathBuf {
    session_state_dir().join(format!("{}.json", sanitize_profile_name(name)))
}

pub fn write_attached_session(session: &AttachedSession) -> Result<()> {
    if session.session.trim().is_empty() {
        bail!("session name is required");
    }
    if session.ws_url.trim().is_empty() {
        bail!("target websocket URL is required");
    }
    fs::create_dir_all(session_state_dir()).context("create session state directory")?;
    let data = serde_json::to_string_pretty(session).context("serialize attached session")?;
    fs::write(session_state_path(&session.session), data).context("write attached session")?;
    Ok(())
}

pub fn read_attached_session(name: &str) -> Result<AttachedSession> {
    let path = session_state_path(name);
    let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut session: AttachedSession =
        serde_json::from_str(&data).context("parse attached session")?;
    if session.session.trim().is_empty() {
        session.session = sanitize_profile_name(name);
    }
    if session.mode.trim().is_empty() {
        session.mode = SESSION_MODE_READ_ONLY.to_owned();
    }
    Ok(session)
}

pub fn list_attached_sessions() -> Result<Vec<AttachedSession>> {
    let mut items = Vec::new();
    let entries = match fs::read_dir(session_state_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(items),
        Err(err) => return Err(err).context("read session state directory"),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir()
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if let Ok(session) = read_attached_session(name) {
            items.push(session);
        }
    }
    items.sort_by(|a, b| a.session.cmp(&b.session));
    Ok(items)
}

pub fn delete_attached_session(name: &str) -> Result<()> {
    let path = session_state_path(name);
    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))
}

pub fn choose_chrome_target(
    targets: &[ChromeTarget],
    id: &str,
    url_contains: &str,
    title_contains: &str,
) -> Result<ChromeTarget> {
    if targets.is_empty() {
        bail!("no attachable browser targets discovered");
    }
    if !id.trim().is_empty() {
        for target in targets {
            if target.id.eq_ignore_ascii_case(id.trim()) {
                return Ok(target.clone());
            }
        }
        bail!("target id not found: {}", id.trim());
    }
    if !url_contains.trim().is_empty() || !title_contains.trim().is_empty() {
        let url_filter = url_contains.trim().to_lowercase();
        let title_filter = title_contains.trim().to_lowercase();
        for target in targets {
            let url_ok = url_filter.is_empty() || target.url.to_lowercase().contains(&url_filter);
            let title_ok =
                title_filter.is_empty() || target.title.to_lowercase().contains(&title_filter);
            if url_ok && title_ok {
                return Ok(target.clone());
            }
        }
        bail!("no target matched --url-contains/--title-contains");
    }
    if targets.len() == 1 {
        return Ok(targets[0].clone());
    }
    bail!(
        "multiple targets discovered; provide --id or filter with --url-contains/--title-contains"
    )
}

pub fn discover_chrome_targets(
    host: &str,
    cdp_port: i32,
    timeout: Duration,
) -> Result<Vec<ChromeTarget>> {
    let host = if host.trim().is_empty() {
        "127.0.0.1"
    } else {
        host.trim()
    };
    let cdp_port = if cdp_port <= 0 {
        crate::constants::DEFAULT_HOST_CDP_PORT
    } else {
        cdp_port
    };
    let endpoint = format!("http://{host}:{cdp_port}/json/list");
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .context("build chrome target discovery client")?;
    let response = client
        .get(&endpoint)
        .send()
        .with_context(|| format!("discover chrome targets from {endpoint}"))?;
    if !response.status().is_success() {
        bail!(
            "discover chrome targets failed: status={}",
            response.status().as_u16()
        );
    }
    let targets: Vec<ChromeTarget> = response.json().context("decode chrome targets")?;
    Ok(targets
        .into_iter()
        .filter(|target| {
            !target.websocket_debugger_url.trim().is_empty()
                && (target.r#type.trim().is_empty() || target.r#type.trim() == "page")
        })
        .collect())
}

pub fn run_session_action(
    session: &AttachedSession,
    req: &SessionActionRequest,
) -> Result<SessionActionResult> {
    let action = req.action.trim().to_lowercase();
    if action.is_empty() {
        bail!("action is required");
    }
    if session.browser != "chrome" {
        bail!(
            "browser {:?} is not supported for session actions yet",
            session.browser
        );
    }
    let mut session = session.clone();
    if session.mode.trim().is_empty() {
        session.mode = SESSION_MODE_READ_ONLY.to_owned();
    }
    if session.mode == SESSION_MODE_READ_ONLY
        && matches!(action.as_str(), "click" | "type" | "paste" | "scroll")
    {
        bail!("action {action:?} requires interactive mode (session is read_only)");
    }

    let human = normalize_session_human_options(req.human.clone());
    let mut client = SessionCdp::connect(&session)?;
    let current_url = cdp_eval_value(&mut client, "location.href")
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| session.url.clone());
    enforce_session_domain_policy(&current_url)?;

    let mut result = SessionActionResult {
        action: action.clone(),
        session: session.session.clone(),
        mode: session.mode.clone(),
        value: None,
        output: String::new(),
    };

    match action.as_str() {
        "title" => result.value = Some(cdp_eval_value(&mut client, "document.title")?),
        "url" => result.value = Some(cdp_eval_value(&mut client, "location.href")?),
        "text" => {
            result.value = Some(cdp_eval_value(
                &mut client,
                "document.body ? document.body.innerText : ''",
            )?)
        }
        "elements" => {
            result.value = Some(cdp_eval_value(
                &mut client,
                r##"(() => {
  const toText = (value) => (value || "").toString().trim().replace(/\s+/g, " ").slice(0, 160);
  const list = [];
  const selectors = "a,button,input,textarea,select,[role='button'],[role='link'],[contenteditable='true'],[tabindex]";
  const nodes = Array.from(document.querySelectorAll(selectors)).slice(0, 120);
  for (const el of nodes) {
    const rect = el.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) continue;
    const style = window.getComputedStyle(el);
    if (style && (style.visibility === "hidden" || style.display === "none")) continue;
    const tag = (el.tagName || "").toLowerCase();
    const role = toText(el.getAttribute("role"));
    const id = toText(el.id);
    const classes = toText(el.className);
    const name = toText(el.getAttribute("name"));
    const placeholder = toText(el.getAttribute("placeholder"));
    const ariaLabel = toText(el.getAttribute("aria-label"));
    const text = toText(el.innerText || el.textContent || el.value);
    let selector = tag;
    if (id) selector += "#" + id.replace(/\s+/g, "");
    else if (classes) selector += "." + classes.split(" ").slice(0, 2).join(".");
    list.push({
      tag,
      role,
      id,
      name,
      placeholder,
      aria_label: ariaLabel,
      text,
      selector,
      x: Math.round(rect.left + rect.width / 2),
      y: Math.round(rect.top + rect.height / 2),
      width: Math.round(rect.width),
      height: Math.round(rect.height)
    });
  }
  return list;
})()"##,
            )?)
        }
        "copy" => {
            result.value = Some(cdp_eval_value(
                &mut client,
                r#"(() => {
  const selected = window.getSelection ? window.getSelection().toString() : "";
  if (selected && selected.trim() !== "") return selected;
  const active = document.activeElement;
  if (active && "value" in active) return String(active.value || "");
  return "";
})()"#,
            )?)
        }
        "eval" => {
            if req.expr.trim().is_empty() {
                bail!("--expr is required for eval action");
            }
            result.value = Some(cdp_eval_value(&mut client, &req.expr)?)
        }
        "click" => {
            if req.selector.trim().is_empty() {
                bail!("--selector is required for click action");
            }
            result.value = Some(cdp_human_click(
                &mut client,
                &req.selector,
                &human,
                req.steps,
            )?);
        }
        "type" => {
            if req.selector.trim().is_empty() {
                bail!("--selector is required for type action");
            }
            result.value = Some(cdp_human_type(
                &mut client,
                &req.selector,
                &req.text,
                &human,
                req.steps,
            )?);
        }
        "paste" => {
            if req.text.trim().is_empty() {
                bail!("--text is required for paste action");
            }
            result.value = Some(cdp_type_into_focused_element(
                &mut client,
                &req.text,
                &human,
            )?);
        }
        "scroll" => {
            result.value = Some(cdp_human_scroll(
                &mut client,
                req.delta_y,
                req.steps,
                &human,
            )?);
        }
        "screenshot" => {
            client.call("Page.enable", json!({}))?;
            let raw = client.call("Page.captureScreenshot", json!({"format": "png"}))?;
            let data = raw
                .get("data")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("capture screenshot returned empty payload"))?;
            let image = base64::engine::general_purpose::STANDARD
                .decode(data)
                .context("decode screenshot payload")?;
            let output = if req.out.trim().is_empty() {
                session_state_dir().join(format!(
                    "{}-{}.png",
                    sanitize_profile_name(&session.session),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                ))
            } else {
                crate::paths::expand_tilde(&req.out)
            };
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create screenshot directory {}", parent.display()))?;
            }
            let mut file = fs::File::create(&output)
                .with_context(|| format!("create screenshot {}", output.display()))?;
            file.write_all(&image)
                .with_context(|| format!("write screenshot {}", output.display()))?;
            result.output = output.display().to_string();
        }
        _ => bail!("unsupported action {action:?}"),
    }

    Ok(result)
}

pub fn normalize_session_human_options(mut options: SessionHumanOptions) -> SessionHumanOptions {
    if options.min_delay_ms <= 0 {
        options.min_delay_ms = default_session_human_min_delay_ms();
    }
    if options.max_delay_ms <= 0 {
        options.max_delay_ms = default_session_human_max_delay_ms();
    }
    if options.max_delay_ms < options.min_delay_ms {
        options.max_delay_ms = options.min_delay_ms;
    }
    if options.type_min_delay <= 0 {
        options.type_min_delay = default_session_human_type_min_delay_ms();
    }
    if options.type_max_delay <= 0 {
        options.type_max_delay = default_session_human_type_max_delay_ms();
    }
    if options.type_max_delay < options.type_min_delay {
        options.type_max_delay = options.type_min_delay;
    }
    if options.mouse_steps <= 0 {
        options.mouse_steps = default_session_human_mouse_steps();
    }
    if options.scroll_step_px <= 0 {
        options.scroll_step_px = default_session_human_scroll_step_px();
    }
    options
}

pub fn enforce_session_domain_policy(session_url: &str) -> Result<()> {
    let settings = load_surf_settings_or_default();
    let allowed = if settings.existing_session.allowed_domains.is_empty() {
        vec!["*".to_owned()]
    } else {
        settings.existing_session.allowed_domains
    };
    let blocked = settings.existing_session.blocked_domains;

    let Ok(parsed) = Url::parse(session_url.trim()) else {
        return Ok(());
    };
    let Some(host) = parsed.host_str() else {
        return Ok(());
    };
    let host = host.trim().to_lowercase();
    for pattern in blocked {
        if domain_match(&host, &pattern) {
            bail!("session domain policy blocked host {host:?} (matched {pattern:?})");
        }
    }
    for pattern in allowed {
        if domain_match(&host, &pattern) {
            return Ok(());
        }
    }
    bail!("session domain policy denied host {host:?}")
}

pub fn domain_match(host: &str, pattern: &str) -> bool {
    let host = host.trim().to_lowercase();
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    host == pattern
}

fn cdp_human_click(
    client: &mut SessionCdp,
    selector: &str,
    human: &SessionHumanOptions,
    steps_override: i32,
) -> Result<Value> {
    let target = cdp_resolve_target_point(client, selector)?;
    let x = value_as_f64(target.get("x"));
    let y = value_as_f64(target.get("y"));
    if x == 0.0 && y == 0.0 {
        bail!("selector {selector:?} returned invalid target point");
    }
    let (start_x, start_y) = cdp_viewport_center(client)?;
    let steps = if steps_override > 0 {
        steps_override
    } else {
        human.mouse_steps
    };
    cdp_move_pointer(client, start_x, start_y, x, y, steps, human)?;
    client.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    sleep_human_delay(
        human.min_delay_ms / 2,
        human.max_delay_ms / 2,
        human.enabled,
    );
    client.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    sleep_human_delay(human.min_delay_ms, human.max_delay_ms, human.enabled);
    Ok(json!({"ok": true, "x": x, "y": y}))
}

fn cdp_human_type(
    client: &mut SessionCdp,
    selector: &str,
    text: &str,
    human: &SessionHumanOptions,
    steps_override: i32,
) -> Result<Value> {
    cdp_human_click(client, selector, human, steps_override)?;
    cdp_eval_value(
        client,
        &format!(
            r#"(() => {{
  const el = document.querySelector({});
  if (!el) return {{ ok: false, error: "selector not found" }};
  el.focus();
  if ("value" in el) {{
    el.value = "";
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
  }}
  return {{ ok: true }};
}})()"#,
            js_string_literal(selector)
        ),
    )?;
    let typed = cdp_type_into_focused_element(client, text, human)?;
    let _ = cdp_eval_value(
        client,
        r#"(() => {
  const el = document.activeElement;
  if (!el) return { ok: false };
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return { ok: true };
})()"#,
    );
    Ok(typed)
}

fn cdp_type_into_focused_element(
    client: &mut SessionCdp,
    text: &str,
    human: &SessionHumanOptions,
) -> Result<Value> {
    let length = text.chars().count();
    for ch in text.chars() {
        client.call("Input.insertText", json!({"text": ch.to_string()}))?;
        sleep_human_delay(human.type_min_delay, human.type_max_delay, human.enabled);
    }
    Ok(json!({"ok": true, "length": length}))
}

fn cdp_human_scroll(
    client: &mut SessionCdp,
    delta_y: i32,
    steps: i32,
    human: &SessionHumanOptions,
) -> Result<Value> {
    let delta_y = if delta_y == 0 {
        human.scroll_step_px
    } else {
        delta_y
    };
    let steps = if steps <= 0 { 3 } else { steps };
    let (x, y) = cdp_viewport_center(client)?;
    let per_step = delta_y as f64 / steps as f64;
    for _ in 0..steps {
        client.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":per_step}),
        )?;
        sleep_human_delay(human.min_delay_ms, human.max_delay_ms, human.enabled);
    }
    Ok(json!({"ok": true, "delta_y": delta_y, "steps": steps}))
}

fn cdp_resolve_target_point(client: &mut SessionCdp, selector: &str) -> Result<Value> {
    let value = cdp_eval_value(
        client,
        &format!(
            r#"(() => {{
  const el = document.querySelector({});
  if (!el) return {{ ok: false, error: "selector not found" }};
  el.scrollIntoView({{ behavior: "auto", block: "center", inline: "center" }});
  const rect = el.getBoundingClientRect();
  return {{
    ok: true,
    x: rect.left + rect.width / 2,
    y: rect.top + rect.height / 2,
    tag: (el.tagName || "").toLowerCase(),
    text: (el.innerText || el.textContent || "").trim().slice(0, 80)
  }};
}})()"#,
            js_string_literal(selector)
        ),
    )?;
    let value_map = value
        .as_object()
        .ok_or_else(|| anyhow!("selector {selector:?} did not return an object payload"))?;
    if value_map.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = value_map
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("selector not found");
        bail!("selector {selector:?}: {error}");
    }
    Ok(value)
}

fn cdp_viewport_center(client: &mut SessionCdp) -> Result<(f64, f64)> {
    let value = cdp_eval_value(
        client,
        "({ x: window.innerWidth / 2, y: window.innerHeight / 2 })",
    )?;
    let value_map = value
        .as_object()
        .ok_or_else(|| anyhow!("viewport center response was not an object"))?;
    Ok((
        value_as_f64(value_map.get("x")),
        value_as_f64(value_map.get("y")),
    ))
}

fn cdp_move_pointer(
    client: &mut SessionCdp,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    steps: i32,
    human: &SessionHumanOptions,
) -> Result<()> {
    let steps = steps.max(1);
    for index in 1..=steps {
        let t = index as f64 / steps as f64;
        let x = from_x + (to_x - from_x) * t;
        let y = from_y + (to_y - from_y) * t;
        client.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":x,"y":y}),
        )?;
        sleep_human_delay(
            human.min_delay_ms / 2,
            human.max_delay_ms / 2,
            human.enabled,
        );
    }
    Ok(())
}

fn value_as_f64(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn sleep_human_delay(min_delay_ms: i32, _max_delay_ms: i32, enabled: bool) {
    if !enabled {
        return;
    }
    let delay = min_delay_ms.max(5) as u64;
    thread::sleep(Duration::from_millis(delay));
}

fn cdp_eval_value(client: &mut SessionCdp, expr: &str) -> Result<Value> {
    let raw = client.call(
        "Runtime.evaluate",
        json!({"expression": expr, "returnByValue": true, "awaitPromise": true}),
    )?;
    if raw.get("exceptionDetails").is_some() {
        bail!(
            "javascript evaluation failed: {:?}",
            raw.get("exceptionDetails")
        );
    }
    let result = raw.get("result").cloned().unwrap_or(Value::Null);
    if let Some(value) = result.get("value") {
        return Ok(value.clone());
    }
    Ok(result
        .get("description")
        .cloned()
        .unwrap_or(Value::String(String::new())))
}

fn js_string_literal(raw: &str) -> String {
    serde_json::to_string(raw).unwrap_or_else(|_| "\"\"".to_owned())
}

pub fn split_host_port_from_url(raw_url: &str) -> Result<(String, i32)> {
    let parsed = Url::parse(raw_url).context("parse url")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("url missing host/port: {raw_url}"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow!("url missing host/port: {raw_url}"))?;
    Ok((host.to_owned(), i32::from(port)))
}

pub fn current_timestamp_rfc3339() -> String {
    let output = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => "1970-01-01T00:00:00Z".to_owned(),
    }
}

struct SessionCdp {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl SessionCdp {
    fn connect(session: &AttachedSession) -> Result<Self> {
        let ws_url = if !session.ws_url.trim().is_empty() {
            session.ws_url.trim().to_owned()
        } else if !session.target_id.trim().is_empty()
            && !session.cdp_host.trim().is_empty()
            && session.cdp_port > 0
        {
            format!(
                "ws://{}:{}/devtools/page/{}",
                session.cdp_host.trim(),
                session.cdp_port,
                session.target_id.trim()
            )
        } else {
            bail!("session missing websocket debugger URL and endpoint details");
        };
        let parsed = Url::parse(&ws_url).context("parse websocket url")?;
        if !matches!(parsed.scheme(), "ws" | "wss") {
            bail!("invalid websocket scheme in {ws_url}");
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("websocket url missing host: {ws_url}"))?
            .to_owned();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| anyhow!("websocket url missing port: {ws_url}"))?;
        let mut last_error = None;
        for _ in 0..10 {
            let attempt = if parsed.scheme() == "ws" {
                TcpStream::connect((host.as_str(), port))
                    .map_err(tungstenite::Error::Io)
                    .and_then(|stream| {
                        stream
                            .set_nodelay(true)
                            .map_err(tungstenite::Error::Io)
                            .and_then(|_| {
                                match tungstenite::client(
                                    ws_url.as_str(),
                                    MaybeTlsStream::Plain(stream),
                                ) {
                                    Ok((socket, response)) => Ok((socket, response)),
                                    Err(HandshakeError::Failure(error)) => Err(error),
                                    Err(HandshakeError::Interrupted(_)) => {
                                        Err(tungstenite::Error::Io(std::io::Error::new(
                                            std::io::ErrorKind::WouldBlock,
                                            "interrupted websocket handshake",
                                        )))
                                    }
                                }
                            })
                    })
            } else {
                connect(ws_url.as_str())
            };
            match attempt {
                Ok((socket, _)) => return Ok(Self { socket, next_id: 1 }),
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
        let error = last_error.unwrap();
        let _ = parsed;
        let _ = ws_url;
        let error = anyhow!(error).context("connect to browser target");
        Err(error)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = CdpRequest {
            id,
            method: method.to_owned(),
            params,
        };
        self.socket
            .send(Message::Text(serde_json::to_string(&request)?.into()))
            .context("send cdp request")?;
        loop {
            let message = self.socket.read().context("read cdp response")?;
            let Message::Text(text) = message else {
                continue;
            };
            let response: CdpResponse =
                serde_json::from_str(&text).context("decode cdp response")?;
            if response.id == 0 || response.id != id {
                continue;
            }
            if let Some(error) = response.error {
                bail!("cdp {method} failed ({}): {}", error.code, error.message);
            }
            return Ok(response.result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tiny_http::{Header, Response, Server};

    use crate::paths::{env_lock, set_env};
    use crate::settings::{default_surf_settings, save_surf_settings};

    #[test]
    fn choose_chrome_target_prefers_id_and_filters() {
        let targets = vec![
            ChromeTarget {
                id: "a1".to_owned(),
                r#type: "page".to_owned(),
                title: "Inbox".to_owned(),
                url: "https://mail.example.com".to_owned(),
                websocket_debugger_url: "ws://127.0.0.1:1/devtools/page/a1".to_owned(),
            },
            ChromeTarget {
                id: "b2".to_owned(),
                r#type: "page".to_owned(),
                title: "Docs".to_owned(),
                url: "https://docs.example.com".to_owned(),
                websocket_debugger_url: "ws://127.0.0.1:1/devtools/page/b2".to_owned(),
            },
        ];
        assert_eq!(
            choose_chrome_target(&targets, "b2", "", "").unwrap().id,
            "b2"
        );
        assert_eq!(
            choose_chrome_target(&targets, "", "mail", "").unwrap().id,
            "a1"
        );
        assert!(choose_chrome_target(&targets, "", "", "").is_err());
    }

    #[test]
    fn discover_chrome_targets_filters_non_page_targets() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr();
        let (host, port) = split_host_port_from_url(&format!("http://{}", address)).unwrap();
        thread::spawn(move || {
            if let Ok(request) = server.recv() {
                let body = serde_json::to_string(&vec![
                    json!({"id":"page-1","type":"page","title":"One","url":"https://one.example","webSocketDebuggerUrl":"ws://127.0.0.1:123/devtools/page/page-1"}),
                    json!({"id":"worker-1","type":"worker","title":"Worker","url":"","webSocketDebuggerUrl":"ws://127.0.0.1:123/devtools/page/worker-1"}),
                    json!({"id":"page-2","type":"page","title":"Two","url":"https://two.example","webSocketDebuggerUrl":""}),
                ])
                .unwrap();
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
            }
        });
        let targets = discover_chrome_targets(&host, port, Duration::from_secs(2)).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "page-1");
    }

    #[test]
    #[serial]
    fn session_state_round_trip() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let state_dir = tempfile::tempdir().unwrap();
        set_env(
            "SURF_STATE_DIR",
            Some(state_dir.path().to_string_lossy().as_ref()),
        );
        let session = AttachedSession {
            session: "alpha".to_owned(),
            browser: "chrome".to_owned(),
            mode: SESSION_MODE_READ_ONLY.to_owned(),
            target_id: "tab-1".to_owned(),
            title: "Example".to_owned(),
            url: "https://example.com".to_owned(),
            ws_url: "ws://127.0.0.1:9222/devtools/page/tab-1".to_owned(),
            cdp_host: "127.0.0.1".to_owned(),
            cdp_port: 9222,
            created_at: "2026-03-15T00:00:00Z".to_owned(),
        };
        write_attached_session(&session).unwrap();
        assert_eq!(read_attached_session("alpha").unwrap().target_id, "tab-1");
        assert_eq!(list_attached_sessions().unwrap().len(), 1);
        delete_attached_session("alpha").unwrap();
        assert!(read_attached_session("alpha").is_err());
    }

    #[test]
    fn read_only_blocks_write_actions() {
        let session = AttachedSession {
            session: "ro".to_owned(),
            browser: "chrome".to_owned(),
            mode: SESSION_MODE_READ_ONLY.to_owned(),
            target_id: String::new(),
            title: String::new(),
            url: String::new(),
            ws_url: "ws://127.0.0.1:1/devtools/page/x".to_owned(),
            cdp_host: String::new(),
            cdp_port: 0,
            created_at: String::new(),
        };
        for action in ["click", "type", "paste", "scroll"] {
            let err = run_session_action(
                &session,
                &SessionActionRequest {
                    action: action.to_owned(),
                    selector: "#x".to_owned(),
                    text: "hi".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(err.to_string().contains("requires interactive mode"));
        }
    }

    #[test]
    #[serial]
    fn run_session_action_against_mock_cdp() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let (ws_url, calls) = start_mock_cdp_target_server();
        let session = AttachedSession {
            session: "work".to_owned(),
            browser: "chrome".to_owned(),
            mode: SESSION_MODE_INTERACTIVE.to_owned(),
            target_id: String::new(),
            title: String::new(),
            url: String::new(),
            ws_url,
            cdp_host: String::new(),
            cdp_port: 0,
            created_at: String::new(),
        };

        let title = run_session_action(
            &session,
            &SessionActionRequest {
                action: "title".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(title.value.unwrap(), Value::String("Mock Title".to_owned()));

        let url = run_session_action(
            &session,
            &SessionActionRequest {
                action: "url".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            url.value.unwrap(),
            Value::String("https://example.test/page".to_owned())
        );

        let text = run_session_action(
            &session,
            &SessionActionRequest {
                action: "text".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            text.value.unwrap(),
            Value::String("Example Body".to_owned())
        );

        let eval = run_session_action(
            &session,
            &SessionActionRequest {
                action: "eval".to_owned(),
                expr: "2+2".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(eval.value.unwrap(), json!(4));

        let click = run_session_action(
            &session,
            &SessionActionRequest {
                action: "click".to_owned(),
                selector: "#open".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(click.value.unwrap()["ok"], json!(true));

        let typed = run_session_action(
            &session,
            &SessionActionRequest {
                action: "type".to_owned(),
                selector: "#q".to_owned(),
                text: "hello".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(typed.value.unwrap()["length"], json!(5));

        let copy = run_session_action(
            &session,
            &SessionActionRequest {
                action: "copy".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(copy.value.unwrap(), Value::String("Mock Copy".to_owned()));

        let screenshot_file = tempfile::tempdir().unwrap().path().join("shot.png");
        let shot = run_session_action(
            &session,
            &SessionActionRequest {
                action: "screenshot".to_owned(),
                out: screenshot_file.display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(shot.output, screenshot_file.display().to_string());
        assert_eq!(fs::read_to_string(&screenshot_file).unwrap(), "PNGDATA");

        let calls = calls.lock().unwrap().clone();
        assert!(calls.contains(&"Input.dispatchMouseEvent".to_owned()));
        assert!(calls.contains(&"Input.insertText".to_owned()));
    }

    #[test]
    #[serial]
    fn enforce_domain_policy_uses_settings() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        set_env(
            "SURF_SETTINGS_HOME",
            Some(home.path().to_string_lossy().as_ref()),
        );
        set_env("SURF_SETTINGS_FILE", None);
        let mut settings = default_surf_settings();
        settings.existing_session.allowed_domains = vec!["example.test".to_owned()];
        settings.existing_session.blocked_domains = vec!["admin.example.test".to_owned()];
        save_surf_settings(&settings).unwrap();
        assert!(enforce_session_domain_policy("https://example.test/path").is_ok());
        assert!(enforce_session_domain_policy("https://admin.example.test/settings").is_err());
        assert!(enforce_session_domain_policy("https://other.example.test").is_err());
    }

    fn start_mock_cdp_target_server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = Arc::clone(&calls);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    break;
                };
                let Ok(mut socket) = tungstenite::accept(stream) else {
                    continue;
                };
                loop {
                    let message = match socket.read() {
                        Ok(message) => message,
                        Err(_) => break,
                    };
                    let Message::Text(text) = message else {
                        continue;
                    };
                    let request: Value = serde_json::from_str(&text).unwrap();
                    let id = request["id"].as_u64().unwrap();
                    let method = request["method"].as_str().unwrap().to_owned();
                    calls_clone.lock().unwrap().push(method.clone());
                    let expression = request["params"]["expression"].as_str().unwrap_or_default();
                    let result = match method.as_str() {
                        "Runtime.evaluate" => {
                            if expression.contains("document.title") {
                                json!({"result": {"type": "string", "value": "Mock Title"}})
                            } else if expression.contains("location.href") {
                                json!({"result": {"type": "string", "value": "https://example.test/page"}})
                            } else if expression.contains("querySelectorAll(selectors)") {
                                json!({"result": {"type": "object", "value": [{"tag":"button","selector":"button#open","text":"Open"}]}})
                            } else if expression.contains("getBoundingClientRect") {
                                json!({"result": {"type": "object", "value": {"ok": true, "x": 320, "y": 220, "tag":"button","text":"Open"}}})
                            } else if expression.contains("innerText") {
                                json!({"result": {"type": "string", "value": "Example Body"}})
                            } else if expression.contains("window.getSelection") {
                                json!({"result": {"type": "string", "value": "Mock Copy"}})
                            } else if expression.contains("window.innerWidth") {
                                json!({"result": {"type": "object", "value": {"x": 640, "y": 360}}})
                            } else if expression.contains("el.focus") {
                                json!({"result": {"type": "object", "value": {"ok": true}}})
                            } else {
                                json!({"result": {"type": "number", "value": 4}})
                            }
                        }
                        "Page.captureScreenshot" => {
                            json!({"data": base64::engine::general_purpose::STANDARD.encode("PNGDATA")})
                        }
                        "Input.dispatchMouseEvent" | "Input.insertText" | "Page.enable" => {
                            json!({})
                        }
                        _ => json!({}),
                    };
                    let response = json!({"id": id, "result": result});
                    let _ = socket.send(Message::Text(response.to_string().into()));
                }
            }
        });

        let deadline = SystemTime::now() + Duration::from_secs(1);
        loop {
            if TcpStream::connect(addr).is_ok() {
                break;
            }
            if SystemTime::now() > deadline {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        (format!("ws://{}/devtools/page/mock-1", addr), calls)
    }
}

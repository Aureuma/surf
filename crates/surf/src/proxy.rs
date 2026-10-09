use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use reqwest::Method;
use tiny_http::{Header, Request, Response, Server, StatusCode};
use url::Url;

/// Authenticated MCP gateway.
/// - `/mcp` (and `/sse`) route to agent slot 0 and require the main token.
/// - `/agent/<name>/mcp` (and `/sse`) route to that agent's slot and require
///   the agent's token from `[proxy.tokens]`.
///
/// Tokens arrive via `Authorization: Bearer` or `?token=` and are stripped
/// before forwarding upstream.
pub struct ProxyConfig {
    pub bind: String,
    pub port: u16,
    /// Upstream base URL for slot 0; slot N uses base port + N.
    pub upstream: String,
    pub main_token: String,
    pub agents: Vec<String>,
    pub tokens: HashMap<String, String>,
}

pub struct ProxyRoute {
    /// Agent name, or None for the default `/mcp` path.
    pub agent: Option<String>,
    pub slot: usize,
    /// Path to forward upstream (agent prefix stripped).
    pub forward_path: String,
}

pub fn run_proxy(config: &ProxyConfig) -> Result<()> {
    let main_token = config.main_token.trim().to_owned();
    if main_token.is_empty() {
        bail!("proxy auth token is required");
    }
    if config.agents.is_empty() {
        bail!("proxy requires at least one agent");
    }
    let upstream_base = Url::parse(config.upstream.trim()).context("invalid upstream url")?;
    let base_port = upstream_base
        .port()
        .ok_or_else(|| anyhow::anyhow!("upstream url must include an explicit port"))?;
    let server = Server::http(format!("{}:{}", config.bind.trim(), config.port))
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("bind proxy server")?;
    println!(
        "surf proxy: listening on http://{}:{} -> {} (auth required, {} agent(s))",
        config.bind.trim(),
        config.port,
        upstream_base,
        config.agents.len()
    );

    let client = reqwest::blocking::Client::builder().build()?;
    for request in server.incoming_requests() {
        if let Err(error) = proxy_request(
            request,
            &client,
            &upstream_base,
            base_port,
            &main_token,
            &config.agents,
            &config.tokens,
        ) {
            eprintln!("proxy error: {error}");
        }
    }
    Ok(())
}

/// Map a request path to an agent slot and upstream path.
/// Returns None for unknown agents or paths outside `/mcp`, `/sse`.
pub fn route_for_path(url_path: &str, agents: &[String]) -> Option<ProxyRoute> {
    let base = url_path.split('?').next().unwrap_or("");
    if let Some(rest) = base.strip_prefix("/agent/") {
        let (name, forward) = match rest.split_once('/') {
            Some((name, tail)) => (name, format!("/{tail}")),
            None => (rest, "/mcp".to_owned()),
        };
        if name.is_empty() {
            return None;
        }
        let slot = agents.iter().position(|a| a == name)?;
        if forward != "/mcp"
            && !forward.starts_with("/mcp/")
            && forward != "/sse"
            && !forward.starts_with("/sse/")
        {
            return None;
        }
        return Some(ProxyRoute {
            agent: Some(name.to_owned()),
            slot,
            forward_path: forward,
        });
    }
    if base == "/mcp" || base.starts_with("/mcp/") || base == "/sse" || base.starts_with("/sse/") {
        return Some(ProxyRoute {
            agent: None,
            slot: 0,
            forward_path: base.to_owned(),
        });
    }
    None
}

pub fn upstream_for_slot(base: &Url, base_port: u16, slot: usize) -> Result<Url> {
    let mut url = base.clone();
    let port = base_port as usize + slot;
    if port > u16::MAX as usize {
        bail!("agent slot {slot} exceeds port range");
    }
    url.set_port(Some(port as u16))
        .map_err(|_| anyhow::anyhow!("cannot set upstream port"))?;
    Ok(url)
}

fn proxy_request(
    mut request: Request,
    client: &reqwest::blocking::Client,
    upstream_base: &Url,
    base_port: u16,
    main_token: &str,
    agents: &[String],
    tokens: &HashMap<String, String>,
) -> Result<()> {
    let Some(route) = route_for_path(request.url(), agents) else {
        respond_text(request, StatusCode(404), "not found\n")?;
        return Ok(());
    };
    let expected = match route.agent.as_deref() {
        None => Some(main_token.to_owned()),
        Some(name) => tokens.get(name).map(|t| t.trim().to_owned()),
    };
    let Some(expected) = expected.filter(|t| !t.is_empty()) else {
        // Agent without a configured token stays locked.
        respond_text(request, StatusCode(401), "unauthorized\n")?;
        return Ok(());
    };

    let authorization = request
        .headers()
        .iter()
        .find(|header| {
            header
                .field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case("authorization")
        })
        .map(|header| header.value.as_str().to_owned());
    if presented_token(authorization.as_deref(), request.url()).as_deref()
        != Some(expected.as_str())
    {
        respond_text(request, StatusCode(401), "unauthorized\n")?;
        return Ok(());
    }

    let upstream = upstream_for_slot(upstream_base, base_port, route.slot)?;
    let stripped = strip_token_param(request.url());
    let query = stripped.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut forward = route.forward_path;
    if !query.is_empty() {
        forward.push('?');
        forward.push_str(query);
    }
    let target = if forward.starts_with('/') {
        upstream.join(forward.trim_start_matches('/'))?
    } else {
        upstream.join(&forward)?
    };

    let method = Method::from_bytes(request.method().as_str().as_bytes())?;
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .context("read proxy request body")?;

    let mut builder = client.request(method, target);
    for header in request.headers() {
        let name = header.field.as_str().to_string();
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "authorization"
        ) {
            continue;
        }
        builder = builder.header(name, header.value.as_str());
    }
    let upstream_response = builder.body(body).send().context("send upstream request")?;
    let status = StatusCode(upstream_response.status().as_u16());
    let content_length = upstream_response
        .content_length()
        .map(|value| value as usize);
    let mut headers = Vec::new();
    for (name, value) in upstream_response.headers() {
        if let Ok(header) = Header::from_bytes(name.as_str(), value.as_bytes()) {
            headers.push(header);
        }
    }
    let response = Response::new(status, headers, upstream_response, content_length, None);
    request
        .respond(response)
        .context("respond to proxy client")?;
    Ok(())
}

fn respond_text(request: Request, status: StatusCode, body: &str) -> Result<()> {
    let response = Response::new(
        status,
        Vec::<Header>::new(),
        body.as_bytes(),
        Some(body.len()),
        None,
    );
    request
        .respond(response)
        .context("respond to proxy client")?;
    Ok(())
}

/// Token presented via `Authorization: Bearer` header (preferred) or the
/// `token` query parameter (fallback for header-less MCP clients).
pub fn presented_token(authorization: Option<&str>, path: &str) -> Option<String> {
    if let Some(value) = authorization.map(str::trim).filter(|v| !v.is_empty()) {
        let token = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty());
        if let Some(token) = token {
            return Some(token.to_owned());
        }
    }
    query_token(path)
}

pub fn query_token(path: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

pub fn strip_token_param(path: &str) -> String {
    let Some((base, query)) = path.split_once('?') else {
        return path.to_owned();
    };
    let kept: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key != "token")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if kept.is_empty() {
        return base.to_owned();
    }
    let query = kept
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}

#[cfg(test)]
mod tests {
    use super::{
        presented_token, query_token, route_for_path, strip_token_param, upstream_for_slot,
    };
    use url::Url;

    #[test]
    fn presented_token_prefers_bearer_header() {
        assert_eq!(
            presented_token(Some("Bearer abc123"), "/mcp"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            presented_token(Some("bearer abc123"), "/mcp"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            presented_token(Some("Bearer abc123"), "/mcp?token=other"),
            Some("abc123".to_owned())
        );
        assert_eq!(presented_token(Some("Basic abc"), "/mcp"), None);
        assert_eq!(presented_token(Some("Bearer "), "/mcp"), None);
        assert_eq!(presented_token(None, "/mcp"), None);
    }

    #[test]
    fn presented_token_falls_back_to_query_param() {
        assert_eq!(
            presented_token(None, "/mcp?token=abc123"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            presented_token(Some("Basic abc"), "/mcp?token=abc123"),
            Some("abc123".to_owned())
        );
        assert_eq!(presented_token(None, "/mcp?token="), None);
        assert_eq!(presented_token(None, "/mcp?other=1"), None);
    }

    #[test]
    fn strip_token_param_removes_only_token() {
        assert_eq!(strip_token_param("/mcp"), "/mcp");
        assert_eq!(strip_token_param("/mcp?token=abc"), "/mcp");
        assert_eq!(strip_token_param("/mcp?token=abc&foo=1"), "/mcp?foo=1");
        assert_eq!(strip_token_param("/mcp?foo=1&token=abc"), "/mcp?foo=1");
    }

    #[test]
    fn query_token_reads_token_param() {
        assert_eq!(query_token("/mcp?token=abc"), Some("abc".to_owned()));
        assert_eq!(query_token("/mcp"), None);
    }

    #[test]
    fn route_for_path_maps_default_and_agent_paths() {
        let agents = vec!["alice".to_owned(), "bob".to_owned()];

        let route = route_for_path("/mcp", &agents).unwrap();
        assert!(route.agent.is_none());
        assert_eq!(route.slot, 0);
        assert_eq!(route.forward_path, "/mcp");

        let route = route_for_path("/mcp?token=x", &agents).unwrap();
        assert!(route.agent.is_none());
        assert_eq!(route.forward_path, "/mcp");

        let route = route_for_path("/agent/bob/mcp", &agents).unwrap();
        assert_eq!(route.agent.as_deref(), Some("bob"));
        assert_eq!(route.slot, 1);
        assert_eq!(route.forward_path, "/mcp");

        let route = route_for_path("/agent/alice/sse/123", &agents).unwrap();
        assert_eq!(route.slot, 0);
        assert_eq!(route.forward_path, "/sse/123");

        assert!(route_for_path("/agent/carol/mcp", &agents).is_none());
        assert!(route_for_path("/agent//mcp", &agents).is_none());
        assert!(route_for_path("/agent/alice/other", &agents).is_none());
        assert!(route_for_path("/vnc.html", &agents).is_none());
        assert!(route_for_path("/", &agents).is_none());
    }

    #[test]
    fn upstream_for_slot_offsets_base_port() {
        let base = Url::parse("http://127.0.0.1:8932").unwrap();
        assert_eq!(
            upstream_for_slot(&base, 8932, 0).unwrap().as_str(),
            "http://127.0.0.1:8932/"
        );
        assert_eq!(
            upstream_for_slot(&base, 8932, 2).unwrap().as_str(),
            "http://127.0.0.1:8934/"
        );
    }
}

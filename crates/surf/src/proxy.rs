use anyhow::{Context, Result};
use reqwest::Method;
use tiny_http::{Header, Request, Response, Server, StatusCode};
use url::Url;

pub fn run_proxy(bind: &str, port: u16, upstream: &str) -> Result<()> {
    let upstream_url = Url::parse(upstream.trim()).context("invalid upstream url")?;
    let server = Server::http(format!("{}:{}", bind.trim(), port))
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("bind proxy server")?;
    println!(
        "surf proxy: listening on http://{}:{} -> {}",
        bind.trim(),
        port,
        upstream_url
    );

    let client = reqwest::blocking::Client::builder().build()?;
    for request in server.incoming_requests() {
        if let Err(error) = proxy_request(request, &client, &upstream_url) {
            eprintln!("proxy error: {error}");
        }
    }
    Ok(())
}

fn proxy_request(
    mut request: Request,
    client: &reqwest::blocking::Client,
    upstream: &Url,
) -> Result<()> {
    let rewritten = rewrite_proxy_path(request.method().as_str(), request.url());
    let target = if rewritten.starts_with('/') {
        upstream.join(rewritten.trim_start_matches('/'))?
    } else {
        upstream.join(&rewritten)?
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
            "host" | "content-length"
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

pub fn rewrite_proxy_path(method: &str, path: &str) -> String {
    if method.eq_ignore_ascii_case("GET") {
        if path == "/mcp" {
            return "/sse".to_owned();
        }
        if let Some(rest) = path.strip_prefix("/mcp/") {
            return format!("/sse/{rest}");
        }
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::rewrite_proxy_path;

    #[test]
    fn rewrites_get_mcp_paths() {
        assert_eq!(rewrite_proxy_path("GET", "/mcp"), "/sse");
        assert_eq!(rewrite_proxy_path("GET", "/mcp/foo"), "/sse/foo");
        assert_eq!(rewrite_proxy_path("POST", "/mcp"), "/mcp");
    }
}

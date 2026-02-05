use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::header::{HOST, SET_COOKIE};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::{auth, APP_CORE_JS, APP_CSS, INDEX_HTML, GatewayState};

#[derive(Debug, Deserialize)]
pub(crate) struct AuthQuery {
    pub(crate) token: Option<String>,
}

pub(crate) async fn index(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Response {
    let html = if let Some(dev) = state.webui_dev.as_ref() {
        let host = request_host(&headers, &state.advertised_host);
        let scheme = request_scheme(&headers);
        let origin = dev.origin_for(&host, scheme);
        dev_index_html(&origin)
    } else {
        INDEX_HTML.to_string()
    };
    let mut response = Html(html).into_response();
    if let Some(token) = query.token.as_deref() {
        let encoded = auth::encode_cookie_value(token);
        let cookie = format!("codeGatewayToken={encoded}; Path=/; SameSite=Lax");
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(SET_COOKIE, value);
        }
    }
    apply_no_cache(&mut response);
    response
}

pub(crate) async fn app_css() -> Response {
    let mut response = Response::new(APP_CSS.into());
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    apply_no_cache(&mut response);
    response
}

pub(crate) async fn app_core_js() -> Response {
    js_response(APP_CORE_JS)
}

fn js_response(body: &'static str) -> Response {
    let mut response = Response::new(body.into());
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/javascript; charset=utf-8"),
    );
    apply_no_cache(&mut response);
    response
}

fn apply_no_cache(response: &mut Response) {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response
        .headers_mut()
        .insert(axum::http::header::PRAGMA, HeaderValue::from_static("no-cache"));
}

fn dev_index_html(origin: &str) -> String {
    let origin = origin.trim_end_matches('/');
    let base = "/assets";
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Every Code</title>
    <script>
      (function () {{
        var saved = localStorage.getItem("codeTheme");
        document.documentElement.dataset.theme = saved || "dark";
      }})();
    </script>
    <script type="module" src="{origin}{base}/@vite/client"></script>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="{origin}{base}/src/main.ts"></script>
  </body>
</html>
"#
    )
}

fn request_host(headers: &HeaderMap, fallback: &str) -> String {
    let forwarded = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let host = forwarded.or_else(|| {
        headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    let Some(raw_host) = host else {
        return fallback.to_string();
    };
    normalize_host(raw_host)
}

fn request_scheme(headers: &HeaderMap) -> &str {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| *value == "https")
        .unwrap_or("http")
}

fn normalize_host(raw_host: &str) -> String {
    if raw_host.starts_with('[') {
        if let Some(end) = raw_host.find(']') {
            return raw_host[..=end].to_string();
        }
    }
    if raw_host.matches(':').count() == 1 {
        if let Some((host, _port)) = raw_host.rsplit_once(':') {
            return host.to_string();
        }
    }
    raw_host.to_string()
}

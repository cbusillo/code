use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::{error_response, GatewayState};

pub(crate) fn enforce_auth(
    state: &GatewayState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), Response> {
    let Some(expected) = state.token.as_ref() else {
        return Ok(());
    };
    if expected.is_empty() {
        return Ok(());
    }

    let header_token = extract_bearer_token(headers).or_else(|| {
        headers
            .get("x-code-token")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string())
    });
    let cookie_token = extract_cookie_token(headers, "codeGatewayToken");
    if header_token.as_deref() == Some(expected)
        || query_token == Some(expected)
        || cookie_token.as_deref() == Some(expected)
    {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized".to_string(),
        ))
    }
}

pub(crate) fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get(axum::http::header::AUTHORIZATION)?;
    let auth = auth.to_str().ok()?;
    let mut parts = auth.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if scheme.eq_ignore_ascii_case("bearer") {
        Some(token.to_string())
    } else {
        None
    }
}

pub(crate) fn extract_cookie_token(headers: &HeaderMap, key: &str) -> Option<String> {
    let cookies = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())?;
    for chunk in cookies.split(';') {
        let mut parts = chunk.trim().splitn(2, '=');
        let name = parts.next()?.trim();
        let value = parts.next().unwrap_or("").trim();
        if name == key {
            return Some(value.to_string());
        }
    }
    None
}

pub(crate) fn encode_cookie_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else {
            out.push('%');
            out.push(hex_char(byte >> 4));
            out.push(hex_char(byte & 0x0f));
        }
    }
    out
}

fn hex_char(nibble: u8) -> char {
    match nibble & 0x0f {
        0..=9 => (b'0' + (nibble & 0x0f)) as char,
        10..=15 => (b'A' + ((nibble & 0x0f) - 10)) as char,
        _ => '0',
    }
}

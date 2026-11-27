//! HTTP requests with `fetch` (SPEC.md 12.9.1): rustls with the operating
//! system's certificate store, redirects, proxies from the environment.

use std::collections::HashMap;
use std::io::Read;
use std::time::{Duration, Instant};

use ureq::ResponseExt;

use crate::builtins::{arg_err, eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::formats::to_json;
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{bool_named, byte_size, count_args, limit_named, named};
use crate::path::OkPath;
use crate::value::{Record, Value};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("fetch", fetch),
];

/// Bytes of the body quoted in an HTTP error.
const ERROR_BODY_PREVIEW: usize = 500;

/// Redirects `fetch` follows before it gives up.
const MAX_REDIRECTS: usize = 10;
/// Default for `max_size:`: the largest response body read into memory.
/// Downloads with `to:` have no limit unless `max_size:` is given.
const DEFAULT_MAX_SIZE: u64 = 1 << 30;

/// The agent for every request. It follows no redirects itself (`fetch`
/// does, to control what reaches another host), and each request sets its
/// own timeout from what is left of the whole call's.
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .tls_config(tls)
        .proxy(ureq::Proxy::try_from_env())
        .user_agent(format!("ok/{}", crate::VERSION))
        .build()
        .into()
}

fn net_err(args: &Args, msg: String) -> crate::interp::Flow {
    OkError::at(ErrKind::Net, args.span, msg).into()
}

fn fetch(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts: HashMap<String, Value> = named(
        &mut args,
        "fetch",
        &[
            "method", "headers", "body", "json", "timeout", "to", "check", "bytes", "max_size",
        ],
    )?;
    count_args(&args, "fetch", 1, 1)?;
    // WASI preview 1 has no sockets (SPEC.md 19).
    if cfg!(target_os = "wasi") {
        return Err(net_err(
            &args,
            "the WebAssembly build of ok cannot open network connections".into(),
        ));
    }
    let Value::Str(url) = &args.pos[0] else {
        return type_err(
            &args,
            format!("`fetch` needs a URL string, got {}", args.pos[0].a_type()),
        );
    };
    let url = url.to_string();
    // Messages, traces and dry runs show the URL without its password.
    let shown = crate::command::redact_url(&url).into_owned();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return arg_err(
            &args,
            format!("`fetch` needs an http:// or https:// URL, got {shown:?}"),
        );
    }
    let mut method = match opts.get("method") {
        None => "GET".to_string(),
        Some(Value::Str(m)) => m.to_ascii_uppercase(),
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "`method` must be a string like \"POST\", got {}",
                    other.a_type()
                ),
            );
        }
    };
    let timeout = match opts.get("timeout") {
        None => Duration::from_secs(30),
        Some(Value::Duration(d)) if *d > 0 => Duration::from_nanos(*d as u64),
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "`timeout` must be a positive duration, got {}",
                    other.repr()
                ),
            );
        }
    };
    let check = match opts.get("check") {
        None => true,
        _ => bool_named(&opts, "check", &args)?,
    };
    let want_bytes = bool_named(&opts, "bytes", &args)?;
    let to = match opts.get("to") {
        None | Some(Value::Null) => None,
        Some(Value::Str(s)) => Some(OkPath::parse(s)),
        Some(Value::Path(p)) => Some((**p).clone()),
        Some(other) => {
            return type_err(
                &args,
                format!("`to` must be a path, got {}", other.a_type()),
            );
        }
    };
    let max_size = if to.is_some() && !opts.contains_key("max_size") {
        None
    } else {
        limit_named(&opts, "max_size", DEFAULT_MAX_SIZE, &args)?
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    match opts.get("headers") {
        None | Some(Value::Null) => {}
        Some(Value::Record(r)) => {
            for (k, v) in r.iter() {
                match v {
                    Value::Str(s) => headers.push((k.to_string(), s.to_string())),
                    Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
                        headers.push((k.to_string(), v.display()))
                    }
                    other => {
                        return type_err(
                            &args,
                            format!("header {k} must be a string, got {}", other.a_type()),
                        );
                    }
                }
            }
        }
        Some(other) => {
            return type_err(
                &args,
                format!("`headers` must be a record, got {}", other.a_type()),
            );
        }
    }
    let mut body: Option<Vec<u8>> = match (opts.get("body"), opts.get("json")) {
        (Some(_), Some(_)) => return arg_err(&args, "pass either `body` or `json`, not both"),
        (Some(Value::Str(s)), None) => Some(s.as_bytes().to_vec()),
        (Some(Value::Bytes(b)), None) => Some(b.to_vec()),
        (Some(Value::Null), None) | (None, None) => None,
        (Some(other), None) => {
            return type_err(
                &args,
                format!("`body` must be a string or bytes, got {}", other.a_type()),
            );
        }
        (None, Some(v)) => {
            let mut s = String::new();
            to_json(v, &mut s, None, 0).map_err(|m| OkError::at(ErrKind::Format, args.span, m))?;
            if !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                headers.push(("content-type".into(), "application/json".into()));
            }
            Some(s.into_bytes())
        }
    };

    let mutating = !matches!(method.as_str(), "GET" | "HEAD") || to.is_some();
    let what = match &to {
        Some(p) => format!(
            "fetch {method} {} > {}",
            crate::command::display_arg(&url),
            crate::command::display_arg(&p.display())
        ),
        None => format!("fetch {method} {}", crate::command::display_arg(&url)),
    };
    let proceed = if mutating {
        interp.mutation(&what)
    } else {
        true
    };
    if !proceed {
        let mut r = Record::new();
        r.insert("status", Value::Int(0));
        r.insert("ok", Value::Bool(true));
        r.insert("headers", Value::record(Record::new()));
        r.insert("body", Value::Null);
        r.insert("url", Value::str(url.as_str()));
        return Ok(Value::record(r));
    }

    match (method.as_str(), &body) {
        ("GET" | "HEAD" | "DELETE", None) | ("POST" | "PUT" | "PATCH", _) => {}
        (m @ ("GET" | "HEAD" | "DELETE"), Some(_)) => {
            return arg_err(&args, format!("a {m} request cannot have a body"));
        }
        (other, _) => {
            return arg_err(
                &args,
                format!(
                    "unsupported HTTP method {other}; use GET, HEAD, POST, PUT, PATCH or DELETE"
                ),
            );
        }
    }
    // A download should land as the server stores it, so it asks for no
    // content encoding (a `.tar.gz` sent with `content-encoding: gzip`
    // would otherwise be saved decompressed).
    if to.is_some()
        && !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"))
    {
        headers.push(("accept-encoding".into(), "identity".into()));
    }

    // Redirects are followed here rather than by ureq, which forwards every
    // header except `authorization` and `cookie` to another host, and
    // follows https to http.
    let agent = agent();
    let deadline = Instant::now() + timeout;
    let mut current = url.clone();
    let mut hops = 0;
    let mut resp = loop {
        let failed = |e: &ureq::Error| {
            let here = crate::command::redact_url(&current).into_owned();
            net_err(
                &args,
                format!(
                    "request to {here} failed: {}",
                    describe(e).replace(&current, &here)
                ),
            )
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(failed(&ureq::Error::Timeout(ureq::Timeout::Global)));
        }
        let resp = send(
            &agent,
            &method,
            &current,
            &headers,
            body.as_deref(),
            remaining,
        )
        .map_err(|e| failed(&e))?;
        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let (true, Some(location)) = (matches!(status, 301 | 302 | 303 | 307 | 308), location)
        else {
            break resp;
        };
        hops += 1;
        let here = crate::command::redact_url(&current).into_owned();
        if hops > MAX_REDIRECTS {
            return Err(net_err(
                &args,
                format!("request to {shown} failed: more than {MAX_REDIRECTS} redirects"),
            ));
        }
        let Some(next) = resolve_location(&current, &location) else {
            return Err(net_err(
                &args,
                format!(
                    "{here} redirects to {}, which is not an http:// or https:// URL",
                    crate::command::redact_url(&location)
                ),
            ));
        };
        if is_https(&current) && !is_https(&next) {
            return Err(OkError::at(
                ErrKind::Net,
                args.span,
                format!(
                    "{here} redirects from https to {}; fetch does not follow a redirect to plain http",
                    crate::command::redact_url(&next)
                ),
            )
            .with_help("fetch the http:// URL directly if you trust it")
            .into());
        }
        let keep_body =
            matches!(status, 307 | 308) || (matches!(status, 301 | 302) && method != "POST");
        if !keep_body {
            if method != "HEAD" {
                method = "GET".into();
            }
            body = None;
        }
        if origin(&next) != origin(&current) {
            // Headers may carry credentials (`authorization`, `x-api-key`,
            // `private-token`, ...), which another host must not see. Only
            // the description of a body that is sent again stays.
            headers.retain(|(k, _)| {
                k.eq_ignore_ascii_case("accept-encoding")
                    || (body.is_some() && k.eq_ignore_ascii_case("content-type"))
            });
        } else if body.is_none() {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
        }
        current = next;
    };
    let status = resp.status().as_u16();
    let hdrs = response_headers(resp.headers());
    let final_url = resp.get_uri().to_string();
    let ok = (200..400).contains(&status);
    let too_large = |max: u64| {
        crate::interp::Flow::from(
            OkError::at(
                ErrKind::Net,
                args.span,
                format!(
                    "the response from {shown} is larger than {}",
                    byte_size(max)
                ),
            )
            .with_help("pass max_size: (in bytes) to allow more, or null for no limit"),
        )
    };
    let body_value = if let (Some(dest), true) = (&to, ok) {
        let native = dest.native();
        let write_fail = |e: std::io::Error| {
            net_err(
                &args,
                format!("cannot write {}: {}", dest.display(), io_reason(&e)),
            )
        };
        // The download goes to a temporary file first, so a broken
        // connection leaves an earlier copy of the file intact.
        let (pending, mut file) = crate::files::ensure_parent(&native)
            .and_then(|()| crate::files::Replacement::create(&native))
            .map_err(write_fail)?;
        // Reading one byte more than `max_size` allows is enough to tell
        // that the body is too large, without reading the rest of it.
        let reader = resp.body_mut().with_config().limit(u64::MAX).reader();
        let n = std::io::copy(
            &mut reader.take(max_size.map_or(u64::MAX, |m| m.saturating_add(1))),
            &mut file,
        )
        .map_err(|e| net_err(&args, format!("download of {shown} failed: {e}")))?;
        if let Some(max) = max_size
            && n > max
        {
            return Err(too_large(max));
        }
        drop(file);
        pending.commit().map_err(write_fail)?;
        Value::Null
    } else {
        let mut buf = Vec::new();
        resp.body_mut()
            .with_config()
            .limit(u64::MAX)
            .reader()
            .take(max_size.map_or(u64::MAX, |m| m.saturating_add(1)))
            .read_to_end(&mut buf)
            .map_err(|e| {
                net_err(
                    &args,
                    format!("reading the response from {shown} failed: {e}"),
                )
            })?;
        if let Some(max) = max_size
            && buf.len() as u64 > max
        {
            return Err(too_large(max));
        }
        if !ok && check {
            let preview = String::from_utf8_lossy(&buf[..buf.len().min(ERROR_BODY_PREVIEW)])
                .trim()
                .to_string();
            let mut e = OkError::at(
                ErrKind::Net,
                args.span,
                format!("{method} {shown} returned HTTP {status}"),
            );
            e.status = Some(status);
            if !preview.is_empty() {
                e.details.push(format!("body: {preview}"));
            }
            return Err(e
                .with_help("pass check: false to get the response instead of an error")
                .into());
        }
        if want_bytes {
            Value::Bytes(buf.into())
        } else {
            match String::from_utf8(buf) {
                Ok(s) => Value::str(s),
                Err(_) => {
                    return Err(net_err(
                        &args,
                        format!("the response from {shown} is not UTF-8 text; pass bytes: true",),
                    ));
                }
            }
        }
    };
    let mut r = Record::new();
    r.insert("status", Value::Int(status as i64));
    r.insert("ok", Value::Bool(ok));
    r.insert("headers", Value::record(hdrs));
    r.insert("body", body_value);
    r.insert("url", Value::str(final_url));
    Ok(Value::record(r))
}

/// Send one request, without following redirects.
fn send(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    fn plain(
        req: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
        headers: &[(String, String)],
        timeout: Duration,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let mut req = req.config().timeout_global(Some(timeout)).build();
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req.call()
    }
    fn with_body(
        req: ureq::RequestBuilder<ureq::typestate::WithBody>,
        headers: &[(String, String)],
        timeout: Duration,
        body: &[u8],
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let mut req = req.config().timeout_global(Some(timeout)).build();
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req.send(body)
    }
    let body = body.unwrap_or_default();
    match method {
        "GET" => plain(agent.get(url), headers, timeout),
        "HEAD" => plain(agent.head(url), headers, timeout),
        "DELETE" => plain(agent.delete(url), headers, timeout),
        "POST" => with_body(agent.post(url), headers, timeout, body),
        "PUT" => with_body(agent.put(url), headers, timeout, body),
        _ => with_body(agent.patch(url), headers, timeout, body),
    }
}

/// The response headers as a record with lowercase names. A header that
/// occurs several times has its values joined with `, ` (RFC 9110 5.3),
/// except `set-cookie`, whose values may contain commas: it is always a
/// list of strings.
fn response_headers(map: &ureq::http::HeaderMap) -> Record {
    let mut r = Record::new();
    for name in map.keys() {
        let values: Vec<String> = map
            .get_all(name)
            .iter()
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
            .collect();
        let key = name.as_str().to_ascii_lowercase();
        let value = if key == "set-cookie" {
            Value::list(values.into_iter().map(Value::str).collect())
        } else {
            Value::str(values.join(", "))
        };
        r.insert(key.as_str(), value);
    }
    r
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
}

/// Split an http(s) URL into scheme (lowercase), authority and the rest
/// (path, query and fragment).
fn split_url(url: &str) -> Option<(String, &str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some((scheme, &rest[..end], &rest[end..]))
}

/// Scheme, host and port of a URL: two URLs with the same origin may share
/// headers on a redirect.
fn origin(url: &str) -> Option<(String, String, u16)> {
    let (scheme, authority, _) = split_url(url)?;
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = match hostport.rfind(':') {
        Some(i) if !hostport[i..].contains(']') => (&hostport[..i], hostport[i + 1..].parse().ok()),
        _ => (hostport, None),
    };
    let default = if scheme == "https" { 443 } else { 80 };
    Some((scheme, host.to_ascii_lowercase(), port.unwrap_or(default)))
}

/// The URL a `location` header names, relative to `base`, without its
/// fragment; `None` when it is not http or https.
fn resolve_location(base: &str, location: &str) -> Option<String> {
    let location = location.trim();
    let location = location.split('#').next().unwrap_or(location);
    let (scheme, authority, rest) = split_url(base)?;
    let out = if location.contains("://") && !location.starts_with('/') {
        location.to_string()
    } else if let Some(r) = location.strip_prefix("//") {
        format!("{scheme}://{r}")
    } else if location.starts_with('/') {
        format!("{scheme}://{authority}{location}")
    } else {
        let path = rest.split(['?', '#']).next().unwrap_or("");
        if location.starts_with('?') {
            format!("{scheme}://{authority}{path}{location}")
        } else {
            let dir = path.rfind('/').map_or("/", |i| &path[..=i]);
            format!("{scheme}://{authority}{dir}{location}")
        }
    };
    if split_url(&out).is_some() {
        Some(out)
    } else {
        None
    }
}

fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "timed out".into(),
        ureq::Error::HostNotFound => "host not found".into(),
        ureq::Error::Io(io) => io_reason(io),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_resolve_against_the_current_url() {
        let r = |b: &str, l: &str| resolve_location(b, l);
        let base = "https://a.example/x/y?q=1";
        assert_eq!(r(base, "/z").as_deref(), Some("https://a.example/z"));
        assert_eq!(r(base, "z#frag").as_deref(), Some("https://a.example/x/z"));
        assert_eq!(
            r(base, "?p=2").as_deref(),
            Some("https://a.example/x/y?p=2")
        );
        assert_eq!(
            r(base, "//b.example/w").as_deref(),
            Some("https://b.example/w")
        );
        assert_eq!(
            r(base, "HTTP://b.example").as_deref(),
            Some("HTTP://b.example")
        );
        assert_eq!(r("http://h", "z").as_deref(), Some("http://h/z"));
        assert_eq!(r(base, "file:///etc/passwd"), None);
        assert_eq!(r(base, "ftp://h/x"), None);
    }

    #[test]
    fn origins_compare_scheme_host_and_port() {
        assert_eq!(
            origin("https://A.example/x"),
            origin("https://a.example:443/y")
        );
        assert_ne!(origin("https://a.example"), origin("http://a.example"));
        assert_ne!(origin("http://a.example:8080"), origin("http://a.example"));
        assert_ne!(origin("http://u:p@a.example"), origin("http://b.example"));
        assert_eq!(
            origin("http://[::1]:8080/x"),
            Some(("http".into(), "[::1]".into(), 8080))
        );
        assert_eq!(origin("http://[::1]/x").map(|o| o.2), Some(80));
    }
}

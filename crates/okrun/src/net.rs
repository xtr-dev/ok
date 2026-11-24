//! HTTP requests with `fetch` (SPEC.md 12.9.1): rustls with the operating
//! system's certificate store, redirects, proxies from the environment.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use ureq::ResponseExt;

use crate::builtins::{arg_err, eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::formats::to_json;
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{bool_named, count_args, named};
use crate::path::OkPath;
use crate::value::{Record, Value};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("fetch", fetch),
];

/// Bytes of the body quoted in an HTTP error.
const ERROR_BODY_PREVIEW: usize = 500;

fn agent(timeout: Duration) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(10)
        .timeout_global(Some(timeout))
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
            "method", "headers", "body", "json", "timeout", "to", "check", "bytes",
        ],
    )?;
    count_args(&args, "fetch", 1, 1)?;
    let Value::Str(url) = &args.pos[0] else {
        return type_err(
            &args,
            format!("`fetch` needs a URL string, got {}", args.pos[0].a_type()),
        );
    };
    let url = url.to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return arg_err(
            &args,
            format!("`fetch` needs an http:// or https:// URL, got {url:?}"),
        );
    }
    let method = match opts.get("method") {
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
    let body: Option<Vec<u8>> = match (opts.get("body"), opts.get("json")) {
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
        Some(p) => format!("fetch {method} {url} > {}", p.display()),
        None => format!("fetch {method} {url}"),
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

    let agent = agent(timeout);
    let result = match (method.as_str(), body) {
        ("GET", None) => apply(agent.get(&url), &headers).call(),
        ("HEAD", None) => apply(agent.head(&url), &headers).call(),
        ("DELETE", None) => apply(agent.delete(&url), &headers).call(),
        ("POST", b) => apply_body(agent.post(&url), &headers).send(b.unwrap_or_default()),
        ("PUT", b) => apply_body(agent.put(&url), &headers).send(b.unwrap_or_default()),
        ("PATCH", b) => apply_body(agent.patch(&url), &headers).send(b.unwrap_or_default()),
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
    };
    let mut resp =
        result.map_err(|e| net_err(&args, format!("request to {url} failed: {}", describe(&e))))?;
    let status = resp.status().as_u16();
    let mut hdrs = Record::new();
    for (k, v) in resp.headers() {
        hdrs.insert(
            k.as_str().to_ascii_lowercase().as_str(),
            Value::str(String::from_utf8_lossy(v.as_bytes()).into_owned()),
        );
    }
    let final_url = resp.get_uri().to_string();
    let ok = (200..400).contains(&status);
    let body_value = if let (Some(dest), true) = (&to, ok) {
        let native = dest.native();
        crate::files::ensure_parent(&native).map_err(|e| {
            net_err(
                &args,
                format!("cannot write {}: {}", dest.display(), io_reason(&e)),
            )
        })?;
        let mut file = std::fs::File::create(&native).map_err(|e| {
            net_err(
                &args,
                format!("cannot write {}: {}", dest.display(), io_reason(&e)),
            )
        })?;
        let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
        std::io::copy(&mut reader, &mut file)
            .map_err(|e| net_err(&args, format!("download of {url} failed: {e}")))?;
        Value::Null
    } else {
        let mut buf = Vec::new();
        resp.body_mut()
            .with_config()
            .limit(u64::MAX)
            .reader()
            .read_to_end(&mut buf)
            .map_err(|e| {
                net_err(
                    &args,
                    format!("reading the response from {url} failed: {e}"),
                )
            })?;
        if !ok && check {
            let preview = String::from_utf8_lossy(&buf[..buf.len().min(ERROR_BODY_PREVIEW)])
                .trim()
                .to_string();
            let mut e = OkError::at(
                ErrKind::Net,
                args.span,
                format!("{method} {url} returned HTTP {status}"),
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
                        format!("the response from {url} is not UTF-8 text; pass bytes: true",),
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

fn apply<B>(
    mut req: ureq::RequestBuilder<B>,
    headers: &[(String, String)],
) -> ureq::RequestBuilder<B> {
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req
}

fn apply_body(
    req: ureq::RequestBuilder<ureq::typestate::WithBody>,
    headers: &[(String, String)],
) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
    apply(req, headers)
}

fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "timed out".into(),
        ureq::Error::HostNotFound => "host not found".into(),
        ureq::Error::Io(io) => io_reason(io),
        other => other.to_string(),
    }
}

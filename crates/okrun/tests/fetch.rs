//! `fetch` against small HTTP servers on 127.0.0.1 (SPEC.md 12.9.1): what
//! reaches another host on a redirect, size limits, downloads and repeated
//! response headers. No external network is used.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;

/// One request as the server saw it.
struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

/// Start a server that answers each request with `answer(request, port)`,
/// which returns the whole response (status line, headers and body).
fn server(answer: fn(&Req, u16) -> Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if r.read_line(&mut line).is_err() {
                    return;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut headers = Vec::new();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(':') {
                        headers.push((k.to_ascii_lowercase(), v.trim().to_string()));
                    }
                }
                let len: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0; len];
                let _ = r.read_exact(&mut body);
                let req = Req {
                    method,
                    path,
                    headers,
                };
                let mut s = stream;
                let _ = s.write_all(&answer(&req, port));
            });
        }
    });
    port
}

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

/// The request's method and headers, one per line, sorted, as the body.
fn echo(req: &Req) -> Vec<u8> {
    let mut lines: Vec<String> = req
        .headers
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    lines.sort();
    lines.insert(0, format!("{} {}", req.method, req.path));
    response("200 OK", &[], lines.join("\n").as_bytes())
}

fn ok(code: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("ok-fetch-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ok"))
        .args(["-e", code])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .output()
        .expect("run ok");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    )
}

#[test]
fn a_redirect_to_another_host_drops_the_request_headers() {
    let other = server(|req, _| echo(req));
    let first = server(|req, _| {
        if req.path.starts_with("/echo") {
            return echo(req);
        }
        // The port of the other server travels in the path.
        let to = req.path.trim_start_matches('/');
        let (status, target) = match to.split_once('/') {
            Some(("same", rest)) => ("307 Temporary Redirect", format!("/echo/{rest}")),
            Some(("see", port)) => ("303 See Other", format!("http://127.0.0.1:{port}/echo")),
            Some((port, _)) => ("302 Found", format!("http://127.0.0.1:{port}/echo")),
            None => return echo(req),
        };
        response(status, &[("location", &target)], b"")
    });
    let hdrs = "{\"x-api-key\": \"K\", \"private-token\": \"T\", \"proxy-authorization\": \"P\", authorization: \"A\"}";
    let (ok_, out, err) = ok(&format!(
        "print(fetch(\"http://127.0.0.1:{first}/{other}/x\", headers: {hdrs}).body)"
    ));
    assert!(ok_, "{err}");
    for secret in [
        "x-api-key",
        "private-token",
        "proxy-authorization",
        "authorization",
    ] {
        assert!(
            !out.contains(secret),
            "{secret} reached the other host:\n{out}"
        );
    }
    assert!(out.starts_with("GET /echo"), "{out}");
    // On the same host the headers stay, and 307 keeps the method and body.
    let (ok_, out, err) = ok(&format!(
        "print(fetch(\"http://127.0.0.1:{first}/same/x\", method: \"POST\", json: {{a: 1}}, headers: {hdrs}).body)"
    ));
    assert!(ok_, "{err}");
    assert!(out.starts_with("POST /echo/x"), "{out}");
    assert!(
        out.contains("x-api-key=K") && out.contains("content-type=application/json"),
        "{out}"
    );
    // 303 turns a POST into a GET without a body.
    let (ok_, out, err) = ok(&format!(
        "print(fetch(\"http://127.0.0.1:{first}/see/{other}\", method: \"POST\", body: \"data\").body)"
    ));
    assert!(ok_, "{err}");
    assert!(
        out.starts_with("GET /echo") && !out.contains("content-length=4"),
        "{out}"
    );
}

#[test]
fn redirect_loops_and_other_schemes_are_errors() {
    let port = server(|req, _| match req.path.as_str() {
        "/file" => response("302 Found", &[("location", "file:///etc/passwd")], b""),
        _ => response("302 Found", &[("location", "/loop")], b""),
    });
    let (ok_, _, err) = ok(&format!("fetch(\"http://127.0.0.1:{port}/loop\")"));
    assert!(!ok_ && err.contains("more than 10 redirects"), "{err}");
    let (ok_, _, err) = ok(&format!("fetch(\"http://127.0.0.1:{port}/file\")"));
    assert!(
        !ok_ && err
            .contains("redirects to file:///etc/passwd, which is not an http:// or https:// URL"),
        "{err}"
    );
}

#[test]
fn responses_are_limited_by_max_size() {
    let port = server(|_, _| response("200 OK", &[], &[b'y'; 5000]));
    let url = format!("http://127.0.0.1:{port}/big");
    let (ok_, _, err) = ok(&format!("fetch(\"{url}\", max_size: 4999)"));
    assert!(!ok_ && err.contains("is larger than 4999 bytes"), "{err}");
    assert!(err.contains("pass max_size:"), "{err}");
    let (ok_, out, err) = ok(&format!(
        "print(len(fetch(\"{url}\", max_size: 5000).body))"
    ));
    assert!(ok_ && out == "5000\n", "{err}");
    // Downloads have no default limit, but take one.
    let (ok_, _, err) = ok(&format!("fetch(\"{url}\", to: \"big.bin\", max_size: 100)"));
    assert!(!ok_ && err.contains("is larger than 100 bytes"), "{err}");
    let (ok_, out, err) = ok(&format!(
        "fetch(\"{url}\", to: \"big.bin\")\nprint(stat(\"big.bin\").size)"
    ));
    assert!(ok_ && out == "5000\n", "{err}");
}

#[test]
fn downloads_ask_for_the_file_as_stored() {
    let port = server(|req, _| echo(req));
    let (ok_, out, err) = ok(&format!(
        "fetch(\"http://127.0.0.1:{port}/f\", to: \"echo.txt\")\nprint(read(\"echo.txt\"))\nprint(fetch(\"http://127.0.0.1:{port}/g\").body)"
    ));
    assert!(ok_, "{err}");
    let (download, plain) = out.split_once("GET /g").unwrap();
    assert!(download.contains("accept-encoding=identity"), "{out}");
    assert!(!plain.contains("accept-encoding=identity"), "{out}");
}

#[test]
fn repeated_response_headers_are_all_kept() {
    let port = server(|_, _| {
        response(
            "200 OK",
            &[
                ("set-cookie", "a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT"),
                ("set-cookie", "b=2"),
                ("x-multi", "one"),
                ("x-multi", "two"),
                ("x-single", "only"),
            ],
            b"ok",
        )
    });
    let (ok_, out, err) = ok(&format!(
        "let h = fetch(\"http://127.0.0.1:{port}/\").headers\nprint(h[\"set-cookie\"])\nprint(h[\"x-multi\"], h[\"x-single\"])"
    ));
    assert!(ok_, "{err}");
    assert_eq!(
        out,
        "[\"a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT\", \"b=2\"]\none, two only\n"
    );
}

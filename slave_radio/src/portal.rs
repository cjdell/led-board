//! Captive-portal provisioning UI served by the C3 on the AP interface.
//!
//! Same JSON API as the Pico web app (`GET /api/wifi`, `POST /api/wifi`), plus
//! `/api/status` and `/api/forget`. Everything is hand-rolled HTTP/1.1 with
//! `Connection: close` — just enough for the page below and mobile probes.

use alloc::{boxed::Box, format, string::String, string::ToString as _, vec::Vec};

use radio_link::RadioMode;
use serde::Deserialize;
use serde_json::json;

use crate::httpd::{self, HTML, JSON};
use crate::state;

/// A parsed HTTP request head (+ body). `raw` keeps every byte consumed so the
/// AP acceptor can replay non-portal requests toward the Pico.
#[derive(Clone)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    pub content_length: usize,
    pub body: Vec<u8>,
    pub raw: Vec<u8>,
}

/// Parse a request head out of already-buffered bytes.
pub fn parse_head(raw: &[u8]) -> RequestHead {
    // Head-parse over BYTES up to the terminator only: a large POST's prefix
    // also carries binary body data, and from_utf8 on the whole buffer fails
    // on that — which once silently turned a `POST /api/ota-radio` into a
    // default "GET /" and relayed the firmware upload to the Pico (404).
    let head_len = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let text = core::str::from_utf8(&raw[..head_len]).unwrap_or("");
    let mut lines = text.split("\r\n");

    let request_line = lines.next().unwrap_or("GET /");
    let mut parts = request_line.split(' ');
    let method = String::from(parts.next().unwrap_or("GET")).to_uppercase();
    let path = String::from(parts.next().unwrap_or("/"));

    let mut content_length = 0usize;
    for line in lines {
        let (name, value) = match line.split_once(':') {
            Some(pair) => pair,
            None => continue,
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    let body = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| raw[pos + 4..].to_vec())
        .unwrap_or_default();

    RequestHead {
        method,
        path,
        content_length,
        body,
        raw: raw.to_vec(),
    }
}

/// Local portal routes. Returns `None` when the request should be relayed.
pub async fn route(head: &RequestHead) -> Option<&'static [u8]> {
    let path = head.path.split('?').next().unwrap_or("/").to_string();

    // When the board is up, the Pico's full web app owns the root and the
    // provisioning page lives at /setup. When it is down, the portal reclaims
    // the root: recovery users (and the captive redirect) must land on the one
    // page that works without the main MCU.
    let pico = state::pico_online();

    // Captive-detection probes → redirect (keeps phones popping up). Where the
    // popup lands depends on why the AP is up: an explicit AccessPoint mode
    // means the user asked for the local app, so go straight there; a
    // fallback AP means WiFi needs repair, so land on setup first — even
    // when the board is online.
    match path.as_str() {
        "/generate_204" | "/hotspot-detect.html" | "/connecttest.txt" | "/fwlink/" | "/redirect"
        | "/library/test/success.html" | "/success.txt" => {
            let mode = {
                let store = httpd::store();
                let mut store = store.lock().await;
                store.load().mode
            };
            let target = if matches!(mode, RadioMode::AccessPoint) && state::pico_online() {
                "/"
            } else {
                "/setup"
            };
            return Some(leak(httpd::redirect(&format!("http://{}{}", state::AP_IP, target))));
        }
        _ => {}
    }

    // CORS preflight for fetch() calls.
    if head.method == "OPTIONS" {
        return Some(leak(httpd::response("204 No Content", "text/plain", b"")));
    }

    match (head.method.as_str(), path.as_str()) {
        ("GET", "/setup") => Some(leak(httpd::response("200 OK", HTML, PORTAL_HTML.as_bytes()))),
        ("GET", "/") | ("GET", "/index.html") if !pico => Some(leak(httpd::response("200 OK", HTML, PORTAL_HTML.as_bytes()))),
        ("GET", "/favicon.ico") => Some(leak(httpd::response("204 No Content", "image/x-icon", b""))),
        ("GET", "/api/status") => Some(leak(status_json().await)),
        ("GET", "/api/wifi") => Some(leak(scan_json().await)),
        ("POST", "/api/wifi") => Some(leak(join_json(&head.body).await)),
        ("POST", "/api/forget") => Some(leak(forget_json(&head.body).await)),
        ("GET", "/api/networks") => Some(leak(networks_json().await)),
        ("GET", "/api/ota-radio") => Some(leak(crate::ota::status_json().await)),
        _ => None, // relay to the Pico
    }
}

// ---------------------------------------------------------------------------
// API implementations
// ---------------------------------------------------------------------------

async fn status_json() -> Vec<u8> {
    let status = state::snapshot().await;

    let body = match status {
        Some(status) => {
            json!({
                "mode": format!("{:?}", status.mode),
                "ssid": status.ssid,
                "ip": status.ip.map(|o| format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])),
                "ap_ssid": status.ap_ssid,
                "ap_ip": status.ap_ip.map(|o| format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])),
                "firmware": status.firmware,
                "pico_online": state::pico_online(),
            })
        }
        None => json!({
            "mode": "Station",
            "connecting": true,
            "pico_online": state::pico_online(),
        }),
    };

    httpd::response("200 OK", JSON, body.to_string().as_bytes())
}

async fn scan_json() -> Vec<u8> {
    let entries = crate::supervisor::request_scan().await;

    let rows: Vec<_> = entries
        .iter()
        .map(|e| {
            json!({
                "ssid": e.ssid,
                "signal_strength": e.signal_strength,
                "password_required": e.password_required,
            })
        })
        .collect();

    httpd::response("200 OK", JSON, json!(rows).to_string().as_bytes())
}

async fn networks_json() -> Vec<u8> {
    let store = httpd::store();
    let cfg = {
        let mut store = store.lock().await;
        store.load()
    };

    let rows: Vec<_> = cfg.networks.iter().map(|n| json!({ "ssid": n.ssid })).collect();
    httpd::response(
        "200 OK",
        JSON,
        json!({ "networks": rows, "ap_ssid": cfg.ap_ssid, "mode": format!("{:?}", cfg.mode) })
            .to_string()
            .as_bytes(),
    )
}

#[derive(Deserialize)]
struct JoinBody {
    ssid: String,
    #[serde(default)]
    pass: String,
}

#[derive(Deserialize)]
struct ForgetBody {
    ssid: String,
}

async fn join_json(body: &[u8]) -> Vec<u8> {
    let join: JoinBody = match serde_json::from_slice(body) {
        Ok(join) => join,
        Err(_) => {
            return httpd::response(
                "400 Bad Request",
                JSON,
                json!({ "error": "expected {ssid, pass}" }).to_string().as_bytes(),
            );
        }
    };

    crate::supervisor::join_network(&join.ssid, &join.pass).await;

    httpd::response(
        "200 OK",
        JSON,
        json!({ "ok": true, "connecting": join.ssid }).to_string().as_bytes(),
    )
}

async fn forget_json(body: &[u8]) -> Vec<u8> {
    let forget: ForgetBody = match serde_json::from_slice(body) {
        Ok(forget) => forget,
        Err(_) => {
            return httpd::response("400 Bad Request", JSON, json!({ "error": "expected {ssid}" }).to_string().as_bytes());
        }
    };

    crate::supervisor::forget_network(&forget.ssid).await;

    httpd::response("200 OK", JSON, json!({ "ok": true }).to_string().as_bytes())
}

fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

const PORTAL_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>LED Board — WiFi Setup</title>
<style>
  body { font-family: system-ui, sans-serif; margin: 0; background: #0f1115; color: #e6e6e6; }
  .wrap { max-width: 480px; margin: 0 auto; padding: 16px; }
  h1 { font-size: 20px; } h2 { font-size: 16px; margin-top: 24px; }
  .card { background: #1a1e26; border-radius: 10px; padding: 12px 14px; margin: 10px 0; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 0; border-bottom: 1px solid #2a2f3a; }
  .row:last-child { border-bottom: 0; }
  button { background: #2f81f7; color: #fff; border: 0; border-radius: 8px; padding: 8px 14px; font-size: 14px; cursor: pointer; }
  button.warn { background: #6b2f2f; }
  button:disabled { opacity: .5; }
  input { background: #10141b; color: #e6e6e6; border: 1px solid #333a48; border-radius: 8px; padding: 8px; width: 100%; box-sizing: border-box; }
  .sig { color: #8b93a5; font-variant-numeric: tabular-nums; }
  .status { font-size: 14px; color: #8b93a5; }
  .ok { color: #4ec97a; } .bad { color: #e06c75; }
</style>
</head>
<body>
<div class="wrap">
  <h1>LED Board — WiFi Setup</h1>
  <div class="card status" id="status">Loading…</div>
  <div class="card" id="appcard" style="display:none"><a href="/" style="color:#fff;text-decoration:none"><button>Open LED Board app</button></a></div>

  <h2>Scan networks <button id="scan" style="float:right">Search</button></h2>
  <div class="card" id="results"><span class="status">Press Search to scan.</span></div>

  <h2>Saved networks</h2>
  <div class="card" id="saved"><span class="status">Loading…</span></div>
</div>
<script>
const $ = (id) => document.getElementById(id);

async function j(url, opts) {
  const res = await fetch(url, opts);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function refreshStatus() {
  try {
    const s = await j('/api/status');
    let txt = '';
    if (s.ssid) txt = `<span class="ok">Connected to ${s.ssid}</span> @ ${s.ip || ''}`;
    else if (s.ap_ssid) txt = `Fallback AP: <b>${s.ap_ssid}</b> (${s.ap_ip || ''})`;
    else txt = 'Connecting…';
    txt += s.pico_online ? ' · board online' : ' · board offline';
    $('status').innerHTML = txt;
    $('appcard').style.display = s.pico_online ? '' : 'none';
  } catch (e) { $('status').textContent = 'status error: ' + e; }
}

async function refreshSaved() {
  try {
    const d = await j('/api/networks');
    if (!d.networks.length) { $('saved').innerHTML = '<span class="status">None yet — scan and connect below.</span>'; return; }
    $('saved').innerHTML = '';
    for (const n of d.networks) {
      const row = document.createElement('div'); row.className = 'row';
      row.innerHTML = `<span>${n.ssid}</span>`;
      const b = document.createElement('button'); b.className = 'warn'; b.textContent = 'Forget';
      b.onclick = async () => { await j('/api/forget', {method:'POST', body: JSON.stringify({ssid: n.ssid})}); refreshSaved(); };
      row.appendChild(b); $('saved').appendChild(row);
    }
  } catch (e) { $('saved').textContent = 'error loading saved networks'; }
}

async function scan() {
  $('scan').disabled = true; $('scan').textContent = 'Scanning…';
  $('results').innerHTML = '<span class="status">Searching…</span>';
  try {
    const list = await j('/api/wifi');
    $('results').innerHTML = '';
    if (!list.length) { $('results').innerHTML = '<span class="status">No networks found.</span>'; }
    for (const n of list) {
      const row = document.createElement('div'); row.className = 'row';
      row.innerHTML = `<span>${n.ssid} <span class="sig">[${n.signal_strength}]${n.password_required ? ' 🔒' : ''}</span></span>`;
      const b = document.createElement('button'); b.textContent = 'Connect';
      b.onclick = () => connect(n.ssid, n.password_required, b, row);
      row.appendChild(b); $('results').appendChild(row);
    }
  } catch (e) { $('results').innerHTML = '<span class="bad">Scan failed.</span>'; }
  $('scan').disabled = false; $('scan').textContent = 'Search';
}

async function connect(ssid, secure, btn, row) {
  let pass = '';
  if (secure) {
    const wrap = document.createElement('div'); wrap.style.width = '100%';
    wrap.innerHTML = `<div style="margin-top:6px"><input type="password" placeholder="password" id="pw"></div>`;
    row.appendChild(wrap);
    const done = document.createElement('button'); done.textContent = 'Go'; done.style.marginTop = '6px';
    done.onclick = async () => {
      const el = document.getElementById('pw');
      await doJoin(ssid, el ? el.value : '', btn);
    };
    wrap.appendChild(done);
    return;
  }
  await doJoin(ssid, '', btn);
}

async function doJoin(ssid, pass, btn) {
  btn.disabled = true; btn.textContent = 'Joining…';
  try {
    await j('/api/wifi', {method: 'POST', body: JSON.stringify({ssid, pass})});
    btn.textContent = 'Sent ✓';
    setTimeout(refreshStatus, 1500);
    setTimeout(refreshSaved, 1500);
  } catch (e) { btn.textContent = 'Failed'; btn.disabled = false; }
}

$('scan').onclick = scan;
refreshStatus(); refreshSaved();
setInterval(refreshStatus, 5000);
</script>
</body>
</html>
"#;

//! Type `b64 hello` to encode or decode text. Press Enter to copy.
//! Build first with `cargo build --release` in this folder.
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn query(text: &str) -> Value {
    if text.is_empty() {
        return json!({"items": []});
    }
    let mut items = vec![json!({"id": STANDARD.encode(text), "title": STANDARD.encode(text), "subtitle": "Encoded"})];
    let decoded = STANDARD.decode(text).ok().and_then(|b| String::from_utf8(b).ok());
    if let Some(decoded) = decoded {
        items.push(json!({"id": decoded, "title": decoded, "subtitle": "Decoded"}));
    }
    json!({"items": items})
}

fn main() {
    // The loop ends when Glint closes stdin, which is how extensions must stop.
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let params = &request["params"];
        let result = if request["method"] == "query" {
            query(params["text"].as_str().unwrap_or(""))
        } else {
            json!({"effects": [{"type": "copy", "text": params["id"]}]})
        };
        let reply = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}

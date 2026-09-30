use std::sync::mpsc::Sender;
use std::thread;
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Clone)]
pub struct TunaUpdate {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub progress_ms: u64,
    pub is_playing: bool,
}

pub struct TunaServer;

impl TunaServer {
    pub fn start(tx: Sender<TunaUpdate>, port: u16) -> Result<(), String> {
        let addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&addr).map_err(|e| format!("Failed to bind {}: {}", addr, e))?;

        println!("[LyricReme] Tuna HTTP server listening on http://{}", addr);

        thread::Builder::new()
            .name("tuna_http_server".into())
            .spawn(move || {
                for mut request in server.incoming_requests() {
                    let cors_origin = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
                    let cors_methods = Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"POST, OPTIONS, GET"[..]).unwrap();
                    let cors_headers = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"*"[..]).unwrap();

                    if request.method() == &Method::Options {
                        let response = Response::empty(StatusCode(200))
                            .with_header(cors_origin)
                            .with_header(cors_methods)
                            .with_header(cors_headers);
                        let _ = request.respond(response);
                        continue;
                    }

                    if request.method() == &Method::Post {
                        let mut body = String::new();
                        let _ = request.as_reader().read_to_string(&mut body);

                        if !body.is_empty() {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                                let data = if v.get("data").is_some() { &v["data"] } else { &v };

                                let title = data["title"].as_str().unwrap_or_default().trim().to_string();

                                let artist = if let Some(artists) = data["artists"].as_array() {
                                    artists.first().and_then(|a| a.as_str()).unwrap_or_default().trim().to_string()
                                } else {
                                    data["artist"].as_str().unwrap_or_default().trim().to_string()
                                };

                                let duration_ms = data["duration"].as_u64()
                                    .or_else(|| data["duration"].as_f64().map(|f| f as u64))
                                    .unwrap_or(0);

                                let progress_ms = data["progress"].as_u64()
                                    .or_else(|| data["progress"].as_f64().map(|f| f as u64))
                                    .unwrap_or(0);

                                let is_playing = data["status"].as_str()
                                    .map(|s| s.eq_ignore_ascii_case("playing"))
                                    .unwrap_or(true);

                                if !title.is_empty() {
                                    let update = TunaUpdate {
                                        title,
                                        artist,
                                        duration_ms,
                                        progress_ms,
                                        is_playing,
                                    };
                                    let _ = tx.send(update);
                                }
                            }
                        }

                        let response = Response::from_string("{\"status\":\"ok\"}")
                            .with_header(cors_origin)
                            .with_header(cors_methods)
                            .with_header(cors_headers);
                        let _ = request.respond(response);
                        continue;
                    }

                    let response = Response::empty(StatusCode(404));
                    let _ = request.respond(response);
                }
            })
            .map_err(|e| format!("Failed to spawn tuna thread: {}", e))?;

        Ok(())
    }
}

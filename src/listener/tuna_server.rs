use std::sync::mpsc::Sender;
use std::thread;
use serde::Deserialize;
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Clone, Deserialize)]
pub struct TunaPayload {
    pub data: TunaData,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TunaData {
    pub title: Option<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    pub duration: Option<u64>,
    pub progress: Option<u64>,
    pub status: Option<String>,
    pub album: Option<String>,
}

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
                        if request.as_reader().read_to_string(&mut body).is_ok() {
                            if let Ok(payload) = serde_json::from_str::<TunaPayload>(&body) {
                                let d = payload.data;
                                let title = d.title.unwrap_or_default().trim().to_string();
                                let artist = d.artists.first().cloned().unwrap_or_default().trim().to_string();
                                let is_playing = d.status.as_deref().unwrap_or("playing").eq_ignore_ascii_case("playing");

                                if !title.is_empty() {
                                    let update = TunaUpdate {
                                        title,
                                        artist,
                                        duration_ms: d.duration.unwrap_or(0),
                                        progress_ms: d.progress.unwrap_or(0),
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

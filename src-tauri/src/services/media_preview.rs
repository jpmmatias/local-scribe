use crate::errors::{AppError, Result};
use std::{path::PathBuf, sync::Arc};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, SeekFrom},
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct ServedMedia {
    token: String,
    path: PathBuf,
    mime: &'static str,
    size: u64,
}

#[derive(Clone)]
pub struct PreviewServer {
    port: u16,
    active: Arc<RwLock<Option<ServedMedia>>>,
}

impl PreviewServer {
    pub async fn start(stop: CancellationToken) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let active = Arc::new(RwLock::new(None));
        let shared = active.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            let active = shared.clone();
                            tokio::spawn(async move { let _ = serve(stream, active).await; });
                        }
                        Err(_) => break,
                    }
                }
            }
        });
        Ok(Self { port, active })
    }

    pub async fn register(&self, path: PathBuf) -> Result<String> {
        let path = tokio::fs::canonicalize(path).await?;
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(AppError::new(
                "media_preview",
                "O arquivo de mídia não está disponível para reprodução.",
            ));
        }
        let mime = mime_for(&path).ok_or_else(|| {
            AppError::new(
                "media_preview",
                "O formato deste arquivo não pode ser pré-visualizado.",
            )
        })?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        *self.active.write().await = Some(ServedMedia {
            token: token.clone(),
            path,
            mime,
            size: metadata.len(),
        });
        Ok(format!("http://127.0.0.1:{}/{token}", self.port))
    }
}

async fn serve(stream: TcpStream, active: Arc<RwLock<Option<ServedMedia>>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    reader.read_line(&mut request).await?;
    if request.len() > 4096 {
        return respond_error(reader.get_mut(), 400, "Bad Request").await;
    }
    let mut request_parts = request.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or_default().to_owned();
    let token = target
        .trim_start_matches('/')
        .split(['?', '/'])
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut range_header = None;
    let mut header_count = 0usize;
    loop {
        request.clear();
        if reader.read_line(&mut request).await? == 0 {
            return Ok(());
        }
        header_count += 1;
        if request.len() > 4096 || header_count > 64 {
            return respond_error(reader.get_mut(), 400, "Bad Request").await;
        }
        if request == "\r\n" || request == "\n" {
            break;
        }
        if let Some((name, value)) = request.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range_header = Some(value.trim().to_owned());
            }
        }
    }
    if method == "OPTIONS" {
        reader.get_mut().write_all(
            b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, HEAD, OPTIONS\r\nAccess-Control-Allow-Headers: Range\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ).await?;
        return Ok(());
    }
    if method != "GET" && method != "HEAD" {
        return respond_error(reader.get_mut(), 405, "Method Not Allowed").await;
    }
    let Some(media) = active
        .read()
        .await
        .clone()
        .filter(|media| media.token == token)
    else {
        return respond_error(reader.get_mut(), 404, "Not Found").await;
    };
    let (status, start, end) = match range_header.as_deref() {
        Some(range) => match parse_range(range, media.size) {
            Some((start, end)) => (206, start, end),
            None => {
                reader.get_mut().write_all(format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    media.size
                ).as_bytes()).await?;
                return Ok(());
            }
        },
        None if media.size > 0 => (200, 0, media.size - 1),
        None => return respond_error(reader.get_mut(), 404, "Not Found").await,
    };
    let length = end - start + 1;
    let mut headers = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\nContent-Length: {length}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Expose-Headers: Accept-Ranges, Content-Range, Content-Length\r\nCache-Control: no-store\r\nConnection: close\r\n",
        if status == 206 { "Partial Content" } else { "OK" },
        media.mime,
    );
    if status == 206 {
        headers.push_str(&format!(
            "Content-Range: bytes {start}-{end}/{}\r\n",
            media.size
        ));
    }
    headers.push_str("\r\n");
    let stream = reader.get_mut();
    stream.write_all(headers.as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }
    let mut file = File::open(&media.path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    tokio::io::copy(&mut file.take(length), stream).await?;
    Ok(())
}

async fn respond_error(stream: &mut TcpStream, status: u16, reason: &str) -> std::io::Result<()> {
    stream
        .write_all(
            format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
}

fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let range = value.strip_prefix("bytes=")?;
    let (start, end) = range.split_once('-')?;
    if end.contains(',') || start.contains(',') || size == 0 {
        return None;
    }
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(size);
        if suffix == 0 {
            return None;
        }
        return Some((size - suffix, size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= size {
        return None;
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    (end >= start).then_some((start, end))
}

fn mime_for(path: &std::path::Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "mkv" => Some("video/x-matroska"),
        "webm" => Some("video/webm"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "m4a" => Some("audio/mp4"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{mime_for, parse_range, PreviewServer};
    use std::path::Path;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };
    use tokio_util::sync::CancellationToken;

    #[test]
    fn parses_media_ranges_for_seek_and_streaming() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=0-1,4-5", 1000), None);
    }

    #[test]
    fn returns_browser_media_mime_types_from_extension() {
        assert_eq!(mime_for(Path::new("capture.mp4")), Some("video/mp4"));
        assert_eq!(mime_for(Path::new("captura.M4A")), Some("audio/mp4"));
        assert_eq!(mime_for(Path::new("capture.unknown")), None);
    }

    #[tokio::test]
    #[ignore = "Requires loopback socket permissions; network binding is blocked in the Codex sandbox"]
    async fn serves_registered_media_with_http_byte_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("sample.mp4");
        tokio::fs::write(&file, b"0123456789").await.unwrap();
        let server = PreviewServer::start(CancellationToken::new())
            .await
            .unwrap();
        let url = server.register(file).await.unwrap();
        let parsed = url.strip_prefix("http://127.0.0.1:").unwrap();
        let (port, token) = parsed.split_once('/').unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap()))
            .await
            .unwrap();
        stream
            .write_all(
                format!(
                    "GET /{token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nRange: bytes=3-6\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(response.contains("Content-Type: video/mp4\r\n"));
        assert!(response.contains("Content-Range: bytes 3-6/10\r\n"));
        assert!(response.ends_with("3456"));
    }
}

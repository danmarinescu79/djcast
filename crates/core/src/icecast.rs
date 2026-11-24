//! Minimal Icecast source client (no external HTTP dependency).
//!
//! * Icecast 2.4+: `PUT /mount HTTP/1.1` with `Expect: 100-continue`.
//! * Older Icecast: legacy `SOURCE /mount HTTP/1.0` (used as fallback).
//! * Metadata for MP3/AAC streams: `GET /admin/metadata?mode=updinfo&song=...`.

use crate::config::ServerConfig;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
pub const USER_AGENT: &str = concat!("djcast/", env!("CARGO_PKG_VERSION"));

/// Audio parameters advertised in `ice-audio-info`.
#[derive(Debug, Clone, Copy)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub bitrate: u32,
    pub channels: u8,
}

/// An open source connection; write encoded audio into it.
pub struct SourceConnection {
    stream: TcpStream,
}

impl SourceConnection {
    pub fn connect(cfg: &ServerConfig, content_type: &str, info: AudioInfo) -> Result<Self> {
        match Self::connect_with(cfg, content_type, info, Method::Put) {
            Ok(c) => Ok(c),
            Err(e) if e.downcast_ref::<MethodRejected>().is_some() => {
                log::info!("server rejected PUT, retrying with legacy SOURCE method");
                Self::connect_with(cfg, content_type, info, Method::Source)
            }
            Err(e) => Err(e),
        }
    }

    fn connect_with(cfg: &ServerConfig, content_type: &str, info: AudioInfo, method: Method) -> Result<Self> {
        let mut stream = open_tcp(&cfg.host, cfg.port)?;
        let mount = normalize_mount(&cfg.mount);
        let (verb, version) = match method {
            Method::Put => ("PUT", "HTTP/1.1"),
            Method::Source => ("SOURCE", "HTTP/1.0"),
        };
        let mut req = format!(
            "{verb} {mount} {version}\r\n\
             Host: {host}:{port}\r\n\
             Authorization: Basic {auth}\r\n\
             User-Agent: {USER_AGENT}\r\n\
             Content-Type: {content_type}\r\n\
             Ice-Public: {public}\r\n\
             Ice-Name: {name}\r\n\
             Ice-Description: {desc}\r\n\
             Ice-Genre: {genre}\r\n\
             Ice-Url: {url}\r\n\
             Ice-Audio-Info: ice-samplerate={sr};ice-bitrate={br};ice-channels={ch}\r\n",
            host = cfg.host,
            port = cfg.port,
            auth = basic_auth(&cfg.user, &cfg.password),
            public = u8::from(cfg.public),
            name = header_safe(&cfg.name),
            desc = header_safe(&cfg.description),
            genre = header_safe(&cfg.genre),
            url = header_safe(&cfg.url),
            sr = info.sample_rate,
            br = info.bitrate,
            ch = info.channels,
        );
        if method == Method::Put {
            req.push_str("Expect: 100-continue\r\n");
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).context("sending source request")?;

        let (code, reason) = read_status(&mut stream)?;
        match code {
            100 | 200 => {
                log::info!("connected to {}:{}{} ({verb})", cfg.host, cfg.port, mount);
                Ok(Self { stream })
            }
            401 => bail!("authentication failed (401): check user/password"),
            403 => bail!("mount {mount} refused (403 {reason}): already in use or not allowed"),
            400 | 405 | 501 if method == Method::Put => Err(MethodRejected.into()),
            _ => bail!("server answered {code} {reason}"),
        }
    }

    pub fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Put,
    Source,
}

#[derive(Debug, thiserror::Error)]
#[error("method rejected")]
struct MethodRejected;

/// Update the "now playing" title of an MP3/AAC mount.
pub fn update_metadata(cfg: &ServerConfig, song: &str) -> Result<()> {
    let mut stream = open_tcp(&cfg.host, cfg.port)?;
    let mount = normalize_mount(&cfg.mount);
    let req = format!(
        "GET /admin/metadata?mount={m}&mode=updinfo&song={s}&charset=UTF-8 HTTP/1.0\r\n\
         Host: {host}:{port}\r\n\
         Authorization: Basic {auth}\r\n\
         User-Agent: {USER_AGENT}\r\n\r\n",
        m = url_encode(&mount),
        s = url_encode(song),
        host = cfg.host,
        port = cfg.port,
        auth = basic_auth(&cfg.user, &cfg.password),
    );
    stream.write_all(req.as_bytes())?;
    let (code, reason) = read_status(&mut stream)?;
    if code != 200 {
        bail!("metadata update failed: {code} {reason}");
    }
    Ok(())
}

fn open_tcp(host: &str, port: u16) -> Result<TcpStream> {
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("resolving {host}:{port}"))?
        .collect();
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => {
                s.set_read_timeout(Some(IO_TIMEOUT))?;
                s.set_write_timeout(Some(IO_TIMEOUT))?;
                s.set_nodelay(true)?;
                return Ok(s);
            }
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(e).with_context(|| format!("connecting to {host}:{port}")),
        None => bail!("no address for {host}:{port}"),
    }
}

/// Read the HTTP status line and headers; returns (code, reason).
fn read_status(stream: &mut TcpStream) -> Result<(u16, String)> {
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") && !buf.ends_with(b"\n\n") {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => buf.push(byte[0]),
            Err(e) => return Err(e).context("reading server response"),
        }
        if buf.len() > 16 * 1024 {
            bail!("response header too large");
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next().unwrap_or_default();
    let mut parts = line.splitn(3, ' ');
    let _version = parts.next();
    let code = parts
        .next()
        .and_then(|c| c.parse().ok())
        .with_context(|| format!("invalid response from server: {line:?}"))?;
    Ok((code, parts.next().unwrap_or_default().trim().to_string()))
}

fn basic_auth(user: &str, pass: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
}

fn normalize_mount(mount: &str) -> String {
    if mount.starts_with('/') { mount.to_string() } else { format!("/{mount}") }
}

fn header_safe(s: &str) -> String {
    s.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    fn server_cfg(port: u16) -> ServerConfig {
        ServerConfig { host: "127.0.0.1".into(), port, ..Default::default() }
    }

    fn read_head(reader: &mut BufReader<TcpStream>) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut l = String::new();
            reader.read_line(&mut l).unwrap();
            if l == "\r\n" || l.is_empty() {
                return lines;
            }
            lines.push(l.trim_end().to_string());
        }
    }

    #[test]
    fn put_handshake_then_data() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let head = read_head(&mut reader);
            (&sock).write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
            let mut data = [0u8; 4];
            reader.read_exact(&mut data).unwrap();
            (head, data)
        });
        let info = AudioInfo { sample_rate: 48_000, bitrate: 192, channels: 2 };
        let mut conn = SourceConnection::connect(&server_cfg(port), "audio/mpeg", info).unwrap();
        conn.send(b"abcd").unwrap();
        let (head, data) = srv.join().unwrap();
        assert_eq!(head[0], "PUT /live HTTP/1.1");
        assert!(head.iter().any(|h| h == "Authorization: Basic c291cmNlOmhhY2ttZQ=="));
        assert!(head.iter().any(|h| h == "Content-Type: audio/mpeg"));
        assert_eq!(&data, b"abcd");
    }

    #[test]
    fn falls_back_to_source_method() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s1, _) = listener.accept().unwrap();
            let _ = read_head(&mut BufReader::new(s1.try_clone().unwrap()));
            (&s1).write_all(b"HTTP/1.0 405 Method Not Allowed\r\n\r\n").unwrap();
            let (s2, _) = listener.accept().unwrap();
            let head = read_head(&mut BufReader::new(s2.try_clone().unwrap()));
            (&s2).write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
            head
        });
        let info = AudioInfo { sample_rate: 44_100, bitrate: 128, channels: 2 };
        SourceConnection::connect(&server_cfg(port), "audio/mpeg", info).unwrap();
        assert_eq!(srv.join().unwrap()[0], "SOURCE /live HTTP/1.0");
    }

    #[test]
    fn bad_password_is_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let _ = read_head(&mut BufReader::new(s.try_clone().unwrap()));
            (&s).write_all(b"HTTP/1.1 401 Unauthorized\r\n\r\n").unwrap();
        });
        let info = AudioInfo { sample_rate: 44_100, bitrate: 128, channels: 2 };
        let err = SourceConnection::connect(&server_cfg(port), "audio/mpeg", info).err().unwrap();
        assert!(err.to_string().contains("401"));
    }

    #[test]
    fn metadata_request_is_url_encoded() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let head = read_head(&mut BufReader::new(s.try_clone().unwrap()));
            (&s).write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
            head
        });
        update_metadata(&server_cfg(port), "Ațeș & Co - Tră").unwrap();
        let head = srv.join().unwrap();
        assert_eq!(
            head[0],
            "GET /admin/metadata?mount=/live&mode=updinfo&song=A%C8%9Be%C8%99%20%26%20Co%20-%20Tr%C4%83&charset=UTF-8 HTTP/1.0"
        );
    }
}

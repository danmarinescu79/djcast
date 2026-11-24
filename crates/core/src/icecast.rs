//! Minimal Icecast / Shoutcast source client (no external HTTP dependency).
//!
//! * Icecast 2.4+: `PUT /mount HTTP/1.1` with `Expect: 100-continue`, Basic auth (user + password).
//! * Older Icecast: legacy `SOURCE /mount HTTP/1.0` (used as fallback).
//! * Shoutcast v1: source connects to port + 1, sends the password as the first line,
//!   server answers `OK2`, then `icy-*` headers and audio. No user, no mount.
//! * Shoutcast v2 (DNAS 2, legacy source mode): like v1 with password `password:#<stream id>`.
//! * Metadata: Icecast `GET /admin/metadata?mode=updinfo`, Shoutcast `GET /admin.cgi?mode=updinfo`.

use crate::config::{Protocol, ServerConfig};
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
        if cfg.protocol != Protocol::Icecast {
            return Self::connect_shoutcast(cfg, content_type, info);
        }
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

    /// Shoutcast v1 / v2 (legacy source protocol) on port + 1.
    fn connect_shoutcast(cfg: &ServerConfig, content_type: &str, info: AudioInfo) -> Result<Self> {
        let source_port = cfg.port.checked_add(1).context("port too high for Shoutcast (source uses port + 1)")?;
        let mut stream = open_tcp(&cfg.host, source_port)?;
        stream.write_all(format!("{}\r\n", shoutcast_password(cfg)).as_bytes()).context("sending password")?;
        let reply = read_line(&mut stream).context("waiting for the server's answer")?;
        if !reply.trim().starts_with("OK2") {
            let r = reply.trim();
            if r.to_lowercase().contains("password") {
                bail!("authentication failed: the server said \"{r}\" — check the password");
            }
            bail!("server refused the source: \"{r}\"");
        }
        let headers = format!(
            "content-type:{content_type}\r\n\
             icy-name:{name}\r\n\
             icy-genre:{genre}\r\n\
             icy-url:{url}\r\n\
             icy-pub:{public}\r\n\
             icy-br:{br}\r\n\r\n",
            name = header_safe(&cfg.name),
            genre = header_safe(&cfg.genre),
            url = header_safe(&cfg.url),
            public = u8::from(cfg.public),
            br = info.bitrate,
        );
        stream.write_all(headers.as_bytes()).context("sending stream headers")?;
        log::info!("connected to {}:{} ({:?}, source port {source_port})", cfg.host, cfg.port, cfg.protocol);
        Ok(Self { stream })
    }

    pub fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(data)
    }
}

fn shoutcast_password(cfg: &ServerConfig) -> String {
    match cfg.protocol {
        Protocol::Shoutcast2 => format!("{}:#{}", cfg.password, cfg.stream_id.max(1)),
        _ => cfg.password.clone(),
    }
}

/// Read one `\n`-terminated line.
fn read_line(stream: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\n") && buf.len() < 1024 {
        match stream.read(&mut byte)? {
            0 => break,
            _ => buf.push(byte[0]),
        }
    }
    if buf.is_empty() {
        bail!("connection closed by the server");
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
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
    if cfg.protocol != Protocol::Icecast {
        return update_metadata_shoutcast(cfg, song);
    }
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

fn update_metadata_shoutcast(cfg: &ServerConfig, song: &str) -> Result<()> {
    let mut stream = open_tcp(&cfg.host, cfg.port)?;
    let sid = match cfg.protocol {
        Protocol::Shoutcast2 => format!("sid={}&", cfg.stream_id.max(1)),
        _ => String::new(),
    };
    // Shoutcast v1 only answers admin.cgi for browser-like user agents.
    let req = format!(
        "GET /admin.cgi?{sid}pass={p}&mode=updinfo&song={s} HTTP/1.0\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: Mozilla/5.0 (compatible; {USER_AGENT})\r\n\r\n",
        p = url_encode(&cfg.password),
        s = url_encode(song),
        host = cfg.host,
        port = cfg.port,
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

    /// Fake Shoutcast source port: checks the password line, answers OK2, returns headers.
    fn fake_shoutcast(listener: TcpListener, reply: &'static [u8]) -> thread::JoinHandle<(String, Vec<String>)> {
        thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            let mut pass = String::new();
            reader.read_line(&mut pass).unwrap();
            (&s).write_all(reply).unwrap();
            let head = if reply.starts_with(b"OK2") { read_head(&mut reader) } else { vec![] };
            (pass.trim_end().to_string(), head)
        })
    }

    /// Bind two adjacent ports (server port P is unused by the source; P + 1 is).
    fn adjacent_ports() -> (u16, TcpListener) {
        for _ in 0..50 {
            let probe = TcpListener::bind("127.0.0.1:0").unwrap();
            let p = probe.local_addr().unwrap().port();
            drop(probe);
            if let Ok(l) = TcpListener::bind(("127.0.0.1", p + 1)) {
                return (p, l);
            }
        }
        panic!("no free adjacent ports");
    }

    #[test]
    fn shoutcast_v1_and_v2_handshakes() {
        let info = AudioInfo { sample_rate: 44_100, bitrate: 192, channels: 2 };

        let (port, listener) = adjacent_ports();
        let srv = fake_shoutcast(listener, b"OK2\r\nicy-caps:11\r\n\r\n");
        let cfg = ServerConfig { protocol: Protocol::Shoutcast1, password: "secret".into(), ..server_cfg(port) };
        SourceConnection::connect(&cfg, "audio/mpeg", info).unwrap();
        let (pass, head) = srv.join().unwrap();
        assert_eq!(pass, "secret");
        assert!(head.contains(&"icy-br:192".to_string()));
        assert!(head.contains(&"content-type:audio/mpeg".to_string()));

        let (port, listener) = adjacent_ports();
        let srv = fake_shoutcast(listener, b"OK2\r\nicy-caps:11\r\n\r\n");
        let cfg = ServerConfig { protocol: Protocol::Shoutcast2, password: "secret".into(), stream_id: 3, ..server_cfg(port) };
        SourceConnection::connect(&cfg, "audio/mpeg", info).unwrap();
        assert_eq!(srv.join().unwrap().0, "secret:#3");

        let (port, listener) = adjacent_ports();
        let srv = fake_shoutcast(listener, b"invalid password\r\n");
        let cfg = ServerConfig { protocol: Protocol::Shoutcast1, password: "nope".into(), ..server_cfg(port) };
        let err = SourceConnection::connect(&cfg, "audio/mpeg", info).err().unwrap();
        assert!(err.to_string().contains("check the password"), "{err}");
        srv.join().unwrap();
    }

    #[test]
    fn shoutcast_v2_metadata_uses_admin_cgi_with_sid() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let head = read_head(&mut BufReader::new(s.try_clone().unwrap()));
            (&s).write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
            head
        });
        let cfg = ServerConfig { protocol: Protocol::Shoutcast2, password: "p w".into(), stream_id: 2, ..server_cfg(port) };
        update_metadata(&cfg, "A - B").unwrap();
        let head = srv.join().unwrap();
        assert_eq!(head[0], "GET /admin.cgi?sid=2&pass=p%20w&mode=updinfo&song=A%20-%20B HTTP/1.0");
        assert!(head.iter().any(|h| h.starts_with("User-Agent: Mozilla/")));
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

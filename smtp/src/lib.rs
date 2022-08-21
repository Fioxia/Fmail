use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::ops::Add;
use std::sync::Arc;
use std::time::Duration;

use rustls::{OwnedTrustAnchor, RootCertStore, ServerName};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};
// use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
// use tokio_rustls::server::TlsStream;
// use tokio_rustls::TlsAcceptor;

#[derive(Debug)]
pub enum Error {
    IO(io::Error),
    /// Total amount, amount written
    WriteAll(usize, usize),
    ConnClosed,
    Timeout,
    Quit,
    StartTls,
    FailedToStartTls,
    String(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::String(s)
    }
}
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::String(s.to_string())
    }
}

// pub enum Success {
//     Quit,
// }

pub struct CommandLine {
    pub command: Command,
    pub rest_of_line: String,
}

pub enum Command {
    Quit,
    Data,
    Helo,
    Ehlo,
    StartTls,
    RcptTo,
    MailFrom,
}

const COMMAND_MAP: [(&'static str, Command); 7] = [
    ("QUIT", Command::Quit),
    ("DATA", Command::Data),
    ("HELO", Command::Helo),
    ("EHLO", Command::Ehlo),
    ("STARTTLS", Command::StartTls),
    ("RCPT TO", Command::RcptTo),
    ("MAIL FROM", Command::MailFrom),
];

pub fn get_command(string: String) -> Option<CommandLine> {
    let command_line = string.to_uppercase();

    for (test, command) in COMMAND_MAP {
        if command_line.starts_with(test) {
            return Some(CommandLine {
                command: command,
                rest_of_line: string[test.len()..].to_string(),
            });
        }
    }
    None
}

#[derive(Debug)]
pub enum STATUS {
    S200(u16),
    S300(u16),
    S400(u16),
    S500(u16),
}

#[derive(Debug)]
pub struct Line {
    pub status: STATUS,
    pub content: String,
}

pub fn get_status_line(string: String) -> Option<Line> {
    let num = &string[0..3];

    let status = if let Ok(number) = num.parse::<u16>() {
        match number {
            200..=299 => STATUS::S200(number),
            300..=399 => STATUS::S300(number),
            400..=499 => STATUS::S400(number),
            500..=599 => STATUS::S500(number),
            _ => return None,
        }
    } else {
        return None;
    };

    Some(Line {
        status,
        content: String::from(&string[3..]),
    })
}

pub struct Stream {
    read_buffer: String,
    pub tcp_stream: Option<TcpStream>,
    pub tls_stream: Option<TlsStream<TcpStream>>,
}

fn get_cert() -> Vec<Certificate> {
    let certfile = File::open("fullchain1.pem").expect("cannot open certificate file");
    let mut reader = BufReader::new(certfile);
    rustls_pemfile::certs(&mut reader)
        .unwrap()
        .iter()
        .map(|v| Certificate(v.clone()))
        .collect()
}

fn get_keys() -> PrivateKey {
    let certfile = File::open("privkey1.pem").expect("cannot open certificate file");
    let mut reader = BufReader::new(certfile);

    loop {
        match rustls_pemfile::read_one(&mut reader).expect("cannot parse private key .pem file") {
            Some(rustls_pemfile::Item::RSAKey(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::ECKey(key)) => return PrivateKey(key),
            None => break,
            _ => {}
        }
    }

    panic!("no keys found in _ (encrypted keys not supported)",);
}

impl Stream {
    pub fn new(tcp_stream: TcpStream) -> Self {
        Self {
            read_buffer: String::new(),
            tcp_stream: Some(tcp_stream),
            tls_stream: None,
        }
    }

    /// Absorbs Stream instace and returns a new instance with TLS enabled
    pub async fn start_tls_server(mut self) -> Result<Stream, Error> {
        let server = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(get_cert(), get_keys());
        if let Err(_) = &server {
            self.write_line("502 Internal Error starting tls").await?;
            return Err(Error::FailedToStartTls);
        }

        self.write_line("220 Goahead").await?;

        let acceptor = TlsAcceptor::from(Arc::new(server.unwrap()));

        match acceptor.accept(self.tcp_stream.unwrap()).await {
            Ok(s) => Ok(Self {
                read_buffer: String::new(),
                tcp_stream: None,
                tls_stream: Some(tokio_rustls::TlsStream::from(s)),
            }),
            Err(e) => {
                println!("E: {}", e);
                Err(Error::FailedToStartTls)
            }
        }
    }

    pub async fn connect_tls_client(self, domain: ServerName) -> Result<Stream, Error> {
        // https://docs.rs/rustls/latest/rustls/index.html
        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let rc_config = Arc::new(config);

        let conn = TlsConnector::from(rc_config);

        match conn.connect(domain, self.tcp_stream.unwrap()).await {
            Ok(stream) => Ok(Self {
                read_buffer: String::new(),
                tcp_stream: None,
                tls_stream: Some(tokio_rustls::TlsStream::from(stream)),
            }),
            Err(e) => return Err(e.into()),
        }
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        if let Some(tls) = &mut self.tls_stream {
            tls.read(buf).await
        } else if let Some(s) = &mut self.tcp_stream {
            s.read(buf).await
        } else {
            panic!("Neither TLS or TCP contained a value")
        }
    }

    async fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        if let Some(tls) = &mut self.tls_stream {
            tls.write(buf).await
        } else if let Some(s) = &mut self.tcp_stream {
            s.write(buf).await
        } else {
            panic!("Neither TLS or TCP contained a value")
        }
    }

    pub async fn read_line(&mut self) -> Result<String, Error> {
        let mut buf = [0; 100];
        for _ in 0..100 {
            // See if line exists in buffer
            if let Some((line, rest)) = self.read_buffer.clone().split_once("\r\n") {
                self.read_buffer = rest.into();
                return Ok(line.to_string());
            }

            // Read more bytes
            let bytes = self.read(&mut buf).await?;

            if bytes == 0 {
                return Err(Error::ConnClosed);
            }

            self.read_buffer += &String::from_utf8_lossy(&buf[0..bytes]);
        }
        Err(Error::Timeout)
    }

    pub async fn write_line(&mut self, string: &str) -> Result<(), Error> {
        let num = self.write(string.as_bytes()).await?;
        let num2 = self.write(b"\r\n").await?;

        if num < string.as_bytes().len() {
            return Err(Error::WriteAll(string.as_bytes().len(), num));
        }

        if num2 < 2 {
            return Err(Error::WriteAll(2, num2));
        }

        Ok(())
    }
}

pub fn parse_email(email: &str) -> Result<&str, String> {
    if email.len() < 5 {
        return Err("555 Syntax error".into());
    }
    if &email[0..1] != ":" {
        return Err(format!("555 Syntax error, expected (:) found: ({})", &email[0..1]).into());
    }

    let bounds = email[1..].trim();
    bounds
        .strip_prefix('<')
        .and_then(|e| e.strip_suffix('>'))
        .ok_or_else(|| "555 Syntax error expect email to be enclosed within (< >)".to_string())
}

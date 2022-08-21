use std::{
    env,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use mail_exchange::{get_status_line, Error, Stream, STATUS};
use rustls::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::{
    rustls::{ClientConfig, ClientConnection, OwnedTrustAnchor, RootCertStore},
    webpki::DNSNameRef,
    TlsConnector,
};
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    name_server::{GenericConnection, GenericConnectionProvider, TokioRuntime},
    AsyncResolver, TokioAsyncResolver,
};

const DATA: &str = r#"Mime-Verion: 1.0
Date: Wed, 20 Mar 2022 17:50:39 +0000 (UTC)
From: John Doe <John@doe.com>
To: Jane Doe <Jane@doe.com>
Subject: Testing
Content-Type: text/plain; charset="UTF-8" 

This is a test email.
"#;

type DNSResolver = Arc<AsyncResolver<GenericConnection, GenericConnectionProvider<TokioRuntime>>>;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let resolver = Arc::new(
        trust_dns_resolver::TokioAsyncResolver::tokio(
            ResolverConfig::cloudflare(),
            ResolverOpts::default(),
        )
        .unwrap(),
    );

    if let Err(e) = send_mail(
        "john@doe.com",
        ["jane@doe.com"].to_vec(),
        String::from(DATA),
        resolver,
    )
    .await
    {
        println!("{:?}", e);
    }
}

async fn connect(domain: &str, resolver: DNSResolver) -> Result<(Stream, String), Error> {
    // Resolve the MX records
    let mx_records = resolver
        .mx_lookup(domain)
        .await
        .and_then(|mx| Ok(mx.into_iter().map(|d| d.exchange().to_string())));

    // Try to connect to the MX records
    if let Ok(mx) = mx_records {
        for domain in mx {
            println!("Trying: {}", domain);
            match TcpStream::connect(domain.clone() + ":25").await {
                Ok(ts) => return Ok((Stream::new(ts), domain)),
                Err(_) => (),
            }
        }
    }

    println!("Unable to find a MX record trying itself: {}", domain);

    // Failback see if the domain itself has a mail server
    TcpStream::connect(domain.to_string() + ":25")
        .await
        .map(|stream| (Stream::new(stream), domain.to_string()))
        .map_err(|e| format!("Unable to connect to the mail server: {:?}", e).into())
}

#[derive(Debug, Clone)]
enum Stage {
    MailFrom(String),
    RcptTo(String),
    Data(String),
    Quit,
}

async fn send_mail(
    from: &str,
    to: Vec<&str>,
    data: String,
    resolver: DNSResolver,
) -> Result<(), Error> {
    let domain = to
        .first()
        .unwrap()
        .split('@')
        .last()
        .ok_or_else(|| "Cannot split domain")?;

    let stage = {
        use Stage::*;
        let mut s = Vec::from([Quit]);
        s.push(Data(data));
        for rcpt in to {
            s.push(RcptTo(String::from(rcpt)))
        }
        s.push(MailFrom(String::from(from)));
        s
    };

    for i in 0..5 {
        let (mut stream, mut mail_server) = connect(domain, resolver.clone()).await?;
        welcome(&mut stream).await?;
        hello(&mut stream).await?;

        if i == 0 {
            stream.write_line("STARTTLS").await?;
            let l = get_status_line(stream.read_line().await?)
                .ok_or("error getting starttls status")?;

            if let STATUS::S200(220) = l.status {
                stream = stream
                    .connect_tls_client(mail_server.as_str().try_into().map_err(|_| "Error tls")?)
                    .await?;
                hello(&mut stream).await?;
            }
        }

        if let Err(e) = execute_stage(&mut stream, stage.clone()).await {
            println!("{:?}", e);
        } else {
            println!("Sent");
            return Ok(());
        };
    }
    Err("Failed to send".into())
}

async fn welcome(stream: &mut Stream) -> Result<(), Error> {
    let l = get_status_line(stream.read_line().await?).ok_or("error getting helo message")?;
    println!("XDF {:?}", l);

    if let STATUS::S200(220) = l.status {
        return Ok(());
    }
    Err(format!("Server returned: {:?}", l).as_str().into())
}

async fn hello(stream: &mut Stream) -> Result<Vec<String>, Error> {
    stream
        .write_line(format!("EHLO {}", env::var("HOSTNAME").unwrap()).as_str())
        .await?;

    let mut options: Vec<String> = Vec::new();
    println!("XxX vsa");

    for _ in 0..100 {
        let l = get_status_line(stream.read_line().await?).ok_or("error getting status message")?;
        println!("XxX {:?}", l);
        if let STATUS::S200(250) = l.status {
            let (c, rest) = l.content.split_at(1);
            if c == " " {
                options.push(String::from(rest));
                return Ok(options);
            } else if c == "-" {
                options.push(String::from(rest));
                continue;
            }
            return Err(format!("Invalid status message for helo: {:?}", l).into());
        }
    }
    return Err("Too many lines returned for options".into());
}

async fn execute_stage(stream: &mut Stream, mut stage: Vec<Stage>) -> Result<(), Error> {
    for _ in 0..1000 {
        if let Some(action) = stage.pop() {
            match action {
                Stage::MailFrom(from) => {
                    stream
                        .write_line(format!("MAIL FROM:<{}>", from).as_str())
                        .await?;

                    let l = get_status_line(stream.read_line().await?)
                        .ok_or("error getting mail from status")?;
                    if let STATUS::S200(_) = l.status {
                        continue;
                    }
                    return Err(format!("Invalid response to mail from: {:?}", l).into());
                }
                Stage::RcptTo(to) => {
                    stream
                        .write_line(format!("RCPT TO:<{}>", to).as_str())
                        .await?;

                    let l = get_status_line(stream.read_line().await?)
                        .ok_or("error getting rcpt to status")?;
                    if let STATUS::S200(_) = l.status {
                        continue;
                    }
                    return Err(format!("Invalid response to rcpt to: {:?}", l).into());
                }
                Stage::Data(data) => {
                    stream.write_line("DATA").await?;

                    let mut l = get_status_line(stream.read_line().await?)
                        .ok_or("error getting data status")?;
                    if let STATUS::S300(354) = l.status {
                        for line in data.lines() {
                            stream.write_line(line).await?;
                        }
                        stream.write_line(".").await?;

                        l = get_status_line(stream.read_line().await?)
                            .ok_or("error getting finish data status")?;
                        if let STATUS::S200(_) = l.status {
                            continue;
                        }
                    }
                    return Err(format!("Invalid response to data: {:?}", l).into());
                }
                Stage::Quit => {
                    stream.write_line("QUIT").await?;

                    let l = get_status_line(stream.read_line().await?)
                        .ok_or("error getting quit status")?;
                    return Ok(());
                }
            }
        } else {
            return Err("No stage in stages".into());
        }
    }
    Err("Timeout".into())
}

// async fn connect(domain: &str) -> Result<(TcpStream, String), Error> {
//     let resolver = Resolver::default().unwrap();

//     // Resolve the MX records
//     let mx_records = resolver
//         .mx_lookup(domain)
//         .and_then(|mx| Ok(mx.into_iter().map(|d| d.exchange().to_string())));

//     // Try to connect to the MX records
//     if let Ok(mx) = mx_records {
//         for domain in mx {
//             println!("Trying: {}", domain);
//             match TcpStream::connect(domain.clone() + ":25").await {
//                 Ok(ts) => return Ok((ts, domain)),
//                 Err(_) => (),
//             }
//         }
//     }

//     println!("Unable to find a MX record trying itself: {}", domain);

//     // Failback see if the domain itself has a mail server
//     let a = TcpStream::connect(domain.to_string() + ":25").await?;

//     Ok((a, domain.to_string()))
// }

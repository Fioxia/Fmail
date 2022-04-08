use std::io::Write;
use std::{borrow::Cow, io::Read, net::TcpStream};

use native_tls::{TlsConnector, TlsStream};
use smtp::{read_line, write, write_line};
use trust_dns_resolver::Resolver;

const DATA: &str = r#"Mime-Verion: 1.0
Date: Wed, 20 Mar 2022 17:50:39 +0000 (UTC)
From: Steve <steve@fioxia.local>
To: Test <test@mailinabox.local>
Subject: Testing
Content-Type: text/plain; charset="UTF-8" 

This is a test email.
"#;

fn main() -> Result<(), Cow<'static, str>> {
    initalize_connection("steve@fioxia.local", &["test@mailinabox.local"], DATA)
}

fn initalize_connection<'l>(from: &str, to: &'l [&str], data: &str) -> Result<(), Cow<'l, str>> {
    let domain = to
        .first()
        .unwrap()
        .split('@')
        .last()
        .ok_or_else(|| "Cannot split domain")?;
    let (mut stream, mail_server) = connect(domain)?;
    println!("Connected to: {} via {} on port 25", domain, mail_server);

    let helo = say_hello(&mut stream)?;

    let backup_stream_ptr = stream.try_clone().map_err(|e| format!("{}", e));

    match upgrade_connection(stream, helo) {
        Ok(tls) => send_mail(from, to, data, tls),
        Err(e) => {
            println!("Error upgrading to TLS: {}", e);
            send_mail(from, to, data, backup_stream_ptr?)
        }
    }
}

fn send_mail(
    from: &str,
    to: &[&str],
    data: &str,
    mut stream: impl Read + Write,
) -> Result<(), Cow<'static, str>> {
    println!("Ready to send");

    send_mail_from(from, &mut stream)?;
    send_rcpt_to(to, &mut stream)?;
    send_data(data, &mut stream)?;

    quit(stream);

    println!("Sent");

    Ok(())
}

/// Very unsafe
/// TODO: Make it safer by catching the errors
fn quit(mut stream: impl Read + Write) {
    let mut buffer = [0; 512];
    write_line(&mut stream, "QUIT").ok();
    match get_status(read_line(&mut stream, &mut buffer).unwrap()) {
        Ok((status, message)) => {
            if status != 221 {
                println!(
                    "Didn't receive 221, received {} - {}, closing anyway",
                    status, message
                )
            }
        }
        Err(e) => println!("Failed to QUIT recieved {}", e),
    }
}

fn send_mail_from(from: &str, mut stream: impl Read + Write) -> Result<(), String> {
    let mut buffer = [0; 512];
    write_line(&mut stream, format!("MAIL FROM: <{}>", from).as_str())?;
    let (status, message) = get_status(read_line(&mut stream, &mut buffer)?)?;
    if status != 250 {
        return Err(format!(
            "Failed to write MAIL FROM, response: {} - {}",
            status, message
        ));
    }
    Ok(())
}

fn send_rcpt_to(to: &[&str], mut stream: impl Read + Write) -> Result<(), String> {
    for rcpt in to {
        let mut buffer = [0; 512];
        write_line(&mut stream, format!("RCPT TO: <{}>", rcpt).as_str())?;
        let (status, message) = get_status(read_line(&mut stream, &mut buffer)?)?;
        if status != 250 {
            return Err(format!(
                "Failed to write MAIL FROM, response: {} - {}",
                status, message
            ));
        }
    }
    Ok(())
}

fn send_data(data: &str, mut stream: impl Read + Write) -> Result<(), String> {
    let mut buffer = [0; 128];
    write_line(&mut stream, "DATA")?;
    let (status, message) = get_status(read_line(&mut stream, &mut buffer)?)?;
    if status != 354 {
        return Err(format!(
            "Failed to send DATA, expected 354, response: {} - {}",
            status, message
        ));
    }

    write(&mut stream, data)?;
    write(&mut stream, "\r\n.\r\n")?;

    let (status, message) = get_status(read_line(&mut stream, &mut buffer)?)?;
    if status != 250 {
        return Err(format!(
            "Failed to accept DATA, response: {} - {}",
            status, message
        ));
    }

    Ok(())
}

fn upgrade_connection(mut stream: TcpStream, helo: Helo) -> Result<TlsStream<TcpStream>, String> {
    if !helo.tls {
        println!("TLS not mentioned trying anyway")
    }

    write_line(&mut stream, "STARTTLS")?;
    let mut buffer = [0; 512];
    let (status, _) = get_status(read_line(&mut stream, &mut buffer)?)?;
    // Server doesn't support STARTTLS

    if status != 220 {
        return Err("Server doesn't support tls".into());
    }

    let connector = TlsConnector::new().unwrap();
    connector
        .connect(&helo.mx.0, stream)
        .map_err(|e| format!("{}", e))
}

fn get_status(string: Cow<str>) -> Result<(usize, String), String> {
    println!("{}", string);
    if string.len() <= 3 {
        return Err("String too small".into());
    }
    if string.len() > 3 && &string[3..4] != " " {
        return Err(format!(
            "Non space value after status code, received: {:?}",
            string
        ));
    }
    let status = string[0..3]
        .parse::<usize>()
        .map_err(|e| format!("Invalid status code: {}", e));
    status.and_then(|s| Ok((s, string[4..].to_string())))
}

#[derive(Debug)]
struct Helo {
    mx: (String, String),
    tls: bool,
}

fn say_hello(mut stream: &mut TcpStream) -> Result<Helo, Cow<'static, str>> {
    let (status, message) = get_status(read_line(&mut stream, &mut [0; 512])?)?;
    if status != 220 {
        return Err(format!("Status code is not 220, received: {} - {}", status, message).into());
    }
    let (mail_server_name, _) = message
        .split_once(' ')
        .ok_or_else(|| "Unable to parse mail server name")?;
    println!("Mail server is: {}", mail_server_name);

    stream
        .write(b"EHLO 202-65-87-191.ip4.superloop.com\r\n")
        .unwrap();

    let mut helo = Helo {
        mx: ("".into(), "".into()),
        tls: false,
    };
    let mut buf = [0; 512];
    let lines = read_line(&mut stream, &mut buf)?;

    for line in lines.lines() {
        if line.len() < 3 {
            return Err(format!("Input line to small received: {}", line).into());
        }
        if &line[0..3] != "250" {
            return Err(format!("Invalid status code encountered: {}", line).into());
        }
        if line.len() == 3 {
            break;
        }
        if !(&line[3..4] == " " || &line[3..4] == "-") {
            break;
        }
        let test = &line[4..];
        let upper_test = test.to_uppercase();

        if helo.mx.0.is_empty() {
            println!("{}", test);
            match test.split_once(' ') {
                Some((name, desc)) => helo.mx = (name.to_string(), desc.to_string()),
                None => helo.mx = (test.to_string(), "".to_string()),
            }
        } else if upper_test == "STARTTLS" {
            helo.tls = true
        }
    }

    println!("{:?}", helo);
    Ok(helo)
}

fn connect(domain: &str) -> Result<(TcpStream, String), Cow<str>> {
    let resolver = Resolver::default().unwrap();

    // Resolve the MX records
    let mx_records = resolver
        .mx_lookup(domain)
        .and_then(|mx| Ok(mx.into_iter().map(|d| d.exchange().to_string())));

    // Try to connect to the MX records
    if let Ok(mx) = mx_records {
        for domain in mx {
            println!("Trying: {}", domain);
            match TcpStream::connect(domain.clone() + ":25") {
                Ok(ts) => return Ok((ts, domain)),
                Err(_) => (),
            }
        }
    }

    println!("Unable to find a MX record trying itself: {}", domain);

    // Failback see if the domain itself has a mail server
    TcpStream::connect(domain.to_string() + ":25")
        .map(|stream| (stream, domain.to_string()))
        .map_err(|_| "Unable to connect to the mail server".into())
}

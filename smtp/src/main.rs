use std::{
    fs::File,
    io::{Read, Write},
};

use chrono::Utc;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};
// use smtp::{read_line, write, write_line};

#[tokio::main]
async fn main() {
    // let db = db::connect_using_env();
    let listener = TcpListener::bind("0.0.0.0:25").await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            process(socket).await;
        });
    }
}

async fn process(mut socket: TcpStream) {
    socket.write("Hello".as_bytes()).await;

    // socket.read_
    // native_tls::
    println!("Hello");

    loop {}
}

//     let listener = TcpListener::bind("0.0.0.0:25").unwrap();

//     for stream in listener.incoming() {
//         if let Ok(mut tcpstream) = stream {
//             if let Err(e) = handle_stream(&mut tcpstream) {
//                 println!("Error with stream: {}", e);
//                 let _ = write(&mut tcpstream, "451 Internal Error: ");
//                 let _ = write_line(tcpstream, e.as_str());
//             }
//         }
//     }
// }

// fn handle_stream(mut stream: &mut TcpStream) -> Result<(), String> {
//     let address = stream
//         .peer_addr()
//         .map_err(|e| format!("Error retreving peer address: {}", e))?;

//     let message = format!("220 fioxia.local ESMTP Hello [{}] - fsmtp", address.ip());
//     write_line(&mut stream, message.as_str())?;

//     loop {
//         match message_loop(
//             Transaction {
//                 stage: Stage::Helo,
//                 helo: String::new(),
//                 mail_from: String::new(),
//                 rcpt_to: Vec::new(),
//                 data: String::new(),
//             },
//             &mut stream,
//         ) {
//             Ok(t) if t.stage == Stage::QUIT => {
//                 write_line(&mut stream, "221 closing connection")?;
//                 break;
//             }
//             Ok(t) => {
//                 let path = format!(
//                     "emails/email-{}-{}",
//                     t.mail_from,
//                     Utc::now().timestamp_millis()
//                 );

//                 println!("{}", path);

//                 let mut f = File::create(path).map_err(|e| format!("{}", e))?;
//                 f.write(t.data.as_bytes()).map_err(|e| format!("{}", e))?;
//                 write_line(&mut stream, "250 Message sent :)")?;
//             }
//             Err(e) => return Err(e),
//         }
//     }
//     Ok(())
// }

// #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
// enum Stage {
//     Helo,
//     MailFrom,
//     RcptTo,
//     QUIT,
// }

// struct Transaction {
//     stage: Stage,
//     helo: String,
//     mail_from: String,
//     rcpt_to: Vec<String>,
//     data: String,
// }

// fn message_loop(
//     mut transaction: Transaction,
//     mut stream: impl Read + Write,
// ) -> Result<Transaction, String> {
//     loop {
//         let mut buffer = [0; 128];
//         let question = read_line(&mut stream, &mut buffer)?;
//         let question = question.trim();

//         let test = question.to_uppercase();

//         let result = if test.starts_with("QUIT") {
//             transaction.stage = Stage::QUIT;
//             return Ok(transaction);
//         } else if test.starts_with("DATA") {
//             return data(transaction, &mut stream);
//         } else if test.starts_with("HELO") {
//             parse_and_respond_helo(&question[4..], &mut transaction, &mut stream)
//         } else if test.starts_with("EHLO") {
//             parse_and_respond_ehlo(&question[4..], &mut transaction, &mut stream)
//         } else if test.starts_with("RCPT TO") {
//             rcpt_to(&question[7..], &mut transaction, &mut stream)
//         } else if test.starts_with("MAIL FROM") {
//             mail_from(&question[9..], &mut transaction, &mut stream)
//         } else {
//             Err(format!("502 Command {} not found", question))
//         };

//         if let Err(e) = result {
//             println!("Err: {}", e);
//             write_line(&mut stream, e.as_str())?;
//         }
//     }
// }

// fn data(
//     mut transaction: Transaction,
//     mut stream: impl Read + Write,
// ) -> Result<Transaction, String> {
//     write_line(
//         &mut stream,
//         "354 Ready, please finish with <CR><LF>.<CR><LF>",
//     )?;

//     for _ in 0..0xFFFF_FF {
//         let mut buffer = [0; 512];
//         let line = read_line(&mut stream, &mut buffer)?;

//         transaction.data.push_str(line.to_string().as_str());

//         if transaction.data.ends_with("\r\n.\r\n") {
//             transaction.data.pop();
//             transaction.data.pop();
//             transaction.data.pop();
//             return Ok(transaction);
//         }
//     }

//     Err("Timed out reading data".into())
// }
// const HELO: &str = "250 fioxia.local ready for mail";
// const EHLO: &[&str] = &["250-fioxia.local ready for mail"];

// fn parse_and_respond_helo(
//     rest: &str,
//     transaction: &mut Transaction,
//     stream: &mut (impl Read + Write),
// ) -> Result<(), String> {
//     if rest.len() < 2 {
//         return Err("501 Empty HELO is not allowed".into());
//     }

//     if transaction.stage == Stage::Helo {
//         transaction.stage = Stage::MailFrom
//     }

//     transaction.helo = rest[1..].to_string();

//     write_line(stream, HELO)?;
//     Ok(())
// }

// fn parse_and_respond_ehlo(
//     rest: &str,
//     transaction: &mut Transaction,
//     mut stream: impl Read + Write,
// ) -> Result<(), String> {
//     if rest.len() < 2 {
//         return Err("501 Empty EHLO is not allowed".into());
//     }

//     if transaction.stage == Stage::Helo {
//         transaction.stage = Stage::MailFrom
//     }

//     transaction.helo = rest[1..].to_string();

//     println!("HELO: {:?}", EHLO.join("\r\n"));

//     write_line(&mut stream, EHLO.join("\r\n").as_str())?;

//     Ok(())
// }

// fn parse_email(email: &str) -> Result<&str, String> {
//     if email.len() < 5 {
//         return Err("555 Syntax error".into());
//     }
//     if &email[0..1] != ":" {
//         return Err(format!("555 Syntax error, expected (:) found: ({})", &email[0..1]).into());
//     }

//     let bounds = email[1..].trim();
//     bounds
//         .strip_prefix('<')
//         .and_then(|e| e.strip_suffix('>'))
//         .ok_or_else(|| "555 Syntax error expect email to be enclosed within (< >)".to_string())
// }

// fn mail_from(
//     rest: &str,
//     transaction: &mut Transaction,
//     mut stream: impl Read + Write,
// ) -> Result<(), String> {
//     if transaction.stage == Stage::Helo {
//         return Err("503 EHLO / HELO First".into());
//     }

//     if transaction.stage > Stage::MailFrom {
//         return Err("503 Sender has already been specified".into());
//     }

//     let email = parse_email(rest.trim())?;

//     transaction.stage = Stage::RcptTo;

//     transaction.mail_from = email.to_string();

//     write_line(&mut stream, "250 Ok")?;

//     Ok(())
// }

// fn rcpt_to(
//     rest: &str,
//     transaction: &mut Transaction,
//     mut stream: impl Read + Write,
// ) -> Result<(), String> {
//     if transaction.stage < Stage::RcptTo {
//         return Err("503 MAIL FROM first".into());
//     }

//     let email = parse_email(rest.trim())?;

//     transaction.rcpt_to.push(email.to_string());

//     write_line(&mut stream, "250 Ok")?;

//     Ok(())
// }

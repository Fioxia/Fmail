use std::{env, net::SocketAddr, path::Path, pin::Pin, sync::Arc, time::SystemTime};

// use db::{
//     connect_using_env,
//     mailbox::{mailbox, MailBox, NewMail},
//     user::{user, User},
// };
// use diesel::QueryDsl;
use mail_exchange::{get_command, parse_email, Command, CommandLine, Error, Stream};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::{
    fs,
    net::{TcpListener, TcpStream},
};

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    println!("Listening with name: {}", env::var("HOSTNAME").unwrap());
    // let db = db::connect_using_env();
    let listener = TcpListener::bind("0.0.0.0:25").await.unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(env::var("DATABASE_URL").unwrap().as_str())
        .await
        .unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        println!("Conn");
        let new_pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = process(socket, new_pool).await {
                println!("Error: {:?}", e);
            }
        });
    }
}

async fn process(socket: TcpStream, db_pool: PgPool) -> Result<(), Error> {
    let addr = socket.peer_addr()?;
    let greet = format!(
        "220 {} ESMTP Hello [{}]",
        env::var("HOSTNAME").unwrap(),
        addr
    );
    let mut stream = Stream::new(socket);
    stream.write_line(greet.as_str()).await?;

    loop {
        let data = command_loop(&mut stream, addr).await;

        if let Err(Error::Quit) = data {
            stream.write_line("221 closing connection").await?;
            return Ok(());
        }

        if let Err(Error::StartTls) = data {
            stream = stream.start_tls_server().await?;
            continue;
        }

        let (server, from, to, data) = data?;

        for receiver in to {
            match sqlx::query!(
                "INSERT INTO email (sender, receiver, server, data) VALUES ($1, $2, $3, $4) RETURNING id",
                &from,
                &receiver,
                &server.addr.to_string(),
                &data
            )
            .fetch_one(&db_pool)
            .await {
                Ok(id) => println!("Success: {}", id.id),
                Err(e) => println!("Failed: {:?}", e),
            }
        }
    }
}

#[derive(Debug)]
struct Server {
    addr: SocketAddr,
    helo: Vec<(bool, String)>,
}

#[derive(Debug)]
enum Stage {
    Connect,
    Helo,
    MailFrom(String),
    RcptTo(String, Vec<String>),
}

async fn command_loop(
    socket: &mut Stream,
    addr: SocketAddr,
) -> Result<(Server, String, Vec<String>, String), Error> {
    let mut stage = Stage::Connect;
    let mut server = Server {
        addr,
        helo: Vec::new(),
    };
    for _ in 0..1000 {
        let line = socket.read_line().await?;

        if let Some(command) = get_command(line) {
            match (command.command, &stage) {
                (Command::Quit, _) => return Err(Error::Quit),
                (Command::Helo, _) => {
                    stage = respond_helo(socket, command.rest_of_line, false, stage, &mut server)
                        .await?;
                }
                (Command::Ehlo, _) => {
                    stage = respond_helo(socket, command.rest_of_line, true, stage, &mut server)
                        .await?;
                }
                (Command::StartTls, _) => return Err(Error::StartTls),
                (Command::MailFrom, Stage::Helo) => {
                    stage = mail_from(socket, command.rest_of_line, stage).await?;
                }
                (Command::RcptTo, Stage::MailFrom(mail_from)) => {
                    stage = rcpt_to(socket, command.rest_of_line, mail_from.clone(), Vec::new())
                        .await?;
                }
                (Command::RcptTo, Stage::RcptTo(mail_from, rcpt)) => {
                    stage = rcpt_to(
                        socket,
                        command.rest_of_line,
                        mail_from.clone(),
                        rcpt.clone(),
                    )
                    .await?;
                }
                (Command::Data, Stage::RcptTo(mail_from, rcpt_to)) => {
                    return Ok((
                        server,
                        mail_from.clone(),
                        rcpt_to.clone(),
                        data(socket).await?,
                    ));
                }
                _ => {
                    socket.write_line("503 Bad sequence of commands").await?;
                    continue;
                }
            }
        } else {
            socket.write_line("502 Unknown command").await?;
            continue;
        }
    }

    Err(Error::Timeout)
}

async fn respond_helo(
    socket: &mut Stream,
    string: String,
    new_helo: bool,
    stage: Stage,
    server: &mut Server,
) -> Result<Stage, Error> {
    if string.len() < 2 {
        socket
            .write_line("501 Empty HELO/EHLO is not allowed")
            .await?;
        return Ok(stage);
    }

    server.helo.push((new_helo, string));

    if new_helo {
        socket
            .write_line(
                format!(
                    "250-{} ready for mail\r\n250 STARTTLS",
                    env::var("HOSTNAME").unwrap()
                )
                .as_str(),
            )
            .await?
    } else {
        socket
            .write_line(format!("250 {} ready for mail", env::var("HOSTNAME").unwrap()).as_str())
            .await?
    }

    // Promote the connection stages
    match stage {
        Stage::Connect => Ok(Stage::Helo),
        _ => Ok(stage),
    }
}

async fn mail_from(socket: &mut Stream, rest: String, stage: Stage) -> Result<Stage, Error> {
    let email = match parse_email(rest.as_str()) {
        Ok(e) => e,
        Err(e) => {
            socket.write_line(e.as_str()).await?;
            return Ok(stage);
        }
    };
    socket.write_line("250 Ok").await?;
    Ok(Stage::MailFrom(email.to_string()))
}

async fn rcpt_to(
    socket: &mut Stream,
    rest: String,
    mail_from: String,
    mut rcpt: Vec<String>,
) -> Result<Stage, Error> {
    let email = match parse_email(rest.as_str()) {
        Ok(e) => e,
        Err(e) => {
            socket.write_line(e.as_str()).await?;
            return Ok(Stage::RcptTo(mail_from, rcpt));
        }
    };

    rcpt.push(email.to_string());

    socket.write_line("250 Ok").await?;
    Ok(Stage::RcptTo(mail_from, rcpt))
}

async fn data(socket: &mut Stream) -> Result<String, Error> {
    socket
        .write_line("354 Ready, please finish with <CR><LF>.<CR><LF>")
        .await?;

    let mut data = String::new();

    loop {
        let line = socket.read_line().await?;

        if line == "." {
            println!("Break");
            break;
        }

        data.push_str(line.as_str());
        data.push('\n');
    }

    println!("Sent email");
    socket.write_line("250 Sent email :)").await?;

    Ok(data)
}

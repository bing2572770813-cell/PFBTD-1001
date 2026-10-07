use clap::Parser;
use reqwest::blocking::Client;
use rm_client_sync::{echo_body, exchange, multiline_text};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:7878")]
    url: String,
}

fn input(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn multiline_input() -> io::Result<String> {
    println!("Enter text line by line; `.` ends input and `\\.` means a literal `.` line.");
    let mut lines = Vec::new();
    loop {
        let line = input("text> ")?;
        let done = line == ".";
        lines.push(line);
        if done {
            break;
        }
    }
    Ok(multiline_text(lines))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut token = String::new();
    loop {
        let command = match input(
            "ping / register / login / logout / list / echo / delete-user / put / get / delete / q > ",
        ) {
            Ok(command) => command,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        };
        let (method, path, body): (&str, String, Option<Value>) = match command.as_str() {
            "q" => break,
            "ping" => ("GET", "/ping".to_owned(), None),
            "list" => ("GET", "/texts".to_owned(), None),
            "logout" => ("DELETE", "/sessions/current".to_owned(), None),
            "delete-user" => ("DELETE", "/users/me".to_owned(), None),
            "echo" => (
                "POST",
                "/echo".to_owned(),
                Some(echo_body(multiline_input()?)),
            ),
            "register" | "login" => {
                let body = json!({
                    "username": input("username: ")?,
                    "password": rpassword::prompt_password("password: ")?,
                });
                (
                    "POST",
                    if command == "register" {
                        "/users".to_owned()
                    } else {
                        "/sessions".to_owned()
                    },
                    Some(body),
                )
            }
            "put" => {
                let name = input("name: ")?;
                (
                    "PUT",
                    format!("/texts/{name}"),
                    Some(echo_body(multiline_input()?)),
                )
            }
            "get" | "delete" => {
                let name = input("name: ")?;
                (
                    if command == "get" { "GET" } else { "DELETE" },
                    format!("/texts/{name}"),
                    None,
                )
            }
            _ => {
                println!("Unknown command.");
                continue;
            }
        };
        let result = exchange(
            &client,
            &args.url,
            method.parse().unwrap(),
            &path,
            &token,
            body.as_ref(),
        );
        match result {
            Ok((status, value)) => {
                println!("{status} {value}");
                if command == "login"
                    && status == 200
                    && let Some(next) = value["data"]["token"].as_str()
                {
                    token = next.into();
                }
                if status == 401 {
                    println!("Please log in again.");
                }
                if status == 401
                    || ((command == "logout" || command == "delete-user") && status == 200)
                {
                    token.clear();
                }
            }
            Err(error) => eprintln!("Request failed: {error}"),
        }
    }
    Ok(())
}

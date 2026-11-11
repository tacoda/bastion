//! `bastion`: Bastion on the command line (PLAN.md I4). It reaches a server
//! only through the client library (D38): the local one, which it starts
//! if nothing runs, or one added in the desktop's Settings.

mod update;

use bastion_client::client::{self, send};
use bastion_client::events::{self, Feed};
use bastion_client::start;
use bastion_protocol::api::{self, Request};
use bastion_protocol::{Change, JobSummary, LOCAL, Server};
use futures::StreamExt;

const USAGE: &str = "usage: bastion [--server NAME] COMMAND

commands:
  servers                      the Bastion servers this machine knows
  jobs                         the server's jobs, newest first
  submit WORKSPACE REQUEST...  ask for a change; prints the new job's id
  approve JOB                  approve the step the job waits on
  comment JOB TEXT...          send the job back, or answer its question
  retry JOB                    run the failed job, or its stuck step, again
  extend JOB                   give the running turn its full time again
  stop JOB                     stop the job
  follow                       print each change the server pushes
  update                       install the newest release, once its signature checks";

/// One parsed command line: the server to use and what to do there.
#[derive(Debug, PartialEq)]
struct Line {
    server: String,
    command: Command,
}

#[derive(Debug, PartialEq)]
enum Command {
    Servers,
    Jobs,
    Submit { workspace: String, request: String },
    Approve(String),
    Comment { job: String, text: String },
    Retry(String),
    Extend(String),
    Stop(String),
    Follow,
}

fn parse(args: &[String]) -> Result<Line, String> {
    let (server, rest) = match args {
        [flag, name, rest @ ..] if flag == "--server" => (name.clone(), rest),
        rest => (LOCAL.to_string(), rest),
    };
    let (name, rest) = rest.split_first().ok_or(USAGE)?;
    let command = command(name, rest).ok_or(USAGE)?;
    Ok(Line { server, command })
}

/// The command `name` with its arguments `args`, or none if they do not fit.
fn command(name: &str, args: &[String]) -> Option<Command> {
    let one = || match args {
        [a] => Some(a.clone()),
        _ => None,
    };
    // A first argument, then words that make one text.
    let then_text = || match args {
        [first, rest @ ..] if !rest.is_empty() => Some((first.clone(), rest.join(" "))),
        _ => None,
    };
    match (name, args.is_empty()) {
        ("servers", true) => Some(Command::Servers),
        ("jobs", true) => Some(Command::Jobs),
        ("follow", true) => Some(Command::Follow),
        ("approve", _) => one().map(Command::Approve),
        ("retry", _) => one().map(Command::Retry),
        ("extend", _) => one().map(Command::Extend),
        ("stop", _) => one().map(Command::Stop),
        ("comment", _) => then_text().map(|(job, text)| Command::Comment { job, text }),
        ("submit", _) => {
            then_text().map(|(workspace, request)| Command::Submit { workspace, request })
        }
        _ => None,
    }
}

/// One job as a line: id, where it is, and what was asked.
fn row(j: &JobSummary) -> String {
    let at = format!("{} {}", j.stage, j.state);
    let asked: String = j
        .request
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    format!("{:<11} {:<22} {:<14} {asked}", j.id, at, j.workspace)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run(args: &[String]) -> Result<(), String> {
    // An update needs no server.
    if args == ["update"] {
        return update::run().await;
    }
    let line = parse(args)?;
    let local = tokio::task::spawn_blocking(start::start)
        .await
        .map_err(|e| e.to_string())??;
    let every = start::every(local)?;
    if line.command == Command::Servers {
        every
            .iter()
            .for_each(|s| println!("{:<12} {}", s.name, s.url));
        return Ok(());
    }
    let server = every
        .into_iter()
        .find(|s| s.name == line.server)
        .ok_or_else(|| format!("no Bastion server named {}", line.server))?;
    act(line.command, server).await
}

/// Do `command` on `server`.
async fn act(command: Command, server: Server) -> Result<(), String> {
    let http = client::http();
    match command {
        Command::Servers => Ok(()),
        Command::Jobs => jobs(&http, &server).await,
        Command::Submit { workspace, request } => {
            let submit = api::JobSubmit {
                workspace,
                request,
                pipeline: None,
                workflow: None,
                model: None,
                ..Default::default()
            };
            println!("{}", send(&http, &server, &submit).await?);
            Ok(())
        }
        Command::Approve(id) => done(&http, &server, api::JobApprove { id }).await,
        Command::Retry(id) => done(&http, &server, api::JobRetry { id }).await,
        Command::Extend(id) => done(&http, &server, api::JobExtend { id }).await,
        Command::Stop(id) => done(&http, &server, api::JobStop { id }).await,
        Command::Comment { job, text } => {
            done(&http, &server, api::JobComment { id: job, text }).await
        }
        Command::Follow => follow(server).await,
    }
}

async fn jobs(http: &reqwest::Client, server: &Server) -> Result<(), String> {
    let all = send(http, server, &api::Jobs).await?;
    all.iter().for_each(|j| println!("{}", row(j)));
    Ok(())
}

/// Send a request that answers nothing, and say it worked.
async fn done<R: Request<Answer = ()>>(
    http: &reqwest::Client,
    server: &Server,
    request: R,
) -> Result<(), String> {
    send(http, server, &request).await?;
    println!("ok");
    Ok(())
}

/// Print each push until interrupted.
async fn follow(server: Server) -> Result<(), String> {
    let (out, mut feed) = futures::channel::mpsc::channel(64);
    tokio::spawn(events::follow(server, out));
    while let Some((name, push)) = feed.next().await {
        let said = match push {
            Feed::Up => "connected".to_string(),
            Feed::Down => "disconnected; trying again".to_string(),
            Feed::Resync => "changes may have been missed; read again".to_string(),
            Feed::Change(Change::Job { id }) if id.is_empty() => "every job changed".into(),
            Feed::Change(Change::Job { id }) => format!("job {id} changed"),
            Feed::Change(Change::Engine { status }) => format!("engine {:?}", status.link),
            Feed::Change(Change::Console) => "a worker joined or left".into(),
            Feed::Change(Change::Notice { text, waiting, .. }) => {
                format!("{text} ({waiting} waiting on a person)")
            }
        };
        println!("{name}: {said}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn a_command_line_names_its_server_and_its_command() {
        let line = parse(&words("--server box comment j1 add a test")).unwrap();
        assert_eq!(line.server, "box");
        assert_eq!(
            line.command,
            Command::Comment {
                job: "j1".into(),
                text: "add a test".into()
            }
        );
        let local = parse(&words("submit demo add negate")).unwrap();
        assert_eq!(local.server, LOCAL);
        assert_eq!(
            local.command,
            Command::Submit {
                workspace: "demo".into(),
                request: "add negate".into()
            }
        );
        assert!(parse(&words("comment j1")).is_err());
        assert!(parse(&words("launch")).is_err());
        assert!(parse(&[]).is_err());
    }
}

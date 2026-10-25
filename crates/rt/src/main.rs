mod agent;
mod connect;
mod init;
mod manage;
mod output;
mod review;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rt",
    version,
    about = "review-tools: review sessions for agentic development",
    after_help = "Agents: run `rt agent-instructions` for how to use review sessions."
)]
struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Review sessions: open, revise, chat, wait for the human, hand off, close.
    #[command(subcommand)]
    Review(review::ReviewCommand),
    /// Work packages: a milestone's worth of work.
    #[command(subcommand)]
    Wp(manage::WpCommand),
    /// Projects: the repositories the instance's app is installed on.
    #[command(subcommand)]
    Project(manage::ProjectCommand),
    /// Connect this repository to a review-tools instance in the cloud: sign in, install
    /// its GitHub App on the repository if needed, and get an agent token, all in the
    /// browser. Remembered for this repository only.
    Init(init::InitArgs),
    /// Print the snippet for a project's agent instructions file (CLAUDE.md, AGENTS.md).
    AgentInstructions,
    /// Which server rt talks to, and who that server takes it for: a quick check of
    /// RT_ENDPOINT and the token (RT_TOKEN, or a Claude Code network secret).
    Whoami,
}

/// An error that has already been reported, carrying the exit code to use.
#[derive(Debug)]
pub struct Exit(pub u8);

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit {}", self.0)
    }
}

impl std::error::Error for Exit {}

fn main() -> ExitCode {
    // Exit quietly when the reader of our output goes away (`rt … | head`).
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let json = cli.json;
    let result = match cli.command {
        Command::Review(cmd) => review::run(cmd, json),
        Command::Wp(cmd) => manage::wp(cmd, json),
        Command::Project(cmd) => manage::project(cmd, json),
        Command::AgentInstructions => {
            print!("{}", agent::INSTRUCTIONS);
            Ok(())
        }
        Command::Whoami => manage::whoami(json),
        Command::Init(args) => init::run(args, json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => match e.downcast_ref::<Exit>() {
            Some(Exit(code)) => ExitCode::from(*code),
            None => {
                eprintln!("rt: {e:#}");
                ExitCode::FAILURE
            }
        },
    }
}

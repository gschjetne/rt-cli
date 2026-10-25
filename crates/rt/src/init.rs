//! `rt init`: connect this repository to a review-tools instance. The
//! browser does the signing in, installing the app and confirming; rt listens on a
//! loopback port for the way back, and exchanges the one-time code it brings, with a PKCE
//! verifier only this process knows, for an agent token (RFC 8252 and RFC 7636).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rt_api::ConnectExchange;
use rt_client::{Client, segment};
use sha2::{Digest, Sha256};

/// How long to wait for the browser.
const PATIENCE: Duration = Duration::from_secs(15 * 60);

/// The instance `rt init` connects to, unless `RT_ENDPOINT` names another.
pub const DEFAULT_INSTANCE: &str = "https://rt.schjetne.dev";

#[derive(clap::Args)]
pub struct InitArgs {
    /// Print the address to open instead of opening a browser.
    #[arg(long)]
    no_browser: bool,
}

fn random(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the operating system provides random bytes");
    URL_SAFE_NO_PAD.encode(buf)
}

fn machine_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this machine".to_owned())
}

/// Open a URL in the person's browser: `RT_BROWSER` if set, else what the platform has
/// (on WSL, the Windows browser). Whether something was started.
fn open_browser(url: &str) -> bool {
    let run = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    };
    if let Ok(browser) = std::env::var("RT_BROWSER")
        && !browser.is_empty()
    {
        return run(&browser, &[url]);
    }
    if cfg!(target_os = "macos") {
        return run("open", &[url]);
    }
    let wsl = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|r| r.to_lowercase().contains("microsoft"));
    if wsl {
        if run("wslview", &[url]) {
            return true;
        }
        let quoted = format!("Start-Process '{}'", url.replace('\'', "''"));
        return run("powershell.exe", &["-NoProfile", "-Command", &quoted]);
    }
    run("xdg-open", &[url])
}

/// The repository's id on GitHub, from the `gh` CLI where it is installed and signed in.
/// The install page can then choose this repository alone in advance, which it cannot do
/// for a private repository the app is not on yet without being told its id.
fn repo_id_from_gh(full_name: &str) -> Option<u64> {
    let output = std::process::Command::new("gh")
        .args(["api", &format!("repos/{full_name}"), "--jq", ".id"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().parse().ok())
        .flatten()
}

/// What came back: from the browser, or pasted.
enum Outcome {
    Code(String),
    Denied,
}

/// Answer the browser's request on the loopback port; the outcome, if it is the one we
/// wait for.
fn serve_one(mut stream: TcpStream, state: &str) -> Option<anyhow::Result<Outcome>> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    let target = line.split_whitespace().nth(1)?.to_owned();
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let param = |name: &str| {
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == name).then(|| {
                percent_encoding::percent_decode_str(v)
                    .decode_utf8_lossy()
                    .into_owned()
            })
        })
    };
    let (status, message, outcome) = if param("state").as_deref() != Some(state) {
        // Something else knocking on the port: not ours.
        (
            "400 Bad Request",
            "This is not the request rt is waiting for.",
            None,
        )
    } else if let Some(code) = param("code") {
        (
            "200 OK",
            "rt has what it needs. You can close this tab and go back to the terminal.",
            Some(Ok(Outcome::Code(code))),
        )
    } else {
        (
            "200 OK",
            "You cancelled. rt stops; run `rt init` again any time.",
            Some(Ok(Outcome::Denied)),
        )
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>rt</title>\
         <body style=\"font-family:system-ui,sans-serif;max-width:36rem;margin:4rem auto;padding:0 1rem\">\
         <h1>rt</h1><p>{message}</p></body>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    outcome
}

pub fn run(args: InitArgs, json: bool) -> anyhow::Result<()> {
    let repo = crate::review::repo_root();
    let top = rt_client::repo_top(std::path::Path::new(&repo))
        .context("rt init runs inside a git repository")?;
    let remote = crate::review::origin_remote(&repo);
    let Some(full_name) = remote
        .as_deref()
        .filter(|r| r.contains("github.com"))
        .and_then(rt_api::remote::repo_full_name)
    else {
        let found = match &remote {
            Some(r) => format!("its `origin` remote, {r}, is not on GitHub"),
            None => "it has no `origin` remote".to_owned(),
        };
        bail!("rt init connects a repository on GitHub to a review-tools instance, and {found}.");
    };

    let endpoint = std::env::var("RT_ENDPOINT")
        .ok()
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_INSTANCE.to_owned())
        .trim_end_matches('/')
        .to_owned();
    if !rt_client::may_carry_token(&endpoint) {
        bail!(
            "{endpoint} is plain HTTP: the token rt init gets would cross the network readable by anyone on the way; use the https:// address"
        );
    }
    let client = Client::new(&endpoint, None);
    let health = client
        .health()
        .with_context(|| format!("{endpoint} does not answer as a review-tools instance"))?;
    if health.service != "review-tools" {
        bail!("{endpoint} is not a review-tools instance");
    }
    if !health.sign_in {
        bail!("{endpoint} has no sign-in to connect to");
    }

    let listener = TcpListener::bind("127.0.0.1:0").context("listening on a loopback port")?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let verifier = random(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random(16);
    let mut url = format!(
        "{endpoint}/connect?repo={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&client={}",
        segment(&full_name),
        segment(&redirect_uri),
        segment(&state),
        segment(&challenge),
        segment(&machine_name()),
    );
    if let Some(id) = repo_id_from_gh(&full_name) {
        url.push_str(&format!("&repo_id={id}"));
    }

    eprintln!("Connecting {full_name} to {endpoint}.");
    let opened = !args.no_browser && open_browser(&url);
    if opened {
        eprintln!("Your browser should open; if it does not, open:\n\n  {url}\n");
    } else {
        eprintln!("Open this in your browser:\n\n  {url}\n");
    }
    eprintln!("Waiting for the browser… (or paste the code it shows here, then Enter)");

    let (tx, rx) = mpsc::channel::<anyhow::Result<Outcome>>();
    {
        let tx = tx.clone();
        let state = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Some(outcome) = serve_one(stream, &state) {
                    let _ = tx.send(outcome);
                    return;
                }
            }
        });
    }
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.lock().read_line(&mut line).is_ok_and(|n| n > 0) {
            let code = line.trim().to_owned();
            if !code.is_empty() {
                let _ = tx.send(Ok(Outcome::Code(code)));
                return;
            }
            line.clear();
        }
    });
    let code = match rx.recv_timeout(PATIENCE) {
        Ok(Ok(Outcome::Code(code))) => code,
        Ok(Ok(Outcome::Denied)) => bail!("cancelled in the browser"),
        Ok(Err(e)) => return Err(e),
        Err(_) => bail!("the browser did not come back within 15 minutes; run rt init again"),
    };

    let connected = client.connect_exchange(&ConnectExchange {
        code,
        code_verifier: verifier,
        redirect_uri,
    })?;
    let config = rt_client::save_repo_config(&top, &endpoint, &connected.token)?;
    if json {
        return crate::output::json(&serde_json::json!({
            "endpoint": endpoint,
            "project": connected.project,
            "token": connected.record,
            "config": config,
        }));
    }
    println!(
        "Connected {} to {endpoint} as project {}, with agent token `{}` (yours, until {}).",
        top.display(),
        connected.project,
        connected.record.name,
        connected
            .record
            .expires_at
            .map_or("revoked".into(), |t| t.strftime("%Y-%m-%d").to_string()),
    );
    println!(
        "Saved in {} for this repository only. rt review … here now uses the instance; \
         `rt whoami` shows it.",
        config.display()
    );
    Ok(())
}

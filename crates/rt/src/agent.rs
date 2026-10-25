//! Who the agent is: the harness and agent session behind this CLI invocation.

use rt_api::AttachAgent;

/// The snippet `rt agent-instructions` prints.
pub const INSTRUCTIONS: &str = include_str!("../../../docs/agent/agent-instructions.md");

#[derive(clap::Args, Clone, Default)]
pub struct AgentArgs {
    /// The agent harness, such as claude-code. Detected for Claude Code; otherwise from
    /// RT_AGENT_HARNESS.
    #[arg(long)]
    pub harness: Option<String>,
    /// The harness's id for this agent session (default: CLAUDE_CODE_SESSION_ID or
    /// RT_AGENT_SESSION).
    #[arg(long = "agent-session")]
    pub session_ref: Option<String>,
    /// A link that opens this agent session, shown to the human (default: RT_AGENT_URL).
    #[arg(long = "agent-url")]
    pub url: Option<String>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Quote a path for a POSIX shell if it needs it.
fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~+@%".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The claude.ai link for a Claude Code session id. Cloud sessions see their id as
/// `cse_…` in the environment; the link names the same session `session_…`.
fn claude_link(id: &str) -> Option<String> {
    let rest = id
        .strip_prefix("session_")
        .or_else(|| id.strip_prefix("cse_"))?;
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| format!("https://claude.ai/code/session_{rest}"))
}

impl AgentArgs {
    /// The binding request for this agent session; `repo` is where it runs.
    pub fn detect(&self, repo: &str) -> AttachAgent {
        let claude = env("CLAUDE_CODE_SESSION_ID");
        let harness = self
            .harness
            .clone()
            .or_else(|| env("RT_AGENT_HARNESS"))
            .or_else(|| {
                (claude.is_some() || env("CLAUDECODE").is_some()).then(|| "claude-code".to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned());
        let session_ref = self
            .session_ref
            .clone()
            .or_else(|| env("RT_AGENT_SESSION"))
            .or_else(|| {
                if harness == "claude-code" {
                    claude.clone()
                } else {
                    None
                }
            });
        // A Claude Code session reachable on claude.ai: a cloud session, or a local one
        // with Remote Control on. The page's Open button goes there.
        let remote = env("CLAUDE_CODE_REMOTE_SESSION_ID");
        let bridge = env("CLAUDE_CODE_BRIDGE_SESSION_ID");
        let claude_url = (harness == "claude-code")
            .then(|| remote.clone().or_else(|| bridge.clone()))
            .flatten()
            .and_then(|id| claude_link(&id));
        let url = self
            .url
            .clone()
            .or_else(|| env("RT_AGENT_URL"))
            .or(claude_url);
        // A local session can also be resumed in a terminal; a cloud one only on claude.ai.
        let resume = match (&*harness, &session_ref) {
            ("claude-code", Some(id))
                if remote.is_none() && (url.is_none() || bridge.is_some()) =>
            {
                Some(format!("cd {} && claude --resume {id}", shell_quote(repo)))
            }
            _ => None,
        };
        AttachAgent {
            harness,
            session_ref,
            url,
            resume,
        }
    }

    /// The key the CLI remembers this agent session's bindings under.
    pub fn key(&self, repo: &str) -> Option<String> {
        let agent = self.detect(repo);
        agent.session_ref.map(|r| format!("{}:{r}", agent.harness))
    }
}

#[cfg(test)]
mod tests {
    use super::{claude_link, shell_quote};

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/home/me/repo"), "/home/me/repo");
        assert_eq!(shell_quote("/home/me/my repo"), "'/home/me/my repo'");
        assert_eq!(shell_quote("/it's"), r"'/it'\''s'");
    }

    #[test]
    fn claude_links() {
        let link = Some("https://claude.ai/code/session_01Abc".to_owned());
        assert_eq!(claude_link("session_01Abc"), link);
        assert_eq!(claude_link("cse_01Abc"), link);
        assert_eq!(claude_link("cse_"), None);
        assert_eq!(claude_link("01Abc"), None);
        assert_eq!(claude_link("session_01/../x"), None);
    }
}

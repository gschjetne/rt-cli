//! What the CLI remembers between invocations: the current review session per working
//! directory, and the agent binding each agent session holds in each review session.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientState {
    /// Review session last opened or attached, by repository root or directory.
    #[serde(default)]
    pub current: BTreeMap<String, String>,
    #[serde(default)]
    pub bindings: Vec<BindingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingEntry {
    pub session: String,
    /// The agent session (harness and reference) holding the binding, if known.
    pub agent: Option<String>,
    pub binding: String,
}

impl ClientState {
    pub fn load() -> Self {
        paths::read_own(&paths::client_state_file())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::client_state_file();
        if let Some(dir) = path.parent() {
            crate::paths::private_dir(dir)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            use std::io::Write;
            // Which reviews this user works on, and their agent sessions: theirs to read.
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
        }
        std::fs::rename(tmp, path)
    }

    pub fn binding(&self, session: &str, agent: Option<&str>) -> Option<&str> {
        self.bindings
            .iter()
            .rev()
            .find(|b| b.session == session && b.agent.as_deref() == agent)
            .map(|b| b.binding.as_str())
    }

    pub fn set_binding(&mut self, session: &str, agent: Option<&str>, binding: &str) {
        self.bindings
            .retain(|b| !(b.session == session && b.agent.as_deref() == agent));
        self.bindings.push(BindingEntry {
            session: session.to_owned(),
            agent: agent.map(str::to_owned),
            binding: binding.to_owned(),
        });
        // Keep the file small: the most recent bindings are the ones that matter.
        let excess = self.bindings.len().saturating_sub(200);
        self.bindings.drain(..excess);
    }
}

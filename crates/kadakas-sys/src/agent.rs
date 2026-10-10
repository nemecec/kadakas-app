//! The runtime file that tells agent clients how to reach a running app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsio;

/// How to reach a running app: its port and the secret that clients must send first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub port: u16,
    pub token: String,
}

/// Where the running app publishes its `AgentInfo`: `agent.json` in the runtime folder.
pub fn agent_info_path() -> Option<PathBuf> {
    Some(crate::dirs::runtime_dir()?.join("agent.json"))
}

/// Writes the runtime file with owner-only permissions (0600, in a 0700 folder).
pub fn write_agent_info(path: &Path, info: &AgentInfo) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fsio::create_private_dir(dir)?;
    }
    let text = serde_json::to_vec(info).map_err(std::io::Error::other)?;
    fsio::write_private(path, &text)
}

pub fn read_agent_info(path: &Path) -> Result<AgentInfo, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_info_round_trips_and_is_private() {
        let dir = std::env::temp_dir().join(format!("kadakas-agentinfo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("sub").join("agent.json");
        let info = AgentInfo {
            port: 4242,
            token: "ab".repeat(32),
        };
        write_agent_info(&f, &info).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info);
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            format!(r#"{{"port":4242,"token":"{}"}}"#, info.token)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&f), 0o600);
            assert_eq!(mode(f.parent().unwrap()), 0o700);
        }
        // Writing again replaces the file.
        let info2 = AgentInfo {
            port: 1,
            token: "cd".repeat(32),
        };
        write_agent_info(&f, &info2).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info2);
        assert!(
            read_agent_info(&dir.join("none"))
                .unwrap_err()
                .contains("cannot read")
        );
        std::fs::write(dir.join("bad"), "{").unwrap();
        assert!(read_agent_info(&dir.join("bad")).is_err());
        let _ = agent_info_path();
        let _ = std::fs::remove_dir_all(dir);
    }
}

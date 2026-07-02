//! Optional config file. Looked up at `./.delphin.toml`, then
//! `<config_dir>/delphin/config.toml`. CLI flags override anything set here.
//!
//! Example `.delphin.toml`:
//! ```toml
//! idle_ms = 1000
//! interrupt = "ctrl-c"
//! arbiter = "question"
//! live = false
//! ready_markers = ["you> ", "❯ "]
//! ```

use std::path::PathBuf;

use serde::Deserialize;

use crate::arbiter::DEFAULT_INTERRUPT_KEYWORDS;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub idle_ms: u64,
    /// Minimum busy time before a silence gap counts as idle (0 = off).
    pub min_busy_ms: u64,
    pub tick_ms: u64,
    pub interrupt: String,
    pub submit_newline: bool,
    pub arbiter: String,
    pub live: bool,
    /// Substrings that, when the agent's output ends with one, mean it's waiting
    /// for input (idle) — flips out of "busy" immediately instead of waiting for
    /// the silence timer.
    pub ready_markers: Vec<String>,
    pub interrupt_keywords: Vec<String>,
    pub log: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idle_ms: 800,
            min_busy_ms: 0,
            tick_ms: 150,
            interrupt: "esc".into(),
            submit_newline: false,
            arbiter: "heuristic".into(),
            live: false,
            ready_markers: Vec::new(),
            interrupt_keywords: DEFAULT_INTERRUPT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            log: true,
        }
    }
}

impl Config {
    /// Load from the first config file found, or defaults. Also returns the path
    /// it loaded (for a startup notice).
    pub fn load() -> (Config, Option<PathBuf>) {
        for path in candidate_paths() {
            if path.is_file() {
                match std::fs::read_to_string(&path) {
                    Ok(text) => match toml::from_str::<Config>(&text) {
                        Ok(cfg) => return (cfg, Some(path)),
                        Err(e) => {
                            eprintln!("delphin: ignoring invalid config {}: {e}", path.display())
                        }
                    },
                    Err(e) => eprintln!("delphin: can't read config {}: {e}", path.display()),
                }
            }
        }
        (Config::default(), None)
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(".delphin.toml")];
    if let Some(dir) = dirs::config_dir() {
        v.push(dir.join("delphin").join("config.toml"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_toml_falls_back_to_defaults() {
        let cfg: Config = toml::from_str("idle_ms = 1200\narbiter = \"question\"\n").unwrap();
        assert_eq!(cfg.idle_ms, 1200);
        assert_eq!(cfg.arbiter, "question");
        // unspecified fields keep defaults
        assert_eq!(cfg.tick_ms, 150);
        assert_eq!(cfg.interrupt, "esc");
        assert!(!cfg.live);
        assert!(cfg.log);
        assert!(!cfg.interrupt_keywords.is_empty());
    }

    #[test]
    fn ready_markers_parse() {
        let cfg: Config = toml::from_str("ready_markers = [\"you> \", \"❯ \"]").unwrap();
        assert_eq!(
            cfg.ready_markers,
            vec!["you> ".to_string(), "❯ ".to_string()]
        );
    }
}

use serde::{Deserialize, Serialize};

/// Configuration for the Command Not Found module.
///
/// Stored in `~/.config/my-power-toys/command-not-found.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandNotFoundConfig {
    /// Install the shell hooks when the module starts.
    pub install_shell_hooks: bool,
    /// Add the hook to `~/.bashrc` (only if the file exists).
    pub bash: bool,
    /// Add the hook to `~/.zshrc` / `$ZDOTDIR/.zshrc` (only if the file exists).
    pub zsh: bool,
    /// Create `~/.config/fish/functions/fish_command_not_found.fish`
    /// (only if the fish config directory exists).
    pub fish: bool,
    /// Suggest similarly named executables on `$PATH` ("did you mean").
    pub fuzzy_suggestions: bool,
    /// Timeout for each package-manager lookup, in milliseconds.
    pub lookup_timeout_ms: u64,
}

impl Default for CommandNotFoundConfig {
    fn default() -> Self {
        Self {
            install_shell_hooks: true,
            bash: true,
            zsh: true,
            fish: true,
            fuzzy_suggestions: true,
            lookup_timeout_ms: 5000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_partial() {
        let cfg = CommandNotFoundConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: CommandNotFoundConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);

        let partial: CommandNotFoundConfig = toml::from_str("zsh = false").unwrap();
        assert!(!partial.zsh);
        assert!(partial.bash);
        assert_eq!(partial.lookup_timeout_ms, 5000);
    }
}

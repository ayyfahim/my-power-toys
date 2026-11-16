//! State and operations behind the editor window, independent of the GUI.

use crate::EnvironmentVariablesConfig;
use crate::envd::{self, EnvFile};
use crate::profiles::ProfileStore;
use crate::session::{self, SessionReport};
use crate::store::{self, EffectiveVar, Paths, Scope, Source};
use anyhow::{Result, anyhow};
use std::collections::{BTreeMap, HashSet};

/// Which editable set a variable belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    User,
    System,
    Profile(String),
}

pub struct Manager {
    pub paths: Paths,
    pub config: EnvironmentVariablesConfig,
    pub user: EnvFile,
    pub system_managed: EnvFile,
    pub profiles: ProfileStore,
    pub sources: Vec<Source>,
    /// Environment of this process at startup (used for `$VAR` expansion and
    /// the "Session" source).
    pub base_env: BTreeMap<String, String>,
    managed_before: HashSet<String>,
}

impl Manager {
    pub fn load(config: EnvironmentVariablesConfig) -> Result<Self> {
        let paths = Paths::detect()?;
        let mut m = Self {
            user: store::read_env_file(&paths.user_file)?,
            system_managed: store::read_env_file(&paths.system_file).unwrap_or_default(),
            profiles: store::load_profiles(&paths.profiles_toml)?,
            sources: Vec::new(),
            base_env: store::process_env(),
            managed_before: HashSet::new(),
            paths,
            config,
        };
        m.refresh_sources();
        m.managed_before = store::managed_names(&m.sources);
        Ok(m)
    }

    /// Re-read every file from disk.
    pub fn reload(&mut self) -> Result<()> {
        self.user = store::read_env_file(&self.paths.user_file)?;
        self.system_managed = store::read_env_file(&self.paths.system_file).unwrap_or_default();
        self.profiles = store::load_profiles(&self.paths.profiles_toml)?;
        self.refresh_sources();
        Ok(())
    }

    fn refresh_sources(&mut self) {
        self.sources = store::collect_sources(&self.paths);
    }

    pub fn effective(&self, include_session: bool) -> Vec<EffectiveVar> {
        store::merge(&self.sources, &self.base_env, include_session)
    }

    pub fn can_edit_system(&self) -> bool {
        self.config.launch_administrator
    }

    /// Set (or rename) a variable in `target` and save.
    pub fn set_var(
        &mut self,
        target: &Target,
        original: Option<&str>,
        name: &str,
        value: &str,
    ) -> Result<SessionReport> {
        envd::validate(name, value).map_err(|e| anyhow!(e))?;
        if original != Some(name) && self.vars_of(target).iter().any(|(k, _)| k == name) {
            return Err(anyhow!("A variable named '{name}' already exists here."));
        }
        match target {
            Target::User => {
                apply_edit(&mut self.user, original, name, value);
                self.save_user()
            }
            Target::System => {
                if !self.can_edit_system() {
                    return Err(anyhow!(
                        "Editing system variables is disabled (launch_administrator = false)"
                    ));
                }
                let mut f = self.system_managed.clone();
                apply_edit(&mut f, original, name, value);
                self.save_system(f)
            }
            Target::Profile(p) => {
                let profile = self
                    .profiles
                    .get_mut(p)
                    .ok_or_else(|| anyhow!("profile not found"))?;
                if let Some(orig) = original
                    && orig != name
                {
                    profile.remove(orig);
                }
                profile.set(name, value);
                self.save_profiles_and_reapply(p)
            }
        }
    }

    pub fn delete_var(&mut self, target: &Target, name: &str) -> Result<SessionReport> {
        match target {
            Target::User => {
                self.user.remove(name);
                self.save_user()
            }
            Target::System => {
                let mut f = self.system_managed.clone();
                f.remove(name);
                self.save_system(f)
            }
            Target::Profile(p) => {
                if let Some(profile) = self.profiles.get_mut(p) {
                    profile.remove(name);
                }
                self.save_profiles_and_reapply(p)
            }
        }
    }

    pub fn vars_of(&self, target: &Target) -> Vec<(String, String)> {
        match target {
            Target::User => self.user.vars(),
            Target::System => self.system_managed.vars(),
            Target::Profile(p) => self
                .profiles
                .get(p)
                .map(|p| {
                    p.variables
                        .iter()
                        .map(|v| (v.name.clone(), v.value.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn save_user(&mut self) -> Result<SessionReport> {
        self.write_user()?;
        Ok(self.sync_session())
    }

    fn write_user(&self) -> Result<()> {
        if self.user.lines.is_empty() && !self.paths.user_file.exists() {
            return Ok(());
        }
        store::write_atomic(&self.paths.user_file, &self.user.serialize())
    }

    fn save_system(&mut self, file: EnvFile) -> Result<SessionReport> {
        store::write_system_file(&self.paths.system_file, &file.serialize())?;
        self.system_managed = file;
        Ok(self.sync_session())
    }

    fn write_profiles(&self) -> Result<()> {
        store::save_profiles(&self.paths.profiles_toml, &self.profiles)
    }

    /// Save profiles; if `name` is the applied profile, rewrite its file.
    fn save_profiles_and_reapply(&mut self, name: &str) -> Result<SessionReport> {
        if self.profiles.applied.as_deref() == Some(name) {
            return self.apply_profile(name);
        }
        self.write_profiles()?;
        Ok(SessionReport::default())
    }

    pub fn create_profile(&mut self, name: &str) -> Result<()> {
        self.profiles.create(name).map_err(|e| anyhow!(e))?;
        self.write_profiles()
    }

    pub fn rename_profile(&mut self, old: &str, new: &str) -> Result<()> {
        self.profiles.rename(old, new).map_err(|e| anyhow!(e))?;
        self.write_profiles()?;
        if let Some(p) = self.profiles.applied_profile() {
            // Refresh the header comment with the new name.
            store::write_atomic(&self.paths.profile_file, &p.to_env_file().serialize())?;
        }
        Ok(())
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<SessionReport> {
        if self.profiles.delete(name, &mut self.user) {
            self.write_user()?;
            store::remove_file_if_exists(&self.paths.profile_file)?;
            self.write_profiles()?;
            return Ok(self.sync_session());
        }
        self.write_profiles()?;
        Ok(SessionReport::default())
    }

    pub fn apply_profile(&mut self, name: &str) -> Result<SessionReport> {
        let profile_file = self
            .profiles
            .apply(name, &mut self.user)
            .map_err(|e| anyhow!(e))?;
        self.write_user()?;
        store::write_atomic(&self.paths.profile_file, &profile_file.serialize())?;
        self.write_profiles()?;
        Ok(self.sync_session())
    }

    pub fn unapply_profile(&mut self) -> Result<SessionReport> {
        self.profiles.unapply(&mut self.user);
        self.write_user()?;
        store::remove_file_if_exists(&self.paths.profile_file)?;
        self.write_profiles()?;
        Ok(self.sync_session())
    }

    /// Reload sources and push changed/removed variables into the session.
    fn sync_session(&mut self) -> SessionReport {
        self.refresh_sources();
        let now = store::managed_names(&self.sources);
        let removed: Vec<String> = self.managed_before.difference(&now).cloned().collect();
        let effective = self.effective(false);
        let (set, unset) = session_changes(&effective, &now, &removed);
        self.managed_before = now;
        if !self.config.update_session {
            return SessionReport {
                messages: vec!["Saved. Log out and back in to apply the changes.".into()],
                warnings: Vec::new(),
            };
        }
        session::update_session(&set, &unset)
    }
}

/// Compute (set, unset) lists: every managed variable is set to its effective
/// expanded value; a removed variable is reset to a remaining file-defined
/// value if there is one, otherwise unset.
pub fn session_changes(
    effective: &[EffectiveVar],
    managed: &HashSet<String>,
    removed: &[String],
) -> (Vec<(String, String)>, Vec<String>) {
    let mut set = Vec::new();
    let mut unset = Vec::new();
    for row in effective {
        if row.scope != Scope::Session
            && (managed.contains(&row.name) || removed.contains(&row.name))
        {
            set.push((row.name.clone(), row.expanded.clone()));
        }
    }
    for name in removed {
        if !effective
            .iter()
            .any(|r| &r.name == name && r.scope != Scope::Session)
        {
            unset.push(name.clone());
        }
    }
    unset.sort();
    (set, unset)
}

fn apply_edit(file: &mut EnvFile, original: Option<&str>, name: &str, value: &str) {
    match original {
        Some(orig) if orig != name => file.rename(orig, name, value),
        _ => file.set(name, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn row(name: &str, val: &str, scope: Scope) -> EffectiveVar {
        EffectiveVar {
            name: name.into(),
            value: val.into(),
            expanded: val.into(),
            scope,
            path: Some(PathBuf::from("/x")),
            overrides: vec![],
        }
    }

    #[test]
    fn session_changes_sets_and_unsets() {
        let eff = vec![
            row("A", "1", Scope::User),
            row("B", "sys", Scope::System),
            row("OTHER", "o", Scope::System),
        ];
        let managed: HashSet<String> = ["A".to_string()].into();
        let removed = vec!["B".to_string(), "GONE".to_string()];
        let (set, unset) = session_changes(&eff, &managed, &removed);
        assert_eq!(
            set,
            vec![("A".into(), "1".into()), ("B".into(), "sys".into())]
        );
        assert_eq!(unset, vec!["GONE".to_string()]);
    }

    #[test]
    fn apply_edit_renames() {
        let mut f = EnvFile::parse("A=1\n");
        apply_edit(&mut f, Some("A"), "B", "2");
        assert_eq!(f.serialize(), "B=2\n");
        apply_edit(&mut f, None, "C", "3");
        assert_eq!(f.get("C"), Some("3"));
    }
}

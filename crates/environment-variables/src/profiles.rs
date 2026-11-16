//! Named variable profiles (upstream "Profiles").
//!
//! Profiles live in `~/.config/my-power-toys/environment-variables-profiles.toml`.
//! At most one profile is applied at a time. Applying writes the profile's
//! variables to `60-my-power-toys-profile.conf`; user variables with the same
//! name are moved out of the user file into `backups` and restored on unapply
//! (upstream renames them to `NAME_PowerToys_<profile>` for the same purpose).

use crate::envd::{self, EnvFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileVar {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub variables: Vec<ProfileVar>,
}

impl Profile {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            variables: Vec::new(),
        }
    }

    /// Insert or update a variable (by name).
    pub fn set(&mut self, name: &str, value: &str) {
        match self.variables.iter_mut().find(|v| v.name == name) {
            Some(v) => v.value = value.to_string(),
            None => self.variables.push(ProfileVar {
                name: name.to_string(),
                value: value.to_string(),
            }),
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.variables.retain(|v| v.name != name);
    }

    /// Contents of the profile environment.d file.
    pub fn to_env_file(&self) -> EnvFile {
        let mut f = EnvFile::parse(&format!(
            "# Managed by MyPowerToys Environment Variables. Do not edit.\n# Applied profile: {}\n",
            self.name.replace(['\n', '\r'], " ")
        ));
        for v in &self.variables {
            f.set(&v.name, &v.value);
        }
        f
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStore {
    /// Name of the applied profile, if any.
    #[serde(default)]
    pub applied: Option<String>,
    /// User variables displaced by the applied profile (name -> value).
    #[serde(default)]
    pub backups: BTreeMap<String, String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

/// Validate a profile name. `exclude` is the current name when renaming.
pub fn validate_profile_name(
    store: &ProfileStore,
    name: &str,
    exclude: Option<&str>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Profile name is empty.".into());
    }
    if name != name.trim() {
        return Err("Profile name cannot start or end with whitespace.".into());
    }
    if name.chars().any(char::is_control) {
        return Err("Profile name cannot contain control characters.".into());
    }
    if name.chars().count() > 64 {
        return Err("Profile name is longer than 64 characters.".into());
    }
    if store
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != exclude)
    {
        return Err(format!("A profile named '{name}' already exists."));
    }
    Ok(())
}

impl ProfileStore {
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(content)?)
    }

    pub fn to_toml(&self) -> anyhow::Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    pub fn applied_profile(&self) -> Option<&Profile> {
        self.applied.as_deref().and_then(|n| self.get(n))
    }

    pub fn create(&mut self, name: &str) -> Result<(), String> {
        validate_profile_name(self, name, None)?;
        self.profiles.push(Profile::new(name));
        Ok(())
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), String> {
        validate_profile_name(self, new, Some(old))?;
        let p = self.get_mut(old).ok_or("Profile not found.")?;
        p.name = new.to_string();
        if self.applied.as_deref() == Some(old) {
            self.applied = Some(new.to_string());
        }
        Ok(())
    }

    /// Check a profile can be applied (all variables valid).
    pub fn check_applicable(&self, name: &str) -> Result<(), String> {
        let p = self.get(name).ok_or("Profile not found.")?;
        for v in &p.variables {
            envd::validate(&v.name, &v.value)?;
        }
        Ok(())
    }

    /// Apply profile `name`. Any currently applied profile is unapplied first.
    /// Overridden user variables are moved from `user` into `backups`.
    /// Returns the content for the profile file.
    pub fn apply(&mut self, name: &str, user: &mut EnvFile) -> Result<EnvFile, String> {
        self.check_applicable(name)?;
        self.unapply(user);
        let profile = self.get(name).cloned().ok_or("Profile not found.")?;
        for v in &profile.variables {
            if let Some(existing) = user.get(&v.name) {
                self.backups.insert(v.name.clone(), existing.to_string());
                user.remove(&v.name);
            }
        }
        self.applied = Some(profile.name.clone());
        Ok(profile.to_env_file())
    }

    /// Unapply the current profile, restoring backed-up user variables unless
    /// the user has since defined the variable again.
    pub fn unapply(&mut self, user: &mut EnvFile) {
        for (k, v) in std::mem::take(&mut self.backups) {
            if !user.contains(&k) {
                user.set(&k, &v);
            }
        }
        self.applied = None;
    }

    /// Delete a profile, unapplying it first if needed. Returns true if it was
    /// applied (so the caller must remove the profile file).
    pub fn delete(&mut self, name: &str, user: &mut EnvFile) -> bool {
        let was_applied = self.applied.as_deref() == Some(name);
        if was_applied {
            self.unapply(user);
        }
        self.profiles.retain(|p| p.name != name);
        was_applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(name: &str, vars: &[(&str, &str)]) -> ProfileStore {
        let mut s = ProfileStore::default();
        s.create(name).unwrap();
        for (k, v) in vars {
            s.get_mut(name).unwrap().set(k, v);
        }
        s
    }

    #[test]
    fn apply_backs_up_and_unapply_restores() {
        let mut s = store_with("dev", &[("EDITOR", "nvim"), ("NEW", "1")]);
        let mut user = EnvFile::parse("# mine\nEDITOR=nano\nOTHER=x\n");
        let pf = s.apply("dev", &mut user).unwrap();
        assert_eq!(s.applied.as_deref(), Some("dev"));
        assert_eq!(pf.get("EDITOR"), Some("nvim"));
        assert_eq!(pf.get("NEW"), Some("1"));
        assert!(!user.contains("EDITOR"));
        assert_eq!(s.backups.get("EDITOR").map(String::as_str), Some("nano"));

        s.unapply(&mut user);
        assert_eq!(s.applied, None);
        assert!(s.backups.is_empty());
        assert_eq!(user.get("EDITOR"), Some("nano"));
        assert_eq!(user.get("OTHER"), Some("x"));
    }

    #[test]
    fn unapply_does_not_clobber_newer_user_value() {
        let mut s = store_with("dev", &[("EDITOR", "nvim")]);
        let mut user = EnvFile::parse("EDITOR=nano\n");
        s.apply("dev", &mut user).unwrap();
        user.set("EDITOR", "vim");
        s.unapply(&mut user);
        assert_eq!(user.get("EDITOR"), Some("vim"));
    }

    #[test]
    fn only_one_profile_applied() {
        let mut s = store_with("a", &[("X", "a")]);
        s.create("b").unwrap();
        s.get_mut("b").unwrap().set("Y", "b");
        let mut user = EnvFile::parse("X=orig\nY=orig\n");
        s.apply("a", &mut user).unwrap();
        s.apply("b", &mut user).unwrap();
        assert_eq!(s.applied.as_deref(), Some("b"));
        // X restored from profile a, Y now backed up by profile b.
        assert_eq!(user.get("X"), Some("orig"));
        assert!(!user.contains("Y"));
        assert_eq!(s.backups.len(), 1);
    }

    #[test]
    fn rename_and_delete_applied() {
        let mut s = store_with("a", &[("X", "1")]);
        let mut user = EnvFile::parse("X=0\n");
        s.apply("a", &mut user).unwrap();
        s.rename("a", "b").unwrap();
        assert_eq!(s.applied.as_deref(), Some("b"));
        assert!(s.delete("b", &mut user));
        assert!(s.profiles.is_empty());
        assert_eq!(user.get("X"), Some("0"));
    }

    #[test]
    fn name_validation() {
        let s = store_with("a", &[]);
        assert!(validate_profile_name(&s, "a", None).is_err());
        assert!(validate_profile_name(&s, "a", Some("a")).is_ok());
        assert!(validate_profile_name(&s, " x", None).is_err());
        assert!(validate_profile_name(&s, "", None).is_err());
        assert!(validate_profile_name(&s, "My profile", None).is_ok());
    }

    #[test]
    fn invalid_variable_blocks_apply() {
        let mut s = store_with("a", &[("1BAD", "x")]);
        let mut user = EnvFile::default();
        assert!(s.apply("a", &mut user).is_err());
        assert_eq!(s.applied, None);
    }

    #[test]
    fn toml_roundtrip() {
        let mut s = store_with("dev", &[("A", "$HOME/x"), ("B", "q\"uote")]);
        s.applied = Some("dev".into());
        s.backups.insert("A".into(), "old".into());
        let text = s.to_toml().unwrap();
        assert_eq!(ProfileStore::parse(&text).unwrap(), s);
        assert_eq!(ProfileStore::parse("").unwrap(), ProfileStore::default());
    }
}

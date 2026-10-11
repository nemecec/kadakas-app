//! The list of profiles that a person can choose from: the built-in ones and the user's own.
//! The set holds no files. A UI program loads the user files into it and saves what it changes.

use kadakas_l10n::tr;

use super::{MAX_NAME, ParseError, Problem, Profile, layout};
use crate::keymap::{Action, Chord, Keymap};

/// Why a profile cannot be renamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameError {
    /// No profile has this id.
    Unknown,
    /// Built-in profiles are read-only.
    ReadOnly,
    /// The name is empty or too long.
    BadName,
    /// Another profile has this name.
    NameTaken,
}

/// Lower case letters and digits of `text`, with `-` between words (`_` stays), at most 24
/// characters. Empty text gives `profile`.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.chars() {
        if c == '_' {
            out.push('_');
            dash = false;
        } else if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    out.truncate(24);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() || out.chars().all(|c| c == '_') {
        "profile".into()
    } else {
        out
    }
}

/// Letters and digits of `text` in lower case: the form in which names are compared.
fn loose(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The built-in profiles and the user profiles. The built-in ones come first.
#[derive(Clone, Debug)]
pub struct ProfileSet {
    profiles: Vec<Profile>,
}

impl ProfileSet {
    /// The built-in profiles and `user`. A user profile whose id is taken gets another id.
    pub fn new(user: Vec<Profile>) -> ProfileSet {
        let mut set = ProfileSet {
            profiles: Profile::builtins(),
        };
        for p in user {
            set.add_user(p);
        }
        set
    }

    /// Adds a user profile and returns its id. A valid id that is free stays as it is. Another
    /// id is made from it. A name that is taken gets a number.
    pub fn add_user(&mut self, mut p: Profile) -> String {
        p.builtin = false;
        p.id = self.unique_id(&p.id);
        p.name = self.unique_name(&p.name);
        let id = p.id.clone();
        self.profiles.push(p);
        id
    }

    /// Gives a built-in profile another keymap. The app uses it to apply the old `keymap.json`
    /// to the Vectorworks-like profile, so the keys that work are the keys that the profile
    /// shows and copies.
    pub fn set_builtin_keymap(&mut self, id: &str, keymap: Keymap) -> bool {
        match self.profiles.iter_mut().find(|p| p.id == id && p.builtin) {
            Some(p) => {
                p.keymap = keymap;
                true
            }
            None => false,
        }
    }

    pub fn all(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// The profile that `key` names: its id, or its name, ignoring case and punctuation (so
    /// `AutoCAD-like` and `autocad_like` both find the same profile). The launcher passes this
    /// text as `--profile NAME`.
    pub fn find(&self, key: &str) -> Option<&Profile> {
        let k = loose(key);
        self.get(key)
            .or_else(|| self.profiles.iter().find(|p| loose(&p.id) == k))
            .or_else(|| self.profiles.iter().find(|p| loose(&p.name) == k))
    }

    /// An id that no profile has, starting from `base`.
    pub fn unique_id(&self, base: &str) -> String {
        let base = if layout::valid_id(base) {
            base.to_string()
        } else {
            slug(base)
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.get(&id).is_some() {
            let suffix = format!("-{n}");
            let mut stem = base.clone();
            stem.truncate(24 - suffix.len());
            id = format!("{stem}{suffix}");
            n += 1;
        }
        id
    }

    /// A name that no profile has, starting from `base`: `Name`, then `Name 2`, `Name 3`.
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| {
            self.profiles
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(n) || p.display_name() == n)
        };
        let base: String = base.trim().chars().take(MAX_NAME - 4).collect();
        let base = if base.is_empty() {
            "Profile".into()
        } else {
            base
        };
        let mut name = base.clone();
        let mut n = 2;
        while taken(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        name
    }

    /// Adds a copy of a profile as a user profile and returns its id.
    pub fn duplicate(&mut self, id: &str) -> Option<String> {
        let src = self.get(id)?.clone();
        let name = self.unique_name(&tr!("uic-profile-copy-of", name = src.display_name()));
        let copy = Profile {
            id: self.unique_id(&name),
            name,
            description: src.display_description(),
            based_on: Some(src.id.clone()),
            builtin: false,
            ..src
        };
        let new_id = copy.id.clone();
        self.profiles.push(copy);
        Some(new_id)
    }

    /// The id of a profile that can be edited. A user profile is its own answer. A built-in
    /// profile gets a user copy first, and the answer is the copy.
    pub fn editable(&mut self, id: &str) -> Option<String> {
        let p = self.get(id)?;
        if p.builtin {
            self.duplicate(id)
        } else {
            Some(id.to_string())
        }
    }

    /// The user profile with this id, for editing. `None` for a built-in profile.
    pub fn user_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id && !p.builtin)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), RenameError> {
        let name = name.trim();
        let p = self.get(id).ok_or(RenameError::Unknown)?;
        if p.builtin {
            return Err(RenameError::ReadOnly);
        }
        if name.is_empty() || name.chars().count() > MAX_NAME {
            return Err(RenameError::BadName);
        }
        let taken = self
            .profiles
            .iter()
            .any(|o| o.id != id && (o.name.eq_ignore_ascii_case(name) || o.display_name() == name));
        if taken {
            return Err(RenameError::NameTaken);
        }
        if let Some(p) = self.user_mut(id) {
            p.name = name.to_string();
        }
        Ok(())
    }

    /// The keymap that "reset" gives: the keymap of the profile that `id` was copied from. A
    /// chain of copies is followed up to a built-in profile; a profile without a source (or with
    /// a source that is gone) resets to the Vectorworks-like defaults.
    pub fn base_keymap(&self, id: &str) -> Keymap {
        let mut at = self.get(id).and_then(|p| p.based_on.clone());
        for _ in 0..16 {
            let Some(base) = at.as_deref().and_then(|b| self.get(b)) else {
                break;
            };
            if base.builtin {
                return base.keymap.clone();
            }
            at = base.based_on.clone();
        }
        Keymap::defaults()
    }

    /// Resets the keys of one action in a user profile to the keys of its base profile. Other
    /// actions that hold those chords lose them. Returns the moved chords; `None` for a
    /// built-in or unknown profile.
    pub fn reset_chords(&mut self, id: &str, action: Action) -> Option<Vec<(Chord, Action)>> {
        let base = self.base_keymap(id);
        self.user_mut(id).map(|p| p.reset_chords(action, &base))
    }

    /// Removes a user profile. False for a built-in or unknown profile.
    pub fn remove(&mut self, id: &str) -> bool {
        match self.profiles.iter().position(|p| p.id == id && !p.builtin) {
            Some(i) => {
                self.profiles.remove(i);
                true
            }
            None => false,
        }
    }

    /// Reads a profile from text and adds it as a user profile. It gets a free id and a free
    /// name. Returns the id and the problems that the loader worked around.
    pub fn import(&mut self, text: &str) -> Result<(String, Vec<Problem>), ParseError> {
        let parsed = Profile::parse(text, "imported")?;
        let id = self.add_user(parsed.profile);
        Ok((id, parsed.problems))
    }
}

//! The apps bzz ships to start from (plan D9): `bzz app new <name>
//! --template <template>` writes one as a folder of the owner's.
//!
//! A template is a script and its brief. Its manifest is written for the
//! name the owner gives, with the record types the script declares; a test
//! loads every template apart and holds [`Template::types`] to what the
//! script says.
//!
//! **To add a template** (Poll and Approvals are to come from the fixtures
//! of M1): put its brief at `src/apps/templates/<name>/brief.md`, point
//! `script` at its script, and add it to [`TEMPLATES`]. The tests of this
//! file then cover it without a change.

use std::path::Path;

use crate::apps::runtime::API_VERSION;

use super::folder::write_new;
use super::manifest::{Manifest, check_name, check_title, title_from_name};

pub struct Template {
    /// What `--template` names.
    pub name: &'static str,
    pub title: &'static str,
    /// One line for the list of templates.
    pub about: &'static str,
    /// The record types the script declares.
    pub types: &'static [&'static str],
    pub script: &'static str,
    pub brief: &'static str,
}

/// The Board is the fixture the runtime's own tests and the bench run, so
/// that what ships is what is tested.
pub const TEMPLATES: &[Template] = &[Template {
    name: "board",
    title: "Board",
    about: "cards in three columns: To do, Doing, Done",
    types: &["card"],
    script: include_str!("../../../tests/fixtures/apps/board/app.lua"),
    brief: include_str!("../templates/board/brief.md"),
}];

/// The template `bzz app new` uses when none is named.
pub const DEFAULT: &str = "board";

pub fn find(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.name == name)
}

/// The templates' names, for a message.
pub fn names() -> String {
    TEMPLATES
        .iter()
        .map(|template| template.name)
        .collect::<Vec<_>>()
        .join(", ")
}

impl Template {
    /// The manifest of an app called `name` that starts from this template.
    /// Without a title: the template's own for an app of the template's
    /// name, and the name with a capital otherwise.
    pub fn manifest(&self, name: &str, title: Option<&str>) -> Result<Manifest, String> {
        check_name(name)?;
        let title = match title {
            Some(title) => check_title(title)?,
            None if name == self.name => self.title.to_owned(),
            None => title_from_name(name),
        };
        Ok(Manifest {
            name: name.to_owned(),
            title,
            api: API_VERSION,
            types: self.types.iter().map(|name| (*name).to_owned()).collect(),
        })
    }

    /// Writes the template as a new folder for an app called `name`.
    pub fn write(&self, name: &str, title: Option<&str>, dir: &Path) -> Result<Manifest, String> {
        let manifest = self.manifest(name, title)?;
        write_new(dir, self.script.as_bytes(), &manifest, self.brief)?;
        Ok(manifest)
    }
}

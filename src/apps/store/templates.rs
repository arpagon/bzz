//! The apps bzz ships to start from (plan D9): `bzz app new <name>
//! --template <template>` writes one as a folder of the owner's, and so
//! does `:app new` in the TUI.
//!
//! A template is a script and its brief. Its manifest is written for the
//! name the owner gives, with the record types the script declares; a test
//! loads every template apart and holds [`Template::types`] to what the
//! script says.
//!
//! **One source.** A template's script and brief are the fixture of the
//! same name, `tests/fixtures/apps/<name>/{app.lua,brief.md}`: what ships
//! is what the runtime's tests (`apps::runtime::tests`) and the apps bench
//! (`scripts/test-apps-bench.sh`) run, so the two cannot drift.
//!
//! **To add a template**: write the fixture and its brief, and add it to
//! [`TEMPLATES`] with the types the script declares. The test
//! `every_template_loads_and_declares_what_its_entry_says` then covers it;
//! `docs/apps.md` gets a row.

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

/// The three of plan D9. Each stores people by id and shows them with
/// `host.name`, keeps to the order field, handles a conflict as its brief
/// says, and fits the pane's 40 by 16.
pub const TEMPLATES: &[Template] = &[
    Template {
        name: "board",
        title: "Board",
        about: "cards in three columns: To do, Doing, Done",
        types: &["card"],
        script: include_str!("../../../tests/fixtures/apps/board/app.lua"),
        brief: include_str!("../../../tests/fixtures/apps/board/brief.md"),
    },
    Template {
        name: "poll",
        title: "Poll",
        about: "one question, its options, one vote for each person",
        types: &["poll", "vote"],
        script: include_str!("../../../tests/fixtures/apps/poll/app.lua"),
        brief: include_str!("../../../tests/fixtures/apps/poll/brief.md"),
    },
    Template {
        name: "approvals",
        title: "Approvals",
        about: "requests that someone else approves, or rejects with a reason",
        types: &["request"],
        script: include_str!("../../../tests/fixtures/apps/approvals/app.lua"),
        brief: include_str!("../../../tests/fixtures/apps/approvals/brief.md"),
    },
];

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

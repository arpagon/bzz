//! An app as a folder of the owner's: `app.lua`, `app.toml` and `brief.md`.
//! It is what `bzz app new` writes, what `bzz app add` proposes and what
//! `bzz app dev` runs.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::store::apps::Proposer;

use super::Submission;
use super::manifest::{
    BRIEF_BYTES, BRIEF_FILE, MANIFEST_BYTES, MANIFEST_FILE, Manifest, SCRIPT_BYTES, SCRIPT_FILE,
};

/// Reads a file that may be at most `limit` bytes, and never more of it
/// than that and one byte. The error names the file.
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut read = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut read)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if read.len() > limit {
        let size = fs::metadata(path).map_or(read.len() as u64, |metadata| metadata.len());
        return Err(format!("{name} is {size} bytes; it may be {limit}"));
    }
    Ok(read)
}

/// The three files of an app, read from a folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Folder {
    pub dir: PathBuf,
    pub script: Vec<u8>,
    pub manifest: Manifest,
    pub brief: Vec<u8>,
}

impl Folder {
    /// Reads the three files. A file that is missing, too large, or a
    /// manifest that is wrong, is an error that names the file.
    pub fn read(dir: &Path) -> Result<Self, String> {
        if !dir.is_dir() {
            return Err(format!("{} is not a folder", dir.display()));
        }
        let missing = [SCRIPT_FILE, MANIFEST_FILE, BRIEF_FILE]
            .into_iter()
            .filter(|file| !dir.join(file).is_file())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "{} has no {}: an app's folder holds {SCRIPT_FILE} (the script), {MANIFEST_FILE} (its name, title, api and record types) and {BRIEF_FILE} (what it does and what its records hold). `bzz app new <name>` writes one to start from",
                dir.display(),
                missing.join(" and no ")
            ));
        }
        let script = read_bounded(&dir.join(SCRIPT_FILE), SCRIPT_BYTES)?;
        let manifest = read_bounded(&dir.join(MANIFEST_FILE), MANIFEST_BYTES)?;
        let manifest = std::str::from_utf8(&manifest)
            .map_err(|_| format!("{MANIFEST_FILE} is not text: it is not valid UTF-8"))
            .and_then(|text| {
                Manifest::parse(text).map_err(|error| format!("{MANIFEST_FILE}: {error}"))
            })?;
        let brief = read_bounded(&dir.join(BRIEF_FILE), BRIEF_BYTES)?;
        Ok(Self {
            dir: dir.to_owned(),
            script,
            manifest,
            brief,
        })
    }

    /// The folder as a version to propose.
    pub fn submission(self, from: Proposer) -> Submission {
        Submission {
            name: self.manifest.name.clone(),
            title: Some(self.manifest.title.clone()),
            script: self.script,
            brief: self.brief,
            declared: Some(self.manifest),
            from,
            script_name: SCRIPT_FILE.to_owned(),
        }
    }
}

/// Writes a new folder with the three files. The folder must not exist:
/// nothing of the owner's is ever written over.
pub fn write_new(
    dir: &Path,
    script: &[u8],
    manifest: &Manifest,
    brief: &str,
) -> Result<(), String> {
    if dir.exists() {
        return Err(format!(
            "{} exists already; nothing was written. Choose another folder with --to",
            dir.display()
        ));
    }
    let failed = |error: std::io::Error| format!("{}: {error}", dir.display());
    fs::create_dir_all(dir).map_err(failed)?;
    let header = format!(
        "# The manifest of a bzz app. `api` and `types` repeat what {SCRIPT_FILE} says\n# (`app.api`, `app.types`): the script is the authority, and bzz refuses a\n# folder in which the two differ. `name` is `[a-z][a-z0-9_-]{{0,31}}`; an app's\n# records belong to its name.\n"
    );
    for (file, content) in [
        (SCRIPT_FILE, script.to_vec()),
        (
            MANIFEST_FILE,
            format!("{header}{}", manifest.to_text()).into_bytes(),
        ),
        (BRIEF_FILE, brief.as_bytes().to_vec()),
    ] {
        fs::write(dir.join(file), content).map_err(failed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            name: "notes".into(),
            title: "Notes".into(),
            api: 1,
            types: vec!["note".into()],
        }
    }

    #[test]
    fn a_folder_that_was_written_reads_back_and_is_never_written_over() {
        let temporary = tempfile::TempDir::new().unwrap();
        let dir = temporary.path().join("notes");
        write_new(&dir, b"return {}", &manifest(), "Notes.\n").unwrap();
        let folder = Folder::read(&dir).unwrap();
        assert_eq!(folder.script, b"return {}");
        assert_eq!(folder.manifest, manifest());
        assert_eq!(folder.brief, b"Notes.\n");
        let again = write_new(&dir, b"other", &manifest(), "Other.\n").unwrap_err();
        assert!(again.contains("exists already"), "{again}");
        assert_eq!(fs::read(dir.join(SCRIPT_FILE)).unwrap(), b"return {}");
    }

    #[test]
    fn what_is_missing_or_too_large_is_named() {
        let temporary = tempfile::TempDir::new().unwrap();
        let dir = temporary.path().join("notes");
        assert!(Folder::read(&dir).unwrap_err().contains("is not a folder"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(SCRIPT_FILE), "return {}").unwrap();
        let error = Folder::read(&dir).unwrap_err();
        assert!(
            error.contains("has no app.toml and no brief.md") && error.contains("bzz app new"),
            "{error}"
        );
        fs::write(dir.join(MANIFEST_FILE), "name = \"notes\"\napi = 7").unwrap();
        fs::write(dir.join(BRIEF_FILE), "Notes.").unwrap();
        let error = Folder::read(&dir).unwrap_err();
        assert!(error.starts_with("app.toml: "), "{error}");
        fs::write(dir.join(MANIFEST_FILE), manifest().to_text()).unwrap();
        fs::write(dir.join(SCRIPT_FILE), vec![b'-'; SCRIPT_BYTES + 1]).unwrap();
        let error = Folder::read(&dir).unwrap_err();
        assert_eq!(error, "app.lua is 131073 bytes; it may be 131072");
        fs::write(dir.join(SCRIPT_FILE), vec![b'-'; SCRIPT_BYTES]).unwrap();
        fs::write(dir.join(BRIEF_FILE), vec![b'a'; BRIEF_BYTES + 5000]).unwrap();
        let error = Folder::read(&dir).unwrap_err();
        assert_eq!(error, "brief.md is 21384 bytes; it may be 16384");
    }
}

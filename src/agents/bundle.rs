//! The file `bzz agent move` makes: one agent, sealed with a passphrase
//! (plan v0.15.0, M5).
//!
//! Only cryptography bzz already had is used: Argon2id turns the passphrase
//! into a key, as for an identity in an encrypted file, and
//! XChaCha20-Poly1305 seals the content in pieces of at most 1 MiB, so that
//! an agent's disk of any size is written and read without being held in
//! memory.
//!
//! ```text
//! head   112 bytes  magic, version, Argon2id parameters, salt, nonce prefix,
//!                   the file's whole length, SHA-256 of the head's fields
//! piece 0           what the caller carries (settings and key) and the list
//!                   of files with their sizes and SHA-256
//! piece 1 … n       the files' bytes, one after the other; the last piece
//!                   is sealed as the last
//! tail    32 bytes  SHA-256 of everything before it
//! ```
//!
//! Each piece is `length (4 bytes) || ciphertext`. Its nonce is the prefix,
//! its number and whether it is the last; the head's digest is its
//! associated data. So pieces cannot be reordered, dropped, or taken from
//! another file.
//!
//! The two plain SHA-256 values protect nothing: whoever can change the file
//! can change them. They are what lets bzz **say what is wrong** before any
//! key is derived: a file that was cut short, a file that was changed, a
//! wrong passphrase. What cannot be forged is the sealing itself.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize as _, Zeroizing};

const MAGIC: [u8; 16] = *b"bzz agent move\n\0";
const VERSION: u8 = 1;
const HEAD: usize = 112;
const DIGEST: usize = 32;
const TAG: usize = 16;
/// The most bytes one piece holds before it is sealed.
pub const PIECE: usize = 1 << 20;
/// As for an identity in an encrypted file.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

/// How the passphrase becomes a key. The values are written in the file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Kdf {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for Kdf {
    /// The parameters of bzz's encrypted identity file.
    fn default() -> Self {
        Self {
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 1,
        }
    }
}

impl Kdf {
    /// Whether a file may ask for these: enough to be Argon2id at all, and
    /// not so much that opening a file a stranger made exhausts the machine.
    const fn acceptable(self) -> bool {
        self.memory_kib >= 8
            && self.memory_kib <= 1_048_576
            && self.iterations >= 1
            && self.iterations <= 20
            && self.parallelism >= 1
            && self.parallelism <= 16
            && self.memory_kib >= 8 * self.parallelism
    }
}

/// One file carried inside.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// Why a file was not made or not opened. Each says which it is.
#[derive(Debug)]
pub enum Refusal {
    /// Not a file `bzz agent move` made.
    NotOurs,
    /// Made by a bzz that writes another version of the format.
    Version(u8),
    /// The end is missing.
    CutShort {
        expected: u64,
        found: u64,
    },
    /// Something in it is not as bzz wrote it.
    Changed(&'static str),
    WrongPassphrase,
    WeakPassphrase,
    /// What was to be packed cannot be.
    Unpackable(String),
    Io {
        path: PathBuf,
        why: std::io::Error,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOurs => write!(formatter, "this is not a file made by `bzz agent move`"),
            Self::Version(version) => write!(
                formatter,
                "this file was made by a newer bzz (format {version}; this bzz reads format {VERSION}): update bzz and try again"
            ),
            Self::CutShort { expected, found } => write!(
                formatter,
                "the file was cut short: it should hold {expected} bytes and holds {found}. Copy it again from where it was made; nothing was received"
            ),
            Self::Changed(what) => write!(
                formatter,
                "the file was changed after bzz made it ({what}). Copy it again from where it was made; nothing was received"
            ),
            Self::WrongPassphrase => write!(
                formatter,
                "the passphrase is wrong: the file is whole, and it does not open with it"
            ),
            Self::WeakPassphrase => write!(
                formatter,
                "the passphrase must contain at least {MIN_PASSPHRASE_CHARS} characters: it is all that protects the agent's key in this file"
            ),
            Self::Unpackable(why) => write!(formatter, "{why}"),
            Self::Io { path, why } => write!(formatter, "{}: {why}", path.display()),
        }
    }
}

impl From<Refusal> for crate::error::Error {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Io { path, why } => Self::io(path, why),
            Refusal::WrongPassphrase | Refusal::WeakPassphrase => Self::Auth(refusal.to_string()),
            other => Self::Config(other.to_string()),
        }
    }
}

type Outcome<T> = std::result::Result<T, Refusal>;

fn io(path: &Path) -> impl Fn(std::io::Error) -> Refusal + '_ {
    move |why| Refusal::Io {
        path: path.to_path_buf(),
        why,
    }
}

fn derive_key(passphrase: &str, salt: &[u8], kdf: Kdf) -> Outcome<Zeroizing<[u8; 32]>> {
    let params = Params::new(kdf.memory_kib, kdf.iterations, kdf.parallelism, Some(32))
        .map_err(|_| Refusal::Changed("its key parameters"))?;
    let mut key = Zeroizing::new([0_u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| Refusal::Changed("its key parameters"))?;
    Ok(key)
}

fn nonce(prefix: &[u8; 19], number: u32, last: bool) -> [u8; 24] {
    let mut nonce = [0_u8; 24];
    nonce[..19].copy_from_slice(prefix);
    nonce[19..23].copy_from_slice(&number.to_be_bytes());
    nonce[23] = u8::from(last);
    nonce
}

/// A simple name of one file: nothing in it can lead out of a folder.
fn simple_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn open_private(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// Size and SHA-256 of a file, read once.
fn measure(name: &str, path: &Path) -> Outcome<FileEntry> {
    let mut file = File::open(path).map_err(io(path))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(io(path))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        size += read as u64;
    }
    Ok(FileEntry {
        name: name.to_owned(),
        size,
        sha256: hex::encode(hash.finalize()),
    })
}

struct Sealer<'a> {
    file: File,
    path: &'a Path,
    cipher: XChaCha20Poly1305,
    prefix: [u8; 19],
    head_digest: [u8; DIGEST],
    whole: Sha256,
    written: u64,
    number: u32,
}

impl Sealer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Outcome<()> {
        self.file.write_all(bytes).map_err(io(self.path))?;
        self.whole.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn piece(&mut self, plain: &[u8], last: bool) -> Outcome<()> {
        let sealed = self
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce(&self.prefix, self.number, last)),
                Payload {
                    msg: plain,
                    aad: &self.head_digest,
                },
            )
            .map_err(|_| Refusal::Unpackable("a piece could not be sealed".into()))?;
        self.number = self
            .number
            .checked_add(1)
            .ok_or_else(|| Refusal::Unpackable("the agent is too large for one file".into()))?;
        let length = u32::try_from(sealed.len())
            .map_err(|_| Refusal::Unpackable("a piece is too large".into()))?;
        self.put(&length.to_be_bytes())?;
        self.put(&sealed)
    }
}

/// How many bytes the pieces of `data` bytes take in the file.
fn data_pieces_len(data: u64) -> u64 {
    let piece = PIECE as u64;
    let frame = |plain: u64| 4 + plain + TAG as u64;
    match (data / piece, data % piece) {
        (0, 0) => frame(0),
        (full, 0) => full * frame(piece),
        (full, rest) => full * frame(piece) + frame(rest),
    }
}

/// Seals `content` and `files` into the new file `to`, which must not
/// exist. On any failure nothing is left at `to`.
///
/// `content` is the caller's own bytes (settings and key); `files` are
/// `(name, path)`, each carried whole under its name.
pub fn seal(
    to: &Path,
    passphrase: &str,
    content: &[u8],
    files: &[(&str, &Path)],
    kdf: Kdf,
) -> Outcome<Vec<FileEntry>> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(Refusal::WeakPassphrase);
    }
    let mut entries = Vec::new();
    for (name, path) in files {
        if !simple_name(name) || entries.iter().any(|entry: &FileEntry| entry.name == *name) {
            return Err(Refusal::Unpackable(format!(
                "{name:?} cannot be the name of a carried file"
            )));
        }
        entries.push(measure(name, path)?);
    }
    let index =
        serde_json::to_vec(&entries).map_err(|error| Refusal::Unpackable(error.to_string()))?;
    let mut first = Zeroizing::new(Vec::with_capacity(4 + index.len() + content.len()));
    first.extend_from_slice(
        &u32::try_from(index.len())
            .map_err(|_| Refusal::Unpackable("too many files".into()))?
            .to_be_bytes(),
    );
    first.extend_from_slice(&index);
    first.extend_from_slice(content);
    if first.len() > PIECE {
        return Err(Refusal::Unpackable(
            "the agent's settings are too long to carry (over 1 MiB)".into(),
        ));
    }
    let data = entries.iter().map(|entry| entry.size).sum::<u64>();
    let length =
        HEAD as u64 + 4 + first.len() as u64 + TAG as u64 + data_pieces_len(data) + DIGEST as u64;

    let mut salt = [0_u8; 16];
    let mut prefix = [0_u8; 19];
    rand::rng().fill(&mut salt);
    rand::rng().fill(&mut prefix);
    let mut head = [0_u8; HEAD];
    head[..16].copy_from_slice(&MAGIC);
    head[16] = VERSION;
    head[20..24].copy_from_slice(&kdf.memory_kib.to_be_bytes());
    head[24..28].copy_from_slice(&kdf.iterations.to_be_bytes());
    head[28..32].copy_from_slice(&kdf.parallelism.to_be_bytes());
    head[32..48].copy_from_slice(&salt);
    head[48..67].copy_from_slice(&prefix);
    head[72..80].copy_from_slice(&length.to_be_bytes());
    let head_digest: [u8; DIGEST] = Sha256::digest(&head[16..80]).into();
    head[80..].copy_from_slice(&head_digest);

    if !kdf.acceptable() {
        return Err(Refusal::Unpackable("unusable key parameters".into()));
    }
    let key = derive_key(passphrase, &salt, kdf)?;
    let file = open_private(to).map_err(io(to))?;
    let mut sealer = Sealer {
        file,
        path: to,
        cipher: XChaCha20Poly1305::new((&*key).into()),
        prefix,
        head_digest,
        whole: Sha256::new(),
        written: 0,
        number: 0,
    };
    let result = (|| {
        sealer.put(&head)?;
        sealer.piece(&first, false)?;
        // The files' bytes as one stream, cut every PIECE bytes.
        let mut buffer = Zeroizing::new(Vec::with_capacity(PIECE));
        let mut chunk = vec![0_u8; 64 * 1024];
        for ((_, path), entry) in files.iter().zip(&entries) {
            let mut source = File::open(path).map_err(io(path))?;
            let mut hash = Sha256::new();
            let mut size = 0_u64;
            loop {
                let read = source.read(&mut chunk).map_err(io(path))?;
                if read == 0 {
                    break;
                }
                hash.update(&chunk[..read]);
                size += read as u64;
                let mut rest = &chunk[..read];
                while !rest.is_empty() {
                    // A full piece is sealed only when more follows: the
                    // last piece is never empty unless there is no data.
                    if buffer.len() == PIECE {
                        sealer.piece(&buffer, false)?;
                        buffer.clear();
                    }
                    let take = rest.len().min(PIECE - buffer.len());
                    buffer.extend_from_slice(&rest[..take]);
                    rest = &rest[take..];
                }
            }
            if size != entry.size || hex::encode(hash.finalize()) != entry.sha256 {
                return Err(Refusal::Unpackable(format!(
                    "{} changed while it was being packed: is the agent still running?",
                    entry.name
                )));
            }
        }
        sealer.piece(&buffer, true)?;
        let digest: [u8; DIGEST] = sealer.whole.clone().finalize().into();
        sealer.file.write_all(&digest).map_err(io(to))?;
        sealer.written += DIGEST as u64;
        if sealer.written != length {
            return Err(Refusal::Unpackable(
                "the file did not come out at its planned length".into(),
            ));
        }
        sealer.file.sync_all().map_err(io(to))
    })();
    match result {
        Ok(()) => Ok(entries),
        Err(refusal) => {
            drop(sealer);
            let _ = std::fs::remove_file(to);
            Err(refusal)
        }
    }
}

/// A file that was checked whole and opened with its passphrase.
pub struct Opened {
    file: File,
    path: PathBuf,
    cipher: XChaCha20Poly1305,
    prefix: [u8; 19],
    head_digest: [u8; DIGEST],
    /// Where the pieces end and the tail begins.
    body_end: u64,
    position: u64,
    number: u32,
    /// The caller's own bytes, as given to [`seal`].
    pub content: Zeroizing<Vec<u8>>,
    pub files: Vec<FileEntry>,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Opened")
            .field("path", &self.path)
            .field("files", &self.files)
            .finish_non_exhaustive()
    }
}

/// Reads as much of `buffer` as the file has.
fn read_up_to(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

/// Checks the whole file, then opens it. Nothing of it is used before every
/// check has passed: its length, both digests, and the passphrase.
pub fn open(path: &Path, passphrase: &str) -> Outcome<Opened> {
    let mut file = File::open(path).map_err(io(path))?;
    let found = file.metadata().map_err(io(path))?.len();
    let mut head = [0_u8; HEAD];
    let read = read_up_to(&mut file, &mut head).map_err(io(path))?;
    if read < HEAD {
        // Too short to say more: ours if it begins as ours do.
        let begins = read.min(MAGIC.len());
        return Err(if read > 0 && head[..begins] == MAGIC[..begins] {
            Refusal::CutShort {
                expected: HEAD as u64 + DIGEST as u64,
                found,
            }
        } else {
            Refusal::NotOurs
        });
    }
    let head_whole = Sha256::digest(&head[16..80]).as_slice() == &head[80..];
    match (head[..16] == MAGIC, head_whole) {
        (true, true) => {}
        (false, false) => return Err(Refusal::NotOurs),
        (false, true) => return Err(Refusal::Changed("its first bytes")),
        (true, false) => return Err(Refusal::Changed("its head")),
    }
    if head[16] != VERSION {
        return Err(Refusal::Version(head[16]));
    }
    let number =
        |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    let kdf = Kdf {
        memory_kib: number(20),
        iterations: number(24),
        parallelism: number(28),
    };
    let mut expected = [0_u8; 8];
    expected.copy_from_slice(&head[72..80]);
    let expected = u64::from_be_bytes(expected);
    if found < expected {
        return Err(Refusal::CutShort { expected, found });
    }
    if found > expected {
        return Err(Refusal::Changed("bytes were added at its end"));
    }
    // Smallest whole file: head, two empty pieces, tail.
    if expected < (HEAD + 2 * (4 + TAG) + DIGEST) as u64 || !kdf.acceptable() {
        return Err(Refusal::Changed("its head"));
    }
    let body_end = expected - DIGEST as u64;

    // The whole file against its own digest, before any key is derived.
    let mut whole = Sha256::new();
    whole.update(head);
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut left = body_end - HEAD as u64;
    while left > 0 {
        let take = usize::try_from(left.min(buffer.len() as u64)).unwrap_or(buffer.len());
        file.read_exact(&mut buffer[..take]).map_err(io(path))?;
        whole.update(&buffer[..take]);
        left -= take as u64;
    }
    let mut tail = [0_u8; DIGEST];
    file.read_exact(&mut tail).map_err(io(path))?;
    if whole.finalize().as_slice() != tail {
        return Err(Refusal::Changed(
            "its content does not match its own checksum",
        ));
    }

    let mut salt = [0_u8; 16];
    salt.copy_from_slice(&head[32..48]);
    let mut prefix = [0_u8; 19];
    prefix.copy_from_slice(&head[48..67]);
    let mut head_digest = [0_u8; DIGEST];
    head_digest.copy_from_slice(&head[80..]);
    let key = derive_key(passphrase, &salt, kdf)?;
    salt.zeroize();
    file.seek(SeekFrom::Start(HEAD as u64)).map_err(io(path))?;
    let mut opened = Opened {
        file,
        path: path.to_path_buf(),
        cipher: XChaCha20Poly1305::new((&*key).into()),
        prefix,
        head_digest,
        body_end,
        position: HEAD as u64,
        number: 0,
        content: Zeroizing::new(Vec::new()),
        files: Vec::new(),
    };
    // The first piece is the only one that says whether the passphrase is
    // right: the file's own checksum has passed, so a failure here is the
    // passphrase.
    let first = opened
        .piece()
        .map_err(|refusal| match refusal {
            Refusal::Changed(_) => Refusal::WrongPassphrase,
            other => other,
        })?
        .ok_or(Refusal::Changed("it holds no content"))?;
    let index_len = first
        .get(..4)
        .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
        .ok_or(Refusal::Changed("its list of files"))?;
    let index = first
        .get(4..4 + index_len)
        .ok_or(Refusal::Changed("its list of files"))?;
    opened.files =
        serde_json::from_slice(index).map_err(|_| Refusal::Changed("its list of files"))?;
    let mut names = std::collections::HashSet::new();
    if !opened
        .files
        .iter()
        .all(|entry| simple_name(&entry.name) && names.insert(entry.name.as_str()))
    {
        return Err(Refusal::Changed("its list of files"));
    }
    opened.content = Zeroizing::new(first[4 + index_len..].to_vec());
    Ok(opened)
}

impl Opened {
    /// The next piece, opened. `None`: the pieces have ended.
    fn piece(&mut self) -> Outcome<Option<Zeroizing<Vec<u8>>>> {
        if self.position >= self.body_end {
            return Ok(None);
        }
        let changed = || Refusal::Changed("a sealed piece does not open");
        let mut length = [0_u8; 4];
        self.file.read_exact(&mut length).map_err(io(&self.path))?;
        let length = u64::from(u32::from_be_bytes(length));
        let after = self.position + 4 + length;
        if length < TAG as u64 || length > (PIECE + TAG) as u64 || after > self.body_end {
            return Err(changed());
        }
        let mut sealed = vec![0_u8; usize::try_from(length).map_err(|_| changed())?];
        self.file.read_exact(&mut sealed).map_err(io(&self.path))?;
        // Whether a piece is the last is where it lies, not what it says:
        // a file cut at a piece's edge does not end with a last piece.
        let last = after == self.body_end;
        let plain = self
            .cipher
            .decrypt(
                XNonce::from_slice(&nonce(&self.prefix, self.number, last)),
                Payload {
                    msg: &sealed,
                    aad: &self.head_digest,
                },
            )
            .map_err(|_| changed())?;
        self.position = after;
        self.number = self.number.checked_add(1).ok_or_else(changed)?;
        Ok(Some(Zeroizing::new(plain)))
    }

    /// Writes every carried file to the path `target` gives for its name,
    /// which must not exist, and checks each against its size and SHA-256.
    /// On any failure the files written so far are removed.
    pub fn unpack(mut self, target: impl Fn(&str) -> PathBuf) -> Outcome<()> {
        let mut written = Vec::new();
        let result = self.unpack_into(&target, &mut written);
        if result.is_err() {
            for path in written {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }

    fn unpack_into(
        &mut self,
        target: &impl Fn(&str) -> PathBuf,
        written: &mut Vec<PathBuf>,
    ) -> Outcome<()> {
        let mut piece = Zeroizing::new(Vec::new());
        let mut at = 0;
        for entry in self.files.clone() {
            let path = target(&entry.name);
            let mut file = open_private(&path).map_err(io(&path))?;
            written.push(path.clone());
            let mut hash = Sha256::new();
            let mut left = entry.size;
            while left > 0 {
                if at == piece.len() {
                    piece = self
                        .piece()?
                        .ok_or(Refusal::Changed("it ends before its files do"))?;
                    at = 0;
                    if piece.is_empty() {
                        return Err(Refusal::Changed("it ends before its files do"));
                    }
                }
                let take = usize::try_from(left.min((piece.len() - at) as u64))
                    .unwrap_or(piece.len() - at);
                file.write_all(&piece[at..at + take]).map_err(io(&path))?;
                hash.update(&piece[at..at + take]);
                at += take;
                left -= take as u64;
            }
            file.sync_all().map_err(io(&path))?;
            if hex::encode(hash.finalize()) != entry.sha256 {
                return Err(Refusal::Changed("a file in it does not match its checksum"));
            }
        }
        // Nothing may follow the files but the end of an empty last piece.
        let rest = if at == piece.len() {
            self.piece()?
        } else {
            return Err(Refusal::Changed("it holds more than its files"));
        };
        match rest {
            None => {}
            Some(extra) if extra.is_empty() && self.position == self.body_end => {}
            Some(_) => return Err(Refusal::Changed("it holds more than its files")),
        }
        if self.position != self.body_end {
            return Err(Refusal::Changed("it holds more than its files"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Argon2id at its cheapest: these tests open a file hundreds of times.
    const QUICK: Kdf = Kdf {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };
    const PASSPHRASE: &str = "a disposable passphrase";

    struct Fixture {
        folder: tempfile::TempDir,
        memory: Vec<u8>,
        disk: Vec<u8>,
    }

    impl Fixture {
        fn new(memory: usize, disk: usize) -> Self {
            let folder = tempfile::TempDir::new().unwrap();
            let bytes = |length: usize, seed: u8| {
                (0..length)
                    .map(|index| (index % 251) as u8 ^ seed)
                    .collect::<Vec<_>>()
            };
            let (memory, disk) = (bytes(memory, 0x5a), bytes(disk, 0xa5));
            std::fs::write(folder.path().join("memory.sqlite"), &memory).unwrap();
            std::fs::write(folder.path().join("files.sqlite"), &disk).unwrap();
            Self {
                folder,
                memory,
                disk,
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.folder.path().join(name)
        }

        fn seal(&self, to: &str, content: &[u8]) -> Outcome<Vec<FileEntry>> {
            let (memory, disk) = (self.path("memory.sqlite"), self.path("files.sqlite"));
            seal(
                &self.path(to),
                PASSPHRASE,
                content,
                &[("memory.sqlite", &memory), ("files.sqlite", &disk)],
                QUICK,
            )
        }

        /// Opens `file` and unpacks it into a fresh folder.
        fn receive(&self, file: &str, passphrase: &str) -> Outcome<(Vec<u8>, Vec<u8>, Vec<u8>)> {
            let opened = open(&self.path(file), passphrase)?;
            let content = opened.content.to_vec();
            let into = tempfile::TempDir::new_in(self.folder.path()).unwrap();
            opened.unpack(|name| into.path().join(name))?;
            Ok((
                content,
                std::fs::read(into.path().join("memory.sqlite")).unwrap(),
                std::fs::read(into.path().join("files.sqlite")).unwrap(),
            ))
        }
    }

    #[test]
    fn what_is_sealed_comes_out_as_it_went_in() {
        // Sizes around the edges of a piece: nothing, a little, exactly one
        // piece, one byte more, and several pieces across two files.
        for (memory, disk) in [
            (0, 0),
            (1, 0),
            (5000, 300),
            (PIECE, 0),
            (PIECE - 7, 7),
            (PIECE, 1),
            (2 * PIECE + 11, PIECE + 5),
        ] {
            let fixture = Fixture::new(memory, disk);
            let content = br#"{"name":"Scout","key":"0123"}"#;
            let entries = fixture.seal("scout.bzzagent", content).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| (entry.name.as_str(), entry.size))
                    .collect::<Vec<_>>(),
                [
                    ("memory.sqlite", memory as u64),
                    ("files.sqlite", disk as u64)
                ]
            );
            let (carried, got_memory, got_disk) =
                fixture.receive("scout.bzzagent", PASSPHRASE).unwrap();
            assert_eq!(carried, content, "{memory}+{disk}");
            assert!(got_memory == fixture.memory, "{memory}+{disk}: memory");
            assert!(got_disk == fixture.disk, "{memory}+{disk}: disk");
            // The length the head promised is the length on disk.
            let sealed = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
            let mut promised = [0_u8; 8];
            promised.copy_from_slice(&sealed[72..80]);
            assert_eq!(u64::from_be_bytes(promised), sealed.len() as u64);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = std::fs::metadata(fixture.path("scout.bzzagent"))
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600);
            }
        }
    }

    #[test]
    fn nothing_carried_is_readable_in_the_file() {
        let fixture = Fixture::new(4000, 4000);
        let secret = "8f2d7c1e-this-stands-for-a-key-5b6a";
        fixture
            .seal(
                "scout.bzzagent",
                format!("{{\"key\":\"{secret}\"}}").as_bytes(),
            )
            .unwrap();
        let sealed = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
        let holds = |needle: &[u8]| sealed.windows(needle.len()).any(|window| window == needle);
        assert!(!holds(secret.as_bytes()), "the content is sealed");
        assert!(!holds(b"memory.sqlite"), "so are the names of the files");
        assert!(!holds(&fixture.memory[..64]) && !holds(&fixture.disk[..64]));
        // Two files of the same agent and passphrase share no sealed byte
        // run: salt and nonce are new each time.
        fixture
            .seal(
                "again.bzzagent",
                format!("{{\"key\":\"{secret}\"}}").as_bytes(),
            )
            .unwrap();
        let again = std::fs::read(fixture.path("again.bzzagent")).unwrap();
        assert_ne!(sealed[HEAD..HEAD + 64], again[HEAD..HEAD + 64]);
    }

    #[test]
    fn a_wrong_passphrase_is_named_as_that() {
        let fixture = Fixture::new(900, 100);
        fixture.seal("scout.bzzagent", b"{}").unwrap();
        for wrong in ["a disposable passphrasE", "", "another passphrase entirely"] {
            let refusal = fixture.receive("scout.bzzagent", wrong).unwrap_err();
            assert!(
                matches!(refusal, Refusal::WrongPassphrase),
                "{wrong:?}: {refusal}"
            );
            assert!(refusal.to_string().contains("the passphrase is wrong"));
        }
        // A short passphrase never seals anything, and leaves no file.
        let (memory, disk) = (fixture.path("memory.sqlite"), fixture.path("files.sqlite"));
        let refusal = seal(
            &fixture.path("weak.bzzagent"),
            "too short",
            b"{}",
            &[("memory.sqlite", &memory), ("files.sqlite", &disk)],
            QUICK,
        )
        .unwrap_err();
        assert!(matches!(refusal, Refusal::WeakPassphrase), "{refusal}");
        assert!(!fixture.path("weak.bzzagent").exists());
    }

    #[test]
    fn a_flipped_byte_anywhere_is_named_as_a_change() {
        let fixture = Fixture::new(700, 300);
        fixture
            .seal("scout.bzzagent", b"{\"name\":\"Scout\"}")
            .unwrap();
        let whole = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
        assert!(whole.len() > HEAD + 1000 && whole.len() < 4000);
        // Every byte of the file, one at a time, one bit each.
        for at in 0..whole.len() {
            let mut changed = whole.clone();
            changed[at] ^= 1 << (at % 8);
            std::fs::write(fixture.path("changed.bzzagent"), &changed).unwrap();
            let refusal = fixture.receive("changed.bzzagent", PASSPHRASE).unwrap_err();
            assert!(
                matches!(refusal, Refusal::Changed(_)),
                "byte {at} of {}: {refusal}",
                whole.len()
            );
            assert!(refusal.to_string().contains("was changed"), "{refusal}");
        }
        // Bytes added at the end.
        let mut longer = whole.clone();
        longer.push(0);
        std::fs::write(fixture.path("changed.bzzagent"), &longer).unwrap();
        assert!(matches!(
            fixture.receive("changed.bzzagent", PASSPHRASE).unwrap_err(),
            Refusal::Changed(_)
        ));
        // And the file as it was still opens.
        std::fs::write(fixture.path("changed.bzzagent"), &whole).unwrap();
        fixture.receive("changed.bzzagent", PASSPHRASE).unwrap();
    }

    #[test]
    fn a_file_cut_short_anywhere_is_named_as_that() {
        let fixture = Fixture::new(700, 300);
        fixture
            .seal("scout.bzzagent", b"{\"name\":\"Scout\"}")
            .unwrap();
        let whole = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
        for length in 1..whole.len() {
            std::fs::write(fixture.path("cut.bzzagent"), &whole[..length]).unwrap();
            let refusal = fixture.receive("cut.bzzagent", PASSPHRASE).unwrap_err();
            assert!(
                matches!(refusal, Refusal::CutShort { .. }),
                "cut at {length} of {}: {refusal}",
                whole.len()
            );
            assert!(refusal.to_string().contains("was cut short"), "{refusal}");
        }
        // Nothing at all, and something that never was such a file.
        std::fs::write(fixture.path("cut.bzzagent"), b"").unwrap();
        assert!(matches!(
            fixture.receive("cut.bzzagent", PASSPHRASE).unwrap_err(),
            Refusal::NotOurs
        ));
        std::fs::write(fixture.path("cut.bzzagent"), vec![b'x'; 5000]).unwrap();
        assert!(matches!(
            fixture.receive("cut.bzzagent", PASSPHRASE).unwrap_err(),
            Refusal::NotOurs
        ));
    }

    /// Rewrites both plain checksums, as someone who knows the format would
    /// after changing the file.
    fn with_checksums_made_right(mut bytes: Vec<u8>) -> Vec<u8> {
        let length = bytes.len() as u64;
        bytes[72..80].copy_from_slice(&length.to_be_bytes());
        let head: [u8; DIGEST] = Sha256::digest(&bytes[16..80]).into();
        bytes[80..HEAD].copy_from_slice(&head);
        let end = bytes.len() - DIGEST;
        let whole: [u8; DIGEST] = Sha256::digest(&bytes[..end]).into();
        bytes[end..].copy_from_slice(&whole);
        bytes
    }

    #[test]
    fn a_forgery_with_its_checksums_made_right_still_does_not_open() {
        let fixture = Fixture::new(PIECE + 500, 300);
        fixture
            .seal("scout.bzzagent", b"{\"name\":\"Scout\"}")
            .unwrap();
        let whole = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
        let attempt = |bytes: Vec<u8>| {
            std::fs::write(fixture.path("forged.bzzagent"), bytes).unwrap();
            fixture.receive("forged.bzzagent", PASSPHRASE).unwrap_err()
        };
        // The untouched file with its checksums written again opens: the
        // helper forges nothing by itself.
        std::fs::write(
            fixture.path("same.bzzagent"),
            with_checksums_made_right(whole.clone()),
        )
        .unwrap();
        fixture.receive("same.bzzagent", PASSPHRASE).unwrap();

        // Each of these is found by the sealing itself, not by a checksum of
        // the files inside.
        let unopened = |refusal: Refusal| {
            assert!(
                matches!(refusal, Refusal::Changed("a sealed piece does not open")),
                "{refusal}"
            );
        };
        // A byte of a file's piece.
        let mut changed = whole.clone();
        let at = whole.len() - DIGEST - 40;
        changed[at] ^= 0x80;
        unopened(attempt(with_checksums_made_right(changed)));
        // The last piece dropped. The file's length is in the head and the
        // head is bound to every piece, so a head that says the shorter
        // length opens nothing: not even the first piece.
        let first_len = u32::from_be_bytes(whole[HEAD..HEAD + 4].try_into().unwrap()) as usize;
        let second = HEAD + 4 + first_len;
        let second_len = u32::from_be_bytes(whole[second..second + 4].try_into().unwrap()) as usize;
        let mut dropped = whole[..second + 4 + second_len].to_vec();
        dropped.extend_from_slice(&[0_u8; DIGEST]);
        let refusal = attempt(with_checksums_made_right(dropped));
        assert!(matches!(refusal, Refusal::WrongPassphrase), "{refusal}");
        // Two pieces in each other's place.
        let third = second + 4 + second_len;
        let end = whole.len() - DIGEST;
        let mut swapped = whole[..second].to_vec();
        swapped.extend_from_slice(&whole[third..end]);
        swapped.extend_from_slice(&whole[second..third]);
        swapped.extend_from_slice(&[0_u8; DIGEST]);
        unopened(attempt(with_checksums_made_right(swapped)));
        // A piece of another file sealed with the same passphrase.
        fixture
            .seal("other.bzzagent", b"{\"name\":\"Other\"}")
            .unwrap();
        let other = std::fs::read(fixture.path("other.bzzagent")).unwrap();
        let mut grafted = whole[..second].to_vec();
        grafted.extend_from_slice(&other[second..]);
        unopened(attempt(with_checksums_made_right(grafted)));
        // Two pieces of equal length in each other's place: only their
        // numbers tell them apart.
        let equal = Fixture::new(2 * PIECE, 0);
        equal.seal("equal.bzzagent", b"{}").unwrap();
        let bytes = std::fs::read(equal.path("equal.bzzagent")).unwrap();
        let first_len = u32::from_be_bytes(bytes[HEAD..HEAD + 4].try_into().unwrap()) as usize;
        let (one, piece) = (HEAD + 4 + first_len, 4 + PIECE + TAG);
        let mut crossed = bytes[..one].to_vec();
        crossed.extend_from_slice(&bytes[one + piece..one + 2 * piece]);
        crossed.extend_from_slice(&bytes[one..one + piece]);
        crossed.extend_from_slice(&bytes[one + 2 * piece..]);
        assert_eq!(crossed.len(), bytes.len());
        std::fs::write(
            equal.path("crossed.bzzagent"),
            with_checksums_made_right(crossed),
        )
        .unwrap();
        unopened(equal.receive("crossed.bzzagent", PASSPHRASE).unwrap_err());
        // A changed head (weaker key parameters) no longer yields the key.
        let mut weaker = whole.clone();
        weaker[24..28].copy_from_slice(&2_u32.to_be_bytes());
        assert!(matches!(
            attempt(with_checksums_made_right(weaker)),
            Refusal::WrongPassphrase
        ));
        // Parameters no file of ours asks for are refused before any work.
        let mut greedy = whole.clone();
        greedy[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            attempt(with_checksums_made_right(greedy)),
            Refusal::Changed(_)
        ));
    }

    #[test]
    fn a_failed_unpack_leaves_no_file_and_a_taken_name_is_refused() {
        let fixture = Fixture::new(PIECE + 500, 300);
        fixture.seal("scout.bzzagent", b"{}").unwrap();
        // Sealing never writes over a file that is there.
        let refusal = fixture.seal("scout.bzzagent", b"{}").unwrap_err();
        assert!(matches!(refusal, Refusal::Io { .. }), "{refusal}");
        fixture.receive("scout.bzzagent", PASSPHRASE).unwrap();
        // A forged last piece is found only while unpacking: the files
        // written before it are removed.
        let whole = std::fs::read(fixture.path("scout.bzzagent")).unwrap();
        let mut changed = whole.clone();
        let at = whole.len() - DIGEST - 40;
        changed[at] ^= 0x80;
        std::fs::write(
            fixture.path("forged.bzzagent"),
            with_checksums_made_right(changed),
        )
        .unwrap();
        let into = tempfile::TempDir::new().unwrap();
        let opened = open(&fixture.path("forged.bzzagent"), PASSPHRASE).unwrap();
        assert!(opened.unpack(|name| into.path().join(name)).is_err());
        assert_eq!(std::fs::read_dir(into.path()).unwrap().count(), 0);
        // Unpacking never writes over a file that is there either.
        std::fs::write(into.path().join("memory.sqlite"), b"mine").unwrap();
        let opened = open(&fixture.path("scout.bzzagent"), PASSPHRASE).unwrap();
        assert!(opened.unpack(|name| into.path().join(name)).is_err());
        assert_eq!(
            std::fs::read(into.path().join("memory.sqlite")).unwrap(),
            b"mine"
        );
        // A name that could lead out of a folder is never carried.
        let memory = fixture.path("memory.sqlite");
        for name in ["../memory.sqlite", "a/b", "", ".hidden"] {
            assert!(
                seal(
                    &fixture.path("bad.bzzagent"),
                    PASSPHRASE,
                    b"{}",
                    &[(name, &memory)],
                    QUICK
                )
                .is_err(),
                "{name:?}"
            );
        }
        // A newer format says so.
        let mut newer = whole.clone();
        newer[16] = 2;
        std::fs::write(
            fixture.path("newer.bzzagent"),
            with_checksums_made_right(newer),
        )
        .unwrap();
        let refusal = fixture.receive("newer.bzzagent", PASSPHRASE).unwrap_err();
        assert!(matches!(refusal, Refusal::Version(2)), "{refusal}");
    }
}

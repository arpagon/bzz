//! Lua 5.4 string patterns, matched in safe Rust with a deadline.
//!
//! A port of the matcher of `lstrlib.c` (Lua 5.4.9): same classes, sets,
//! quantifiers, anchors, captures, back-references, `%b` and `%f`, the same
//! errors and the same recursion limit. Two things differ on purpose:
//!
//! - every step is counted, and every `CHECK_EVERY` steps the deadline is
//!   read: a match that runs past it ends with [`Stop::Timeout`];
//! - a plain search is done by `memchr::memmem`, which is linear in the worst
//!   case (the C `lmemfind` is quadratic).
//!
//! Nothing here knows about `mlua`: subject and pattern are byte slices.

use std::time::Instant;

pub const MAX_CAPTURES: usize = 32;
/// `MAXCCALLS` of `lstrlib.c`: how deep `match` may recurse.
pub const MAX_DEPTH: u32 = 200;
const CHECK_EVERY: u32 = 1 << 12;

const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;

#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// A malformed pattern or replacement; the text is Lua's own.
    Error(String),
    /// The deadline passed while matching.
    Timeout,
}

type M<T> = Result<T, Stop>;

fn err<T>(msg: impl Into<String>) -> M<T> {
    Err(Stop::Error(msg.into()))
}

/// One capture of a finished match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// A substring of the subject: start and length.
    Text(usize, usize),
    /// A position capture `()`: the 1-based position.
    Position(usize),
}

pub struct Matcher<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, isize); MAX_CAPTURES],
    depth: u32,
    steps: u32,
    deadline: Instant,
}

impl<'a> Matcher<'a> {
    pub fn new(src: &'a [u8], pat: &'a [u8], deadline: Instant) -> Self {
        Matcher {
            src,
            pat,
            level: 0,
            capture: [(0, 0); MAX_CAPTURES],
            depth: MAX_DEPTH,
            steps: 0,
            deadline,
        }
    }

    /// Tries the pattern at subject offset `s`. Returns the end of the match.
    pub fn match_at(&mut self, s: usize) -> M<Option<usize>> {
        self.depth = MAX_DEPTH;
        self.level = 0;
        self.do_match(s, 0)
    }

    #[inline]
    fn tick(&mut self) -> M<()> {
        self.steps = self.steps.wrapping_add(1);
        if self.steps.is_multiple_of(CHECK_EVERY) && Instant::now() > self.deadline {
            return Err(Stop::Timeout);
        }
        Ok(())
    }

    /// Counts `n` steps of a loop that did not call `tick` itself.
    #[inline]
    fn spend(&mut self, n: usize) -> M<()> {
        let before = self.steps / CHECK_EVERY;
        self.steps = self.steps.wrapping_add(n.min(u32::MAX as usize) as u32);
        if (self.steps / CHECK_EVERY != before || n >= CHECK_EVERY as usize)
            && Instant::now() > self.deadline
        {
            return Err(Stop::Timeout);
        }
        Ok(())
    }

    /// The pattern byte at `i`, or 0 past its end (C reads the terminator).
    #[inline]
    fn p(&self, i: usize) -> u8 {
        self.pat.get(i).copied().unwrap_or(0)
    }

    fn class_end(&self, p: usize) -> M<usize> {
        let c = self.pat[p];
        let mut p = p + 1;
        match c {
            b'%' => {
                if p == self.pat.len() {
                    return err("malformed pattern (ends with '%')");
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.p(p) == b'^' {
                    p += 1;
                }
                loop {
                    if p >= self.pat.len() {
                        return err("malformed pattern (missing ']')");
                    }
                    let c = self.pat[p];
                    p += 1;
                    if c == b'%' && p < self.pat.len() {
                        p += 1;
                    }
                    if self.p(p) == b']' && p < self.pat.len() {
                        break;
                    }
                }
                Ok(p + 1)
            }
            _ => Ok(p),
        }
    }

    fn match_bracket_class(&self, c: u8, p: usize, ec: usize) -> bool {
        let mut sig = true;
        let mut p = p;
        if self.p(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.pat[p] == b'%' {
                p += 1;
                if match_class(c, self.p(p)) {
                    return sig;
                }
            } else if self.p(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.pat[p - 2] <= c && c <= self.pat[p] {
                    return sig;
                }
            } else if self.pat[p] == c {
                return sig;
            }
        }
        !sig
    }

    #[inline]
    fn single_match(&self, s: usize, p: usize, ep: usize) -> bool {
        let Some(&c) = self.src.get(s) else {
            return false;
        };
        match self.pat[p] {
            b'.' => true,
            b'%' => match_class(c, self.p(p + 1)),
            b'[' => self.match_bracket_class(c, p, ep - 1),
            x => x == c,
        }
    }

    fn match_balance(&mut self, s: usize, p: usize) -> M<Option<usize>> {
        if p + 1 >= self.pat.len() {
            return err("malformed pattern (missing arguments to '%b')");
        }
        if self.src.get(s).copied().unwrap_or(0) != self.pat[p] {
            return Ok(None);
        }
        let (b, e) = (self.pat[p], self.pat[p + 1]);
        let mut cont = 1usize;
        let mut i = s + 1;
        while i < self.src.len() {
            let c = self.src[i];
            if c == e {
                cont -= 1;
                if cont == 0 {
                    self.spend(i - s)?;
                    return Ok(Some(i + 1));
                }
            } else if c == b {
                cont += 1;
            }
            i += 1;
        }
        self.spend(i.saturating_sub(s))?;
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> M<Option<usize>> {
        let mut i = 0usize;
        while self.single_match(s + i, p, ep) {
            i += 1;
        }
        self.spend(i)?;
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> M<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            }
            if self.single_match(s, p, ep) {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> M<Option<usize>> {
        let level = self.level;
        if level >= MAX_CAPTURES {
            return err("too many captures");
        }
        self.capture[level] = (s, what);
        self.level = level + 1;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.level -= 1;
        }
        Ok(r)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> M<Option<usize>> {
        let Some(l) = (0..self.level)
            .rev()
            .find(|&l| self.capture[l].1 == CAP_UNFINISHED)
        else {
            return err("invalid pattern capture");
        };
        self.capture[l].1 = (s - self.capture[l].0) as isize;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.capture[l].1 = CAP_UNFINISHED;
        }
        Ok(r)
    }

    fn match_capture(&mut self, s: usize, digit: u8) -> M<Option<usize>> {
        let l = digit as isize - b'1' as isize;
        if l < 0 || l as usize >= self.level || self.capture[l as usize].1 == CAP_UNFINISHED {
            return err(format!("invalid capture index %{}", l + 1));
        }
        let (init, len) = self.capture[l as usize];
        if len < 0 {
            return Ok(None); // a position capture never matches as text
        }
        let len = len as usize;
        self.spend(len)?;
        if self.src.len() - s >= len && self.src[init..init + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    fn do_match(&mut self, s: usize, p: usize) -> M<Option<usize>> {
        if self.depth == 0 {
            return err("pattern too complex");
        }
        self.depth -= 1;
        let r = self.match_here(s, p)?;
        self.depth += 1;
        Ok(r)
    }

    fn match_here(&mut self, mut s: usize, mut p: usize) -> M<Option<usize>> {
        loop {
            self.tick()?;
            if p >= self.pat.len() {
                return Ok(Some(s));
            }
            match self.pat[p] {
                b'(' => {
                    return if self.p(p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == self.pat.len() => {
                    return Ok((s == self.src.len()).then_some(s));
                }
                b'%' => match self.p(p + 1) {
                    b'b' if p + 1 < self.pat.len() => match self.match_balance(s, p + 2)? {
                        Some(ns) => {
                            s = ns;
                            p += 4;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    b'f' if p + 1 < self.pat.len() => {
                        p += 2;
                        if self.p(p) != b'[' || p >= self.pat.len() {
                            return err("missing '[' after '%f' in pattern");
                        }
                        let ep = self.class_end(p)?;
                        let previous = if s == 0 { 0 } else { self.src[s - 1] };
                        let current = self.src.get(s).copied().unwrap_or(0);
                        if !self.match_bracket_class(previous, p, ep - 1)
                            && self.match_bracket_class(current, p, ep - 1)
                        {
                            p = ep;
                            continue;
                        }
                        return Ok(None);
                    }
                    d @ b'0'..=b'9' if p + 1 < self.pat.len() => match self.match_capture(s, d)? {
                        Some(ns) => {
                            s = ns;
                            p += 2;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    _ => {}
                },
                _ => {}
            }
            // A single character class with an optional suffix.
            let ep = self.class_end(p)?;
            let suffix = self.p(ep);
            if !self.single_match(s, p, ep) {
                if matches!(suffix, b'*' | b'?' | b'-') && ep < self.pat.len() {
                    p = ep + 1;
                    continue;
                }
                return Ok(None);
            }
            match if ep < self.pat.len() { suffix } else { 0 } {
                b'?' => {
                    if let Some(r) = self.do_match(s + 1, ep + 1)? {
                        return Ok(Some(r));
                    }
                    p = ep + 1;
                }
                b'+' => return self.max_expand(s + 1, p, ep),
                b'*' => return self.max_expand(s, p, ep),
                b'-' => return self.min_expand(s, p, ep),
                _ => {
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    /// The `i`-th capture of the match `s..e`; with no captures, capture 0 is
    /// the whole match.
    pub fn one_capture(&self, i: usize, s: usize, e: usize) -> M<Capture> {
        if i >= self.level {
            if i != 0 {
                return err(format!("invalid capture index %{}", i + 1));
            }
            return Ok(Capture::Text(s, e - s));
        }
        let (init, len) = self.capture[i];
        match len {
            CAP_UNFINISHED => err("unfinished capture"),
            CAP_POSITION => Ok(Capture::Position(init + 1)),
            len => Ok(Capture::Text(init, len as usize)),
        }
    }

    /// All captures of the match; the whole match if there are none and
    /// `whole` is set (as `match`, `gmatch` and `gsub` want, and `find` not).
    pub fn captures(&self, s: usize, e: usize, whole: bool) -> M<Vec<Capture>> {
        let n = if self.level == 0 && whole {
            1
        } else {
            self.level
        };
        (0..n).map(|i| self.one_capture(i, s, e)).collect()
    }

    /// Appends the replacement string `repl` for the match `s..e` (`%0` to
    /// `%9` and `%%`), as `add_s` does.
    pub fn add_replacement(&self, out: &mut Vec<u8>, repl: &[u8], s: usize, e: usize) -> M<()> {
        let mut rest = repl;
        while let Some(at) = memchr::memchr(b'%', rest) {
            out.extend_from_slice(&rest[..at]);
            match rest.get(at + 1).copied() {
                Some(b'%') => out.push(b'%'),
                Some(b'0') => out.extend_from_slice(&self.src[s..e]),
                Some(d @ b'1'..=b'9') => match self.one_capture((d - b'1') as usize, s, e)? {
                    Capture::Text(init, len) => out.extend_from_slice(&self.src[init..init + len]),
                    Capture::Position(pos) => out.extend_from_slice(pos.to_string().as_bytes()),
                },
                _ => return err("invalid use of '%' in replacement string"),
            }
            rest = &rest[at + 2..];
        }
        out.extend_from_slice(rest);
        Ok(())
    }
}

/// `match_class` of `lstrlib.c`, with the classes of the C locale.
fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => c == b' ' || (9..=13).contains(&c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_lowercase() { res } else { !res }
}

/// True when the pattern has none of `^$*+?.([%-`: it is plain text.
pub fn no_specials(pat: &[u8]) -> bool {
    !pat.iter().any(|c| b"^$*+?.([%-".contains(c))
}

/// A plain search, linear in the worst case.
pub fn plain_find(src: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::find(src, needle)
}

/// `posrelatI`: a 1-based start position, negative counting from the end.
pub fn start_position(pos: i64, len: usize) -> usize {
    if pos > 0 {
        usize::try_from(pos).unwrap_or(usize::MAX)
    } else if pos == 0 || pos < -(len as i64) {
        1
    } else {
        len - (pos.unsigned_abs() as usize) + 1
    }
}

/// What `find` or `match` found.
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub captures: Vec<Capture>,
}

/// `str_find_aux`: `string.find` (with `find`) or `string.match`. `init` is
/// the 0-based offset to start at.
pub fn find(
    src: &[u8],
    pat: &[u8],
    init: usize,
    plain: bool,
    find: bool,
    deadline: Instant,
) -> M<Option<Found>> {
    if init > src.len() {
        return Ok(None);
    }
    if find && (plain || no_specials(pat)) {
        return Ok(plain_find(&src[init..], pat).map(|at| Found {
            start: init + at,
            end: init + at + pat.len(),
            captures: Vec::new(),
        }));
    }
    let anchor = pat.first() == Some(&b'^');
    let pat = if anchor { &pat[1..] } else { pat };
    let mut m = Matcher::new(src, pat, deadline);
    let mut s1 = init;
    loop {
        if let Some(e) = m.match_at(s1)? {
            let captures = m.captures(s1, e, !find)?;
            return Ok(Some(Found {
                start: s1,
                end: e,
                captures,
            }));
        }
        if s1 >= src.len() || anchor {
            return Ok(None);
        }
        s1 += 1;
    }
}

/// One step of `gmatch` and of `gsub`: the first match that starts at `from`
/// or later (only at `from` with `here_only`) and does not end at
/// `last_match`.
/// With `first_only` only the first capture is read, as `gsub` does for a
/// table replacement.
pub fn step(
    src: &[u8],
    pat: &[u8],
    from: usize,
    last_match: Option<usize>,
    here_only: bool,
    first_only: bool,
    deadline: Instant,
) -> M<Option<Found>> {
    let mut m = Matcher::new(src, pat, deadline);
    let mut q = from;
    while q <= src.len() {
        if let Some(e) = m.match_at(q)?
            && Some(e) != last_match
        {
            let captures = if first_only {
                vec![m.one_capture(0, q, e)?]
            } else {
                m.captures(q, e, true)?
            };
            return Ok(Some(Found {
                start: q,
                end: e,
                captures,
            }));
        }
        if here_only {
            break;
        }
        q += 1;
    }
    Ok(None)
}

/// `str_gsub` with a string replacement. Returns `None` when nothing was
/// replaced (the caller then returns the subject itself) and the count.
/// `max_out` bounds the result, which is built outside Lua's own memory.
pub fn gsub(
    src: &[u8],
    pat: &[u8],
    repl: &[u8],
    max_n: i64,
    max_out: usize,
    deadline: Instant,
) -> M<(Option<Vec<u8>>, i64)> {
    let anchor = pat.first() == Some(&b'^');
    let pat = if anchor { &pat[1..] } else { pat };
    let mut m = Matcher::new(src, pat, deadline);
    let mut out = Vec::new();
    let (mut s, mut last, mut n) = (0usize, None, 0i64);
    while n < max_n {
        match m.match_at(s)? {
            Some(e) if Some(e) != last => {
                n += 1;
                m.add_replacement(&mut out, repl, s, e)?;
                s = e;
                last = Some(e);
            }
            _ if s < src.len() => {
                out.push(src[s]);
                s += 1;
            }
            _ => break,
        }
        if out.len() > max_out {
            return err("not enough memory");
        }
        if anchor {
            break;
        }
    }
    if n == 0 {
        return Ok((None, 0));
    }
    if out.len() + (src.len() - s) > max_out {
        return err("not enough memory");
    }
    out.extend_from_slice(&src[s..]);
    Ok((Some(out), n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn finds_and_captures() {
        let f = find(b"hello world", b"(o) (w)", 0, false, true, far())
            .unwrap()
            .unwrap();
        assert_eq!((f.start, f.end), (4, 7));
        assert_eq!(f.captures, vec![Capture::Text(4, 1), Capture::Text(6, 1)]);
        assert!(
            find(b"abc", b"^b", 0, false, true, far())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            gsub(b"a,b,,c", b",", b";", 10, 1 << 20, far()).unwrap(),
            (Some(b"a;b;;c".to_vec()), 3)
        );
    }

    #[test]
    fn a_pathological_pattern_ends_at_the_deadline() {
        let subject = vec![b'a'; 200_000];
        let started = Instant::now();
        let r = find(
            &subject,
            b".-.-.-.-x",
            0,
            false,
            true,
            started + Duration::from_millis(50),
        );
        assert_eq!(r.err(), Some(Stop::Timeout));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
    }
}

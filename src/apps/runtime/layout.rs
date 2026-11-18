//! How a row or a column divides its space among its parts.
//!
//! Ratatui's own `Layout` is not used for an app's screen. It hands the
//! sizes to a constraint solver whose time grows much faster than the
//! number of parts: on the machine this was written on, 32 parts that each
//! "fill" took 0.1 s in a release build, 48 took minutes and then failed
//! inside the solver with a panic. A script chooses how many parts a row
//! has, and the layout runs on bzz's own thread.
//!
//! This is the same division done directly, in one pass over the parts. It
//! follows the order of strengths that Ratatui gives its constraints, so
//! the screens of the contract look as they do under Ratatui:
//!
//! - a part with a size in cells, a percentage or a ratio wants exactly
//!   that; `max:n` wants `n` and never more; `min:n` wants `n` and never
//!   less;
//! - what is left goes to the parts that fill, by their shares;
//! - with no part that fills, what is left goes to the last part (`legacy`,
//!   the default) or between and around the parts (`flex`);
//! - when the parts want more than there is, the last parts give way
//!   first: `max` before a ratio, a ratio before a percentage, a percentage
//!   before cells, and `min` last.
//!
//! No step can fail, and none depends on the values a script gives.

use ratatui::layout::{Constraint, Direction, Flex, Rect};

/// Which of a part's wishes gives way first: lower gives way sooner.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Max,
    Ratio,
    Percentage,
    Length,
    Min,
    Fill,
}

struct Part {
    class: Class,
    /// What the part wants, in cells.
    size: f64,
    /// It never takes less than this while there is room.
    least: f64,
    /// Its share of what is left over; zero for a part that does not fill.
    share: f64,
}

/// Divides `area` along `direction` among `constraints`, with `spacing`
/// cells between two parts. One rectangle for each constraint, inside
/// `area`, in order.
pub fn split(
    area: Rect,
    direction: Direction,
    constraints: &[Constraint],
    spacing: u16,
    flex: Flex,
) -> Vec<Rect> {
    let total = match direction {
        Direction::Horizontal => area.width,
        Direction::Vertical => area.height,
    };
    spans(total, constraints, spacing, flex)
        .into_iter()
        .map(|(offset, size)| match direction {
            Direction::Horizontal => Rect::new(area.x + offset, area.y, size, area.height),
            Direction::Vertical => Rect::new(area.x, area.y + offset, area.width, size),
        })
        .collect()
}

/// The offset and the size of each part along a line of `total` cells.
pub fn spans(total: u16, constraints: &[Constraint], spacing: u16, flex: Flex) -> Vec<(u16, u16)> {
    let n = constraints.len();
    if n == 0 {
        return Vec::new();
    }
    let whole = f64::from(total);
    let legacy = flex == Flex::Legacy;
    // The space between parts gives way before the parts do.
    let gaps = (n - 1) as f64;
    let gap = if gaps > 0.0 {
        f64::from(spacing).min(whole / gaps)
    } else {
        0.0
    };
    let room = (whole - gap * gaps).max(0.0);

    let mut parts: Vec<Part> = constraints
        .iter()
        .map(|constraint| match *constraint {
            Constraint::Length(cells) => Part {
                class: Class::Length,
                size: f64::from(cells),
                least: 0.0,
                share: 0.0,
            },
            Constraint::Percentage(percent) => Part {
                class: Class::Percentage,
                size: whole * f64::from(percent.min(100)) / 100.0,
                least: 0.0,
                share: 0.0,
            },
            Constraint::Ratio(a, b) => Part {
                class: Class::Ratio,
                size: (whole * f64::from(a) / f64::from(b.max(1))).min(whole),
                least: 0.0,
                share: 0.0,
            },
            Constraint::Max(cells) => Part {
                class: Class::Max,
                size: f64::from(cells),
                least: 0.0,
                share: 0.0,
            },
            // Under `legacy` a `min` is its size and no more; otherwise it
            // fills like `fill`, from its size up.
            Constraint::Min(cells) => Part {
                class: Class::Min,
                size: f64::from(cells),
                least: f64::from(cells),
                share: if legacy { 0.0 } else { 1.0 },
            },
            Constraint::Fill(share) => Part {
                class: Class::Fill,
                size: 0.0,
                least: 0.0,
                share: f64::from(share).max(1e-6),
            },
        })
        .collect();

    let wanted: f64 = parts.iter().map(|p| p.size).sum();
    let mut left = room - wanted;
    if left < 0.0 {
        shrink(&mut parts, -left);
        left = 0.0;
    }
    if left > 0.0 && parts.iter().any(|p| p.share > 0.0) {
        grow(&mut parts, left);
        left = 0.0;
    }
    if left > 0.0 && legacy {
        // The last part of the kind that gives way soonest takes it. A
        // `max` gives way only when nothing else is there.
        let order = [
            Class::Min,
            Class::Ratio,
            Class::Percentage,
            Class::Length,
            Class::Max,
        ];
        let taker = order
            .iter()
            .find_map(|class| parts.iter().rposition(|p| p.class == *class));
        if let Some(i) = taker {
            parts[i].size += left;
        }
        left = 0.0;
    }

    // What is still left is space: before, between and after the parts.
    let (lead, between) = match flex {
        Flex::Legacy | Flex::Start => (0.0, gap),
        Flex::End => (left, gap),
        Flex::Center => (left / 2.0, gap),
        Flex::SpaceBetween if n > 1 => (0.0, gap + left / gaps),
        Flex::SpaceBetween => (0.0, gap),
        // Between two parts twice the space that is before the first.
        Flex::SpaceAround => {
            let free = left + gap * gaps;
            let unit = free / (2.0 * n as f64);
            (unit, 2.0 * unit)
        }
        Flex::SpaceEvenly => {
            let free = left + gap * gaps;
            let unit = free / (n as f64 + 1.0);
            (unit, unit)
        }
    };

    // Edges are rounded, not sizes: the parts then add up exactly.
    let edge = |at: f64| at.round().clamp(0.0, whole) as u16;
    let mut at = lead;
    parts
        .iter()
        .map(|part| {
            let (start, end) = (edge(at), edge(at + part.size));
            at += part.size + between;
            (start, end.saturating_sub(start))
        })
        .collect()
}

/// Takes `over` cells away: from the kind that gives way soonest first,
/// and within a kind from the last part back. A `min` keeps its size until
/// nothing else is left to take from.
fn shrink(parts: &mut [Part], mut over: f64) {
    let order = [
        Class::Max,
        Class::Ratio,
        Class::Percentage,
        Class::Length,
        Class::Min,
    ];
    for class in order {
        for part in parts.iter_mut().rev().filter(|p| p.class == class) {
            if over <= 0.0 {
                return;
            }
            let taken = part.size.min(over);
            part.size -= taken;
            part.least = part.least.min(part.size);
            over -= taken;
        }
    }
}

/// Hands `spare` cells to the parts that fill, by their shares. A `min`
/// that fills counts from zero: it gets its share of everything the
/// filling parts have together, and never less than its size.
fn grow(parts: &mut [Part], spare: f64) {
    // Everything the filling parts have to divide: the spare, and what
    // those of them that are a `min` already hold.
    let mut pool: f64 = spare
        + parts
            .iter()
            .filter(|p| p.share > 0.0)
            .map(|p| p.size)
            .sum::<f64>();
    let mut open: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].share > 0.0).collect();
    // A part whose share would be under its least size keeps that size and
    // leaves the division. Taken in the order of how much each needs for
    // its share, the first that is not short ends the search: one pass.
    open.sort_by(|a, b| {
        let need = |i: &usize| parts[*i].least / parts[*i].share;
        need(b).total_cmp(&need(a))
    });
    let mut shares: f64 = open.iter().map(|i| parts[*i].share).sum();
    let mut settled = 0;
    for i in &open {
        if pool * parts[*i].share / shares >= parts[*i].least {
            break;
        }
        parts[*i].size = parts[*i].least;
        pool -= parts[*i].least;
        shares -= parts[*i].share;
        settled += 1;
    }
    for i in &open[settled..] {
        parts[*i].size = (pool * parts[*i].share / shares).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Layout;

    use Constraint::{Fill, Length, Max, Min, Percentage, Ratio};

    fn sizes(total: u16, constraints: &[Constraint], spacing: u16, flex: Flex) -> Vec<u16> {
        spans(total, constraints, spacing, flex)
            .into_iter()
            .map(|(_, size)| size)
            .collect()
    }

    /// What Ratatui's own layout gives. Only for a few parts: that is the
    /// solver this module exists to keep away from a script.
    fn ratatui(
        total: u16,
        constraints: &[Constraint],
        spacing: u16,
        flex: Flex,
    ) -> Vec<(u16, u16)> {
        Layout::new(Direction::Horizontal, constraints.to_vec())
            .spacing(spacing)
            .flex(flex)
            .split(Rect::new(0, 0, total, 1))
            .iter()
            .map(|r| (r.x, r.width))
            .collect()
    }

    #[test]
    fn the_division_is_what_the_contract_says() {
        let legacy = Flex::Legacy;
        // Cells are cells; what fills takes the rest, by its share.
        assert_eq!(sizes(40, &[Length(8), Fill(1)], 0, legacy), [8, 32]);
        assert_eq!(
            sizes(40, &[Fill(1), Fill(1), Fill(1)], 0, legacy),
            [13, 14, 13]
        );
        assert_eq!(sizes(40, &[Fill(1), Fill(3)], 0, legacy), [10, 30]);
        assert_eq!(sizes(41, &[Fill(1), Fill(1)], 1, legacy), [20, 20]);
        assert_eq!(
            sizes(40, &[Percentage(25), Ratio(1, 4), Fill(1)], 0, legacy),
            [10, 10, 20]
        );
        // With nothing that fills, the last part takes what is left.
        assert_eq!(sizes(40, &[Length(5), Length(5)], 0, legacy), [5, 35]);
        assert_eq!(sizes(40, &[Min(5), Length(5)], 0, legacy), [35, 5]);
        assert_eq!(sizes(40, &[Max(5), Length(5)], 0, legacy), [5, 35]);
        // Otherwise it is space, where `flex` says.
        let at = |flex| spans(40, &[Length(10), Length(10)], 0, flex);
        assert_eq!(at(Flex::Start), [(0, 10), (10, 10)]);
        assert_eq!(at(Flex::End), [(20, 10), (30, 10)]);
        assert_eq!(at(Flex::Center), [(10, 10), (20, 10)]);
        assert_eq!(at(Flex::SpaceBetween), [(0, 10), (30, 10)]);
        assert_eq!(at(Flex::SpaceAround), [(5, 10), (25, 10)]);
        assert_eq!(at(Flex::SpaceEvenly), [(7, 10), (23, 10)]);
        // A `min` fills from its size up, but not under `legacy`.
        assert_eq!(sizes(40, &[Min(30), Fill(1)], 0, Flex::Start), [30, 10]);
        assert_eq!(sizes(40, &[Min(5), Fill(1)], 0, Flex::Start), [20, 20]);
        assert_eq!(sizes(40, &[Min(5), Fill(1)], 0, legacy), [5, 35]);
        // More is wanted than there is: the last parts give way, a `min`
        // last of all, and the space between parts before any part.
        assert_eq!(sizes(10, &[Length(8), Length(8)], 0, legacy), [8, 2]);
        assert_eq!(sizes(10, &[Length(8), Min(8)], 0, legacy), [2, 8]);
        assert_eq!(sizes(10, &[Percentage(80), Length(8)], 0, legacy), [2, 8]);
        assert_eq!(
            sizes(3, &[Fill(1), Fill(1), Fill(1), Fill(1)], 5, legacy),
            [0, 0, 0, 0]
        );
        assert_eq!(sizes(0, &[Length(8), Fill(1)], 1, legacy), [0, 0]);
        assert_eq!(sizes(40, &[], 1, legacy), [] as [u16; 0]);
    }

    #[test]
    fn it_agrees_with_ratatui_where_ratatui_is_usable() {
        // The layouts apps are written with: a few parts, and room for
        // them. Every width from nothing to wide.
        let shapes: &[&[Constraint]] = &[
            &[Fill(1)],
            &[Length(7), Fill(1)],
            &[Fill(1), Length(1)],
            &[Length(1), Fill(1), Length(1)],
            &[Fill(1), Fill(1)],
            &[Fill(1), Fill(1), Fill(1)],
            &[Fill(2), Fill(1)],
            &[Length(4), Length(3), Fill(1), Length(1)],
            &[Percentage(50), Fill(1)],
            &[Percentage(30), Percentage(70)],
            &[Ratio(1, 3), Fill(1)],
            &[Length(10), Length(10)],
            &[Length(3), Length(3), Length(3)],
            &[Min(10), Length(5)],
            &[Length(5), Min(10)],
            &[Max(10), Fill(1)],
            &[Length(8), Max(10)],
        ];
        // What the parts want before anything is divided.
        let wanted = |shape: &[Constraint], spacing: u16, total: u16| -> f64 {
            let whole = f64::from(total);
            let sizes: f64 = shape
                .iter()
                .map(|c| match *c {
                    Length(n) | Min(n) | Max(n) => f64::from(n),
                    Percentage(p) => whole * f64::from(p) / 100.0,
                    Ratio(a, b) => whole * f64::from(a) / f64::from(b),
                    Fill(_) => 0.0,
                })
                .sum();
            sizes + f64::from(spacing) * (shape.len() - 1) as f64
        };
        let (mut roomy, mut tight, mut tight_same) = (0, 0, 0);
        let mut different = Vec::new();
        for shape in shapes {
            for spacing in [0u16, 1] {
                for flex in [
                    Flex::Legacy,
                    Flex::Start,
                    Flex::End,
                    Flex::Center,
                    Flex::SpaceBetween,
                ] {
                    for total in 0..=120u16 {
                        let ours = spans(total, shape, spacing, flex);
                        let theirs = ratatui(total, shape, spacing, flex);
                        if wanted(shape, spacing, total) > f64::from(total) {
                            // Not enough room: Ratatui's answer is whatever
                            // its solver settles on (two parts of 4 and 3
                            // cells in 4 get 2 and 2). Ours is the rule.
                            tight += 1;
                            tight_same += usize::from(ours == theirs);
                        } else {
                            roomy += 1;
                            if ours != theirs && different.len() < 12 {
                                different.push(format!(
                                    "{shape:?} spacing {spacing} {flex:?} in {total}: ours {ours:?}, Ratatui's {theirs:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }
        println!(
            "with room for the parts: {roomy} divisions, all as Ratatui's; without: {tight_same} of {tight} as Ratatui's"
        );
        assert!(
            different.is_empty(),
            "divisions with room for their parts differ from Ratatui's:\n{}",
            different.join("\n")
        );
    }

    #[test]
    fn many_parts_cost_no_more_than_their_number() {
        // What Ratatui's solver took minutes for, and then panicked on.
        let started = std::time::Instant::now();
        for total in [0u16, 1, 40, 300, u16::MAX] {
            for constraint in [
                Fill(1),
                Length(1),
                Min(3),
                Max(3),
                Percentage(10),
                Ratio(1, 7),
            ] {
                for flex in [
                    Flex::Legacy,
                    Flex::Start,
                    Flex::SpaceAround,
                    Flex::SpaceEvenly,
                ] {
                    let parts = spans(total, &vec![constraint; 10_000], 1, flex);
                    assert_eq!(parts.len(), 10_000);
                    // Inside the line, in order, never overlapping.
                    let mut end = 0u32;
                    for (offset, size) in parts {
                        assert!(u32::from(offset) >= end || size == 0);
                        end = end.max(u32::from(offset) + u32::from(size));
                        assert!(end <= u32::from(total));
                    }
                }
            }
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    }
}

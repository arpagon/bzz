use ratatui::layout::Rect;

/// Semantic target attached to a rectangle in one completed render generation.
///
/// The app replaces the complete map after every render, so terminal coordinates
/// are never interpreted against stale geometry after a resize or mode change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HitTarget {
    Community(usize),
    ChannelPane,
    Channel(usize),
    Timeline,
    TimelineMessage(String),
    Thread,
    ThreadMessage(String),
    Composer,
    MentionCandidate(usize),
    FinderChannel(String),
    Theme(String),
    Reaction(usize),
    InboxItem(String),
    SearchResult(String),
    DmCandidate(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HitRegion {
    pub area: Rect,
    pub target: HitTarget,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HitMap {
    generation: u64,
    regions: Vec<HitRegion>,
}

impl HitMap {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            regions: Vec::new(),
        }
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Later regions have visual priority over earlier enclosing regions.
    pub fn push(&mut self, area: Rect, target: HitTarget) {
        if !area.is_empty() {
            self.regions.push(HitRegion { area, target });
        }
    }

    pub fn hit(&self, column: u16, row: u16) -> Option<&HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|region| contains(region.area, column, row))
            .map(|region| &region.target)
    }

    pub fn area_of(&self, target: &HitTarget) -> Option<Rect> {
        self.regions
            .iter()
            .rev()
            .find(|region| &region.target == target)
            .map(|region| region.area)
    }
}

pub fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

#[cfg(test)]
mod tests {
    use super::{HitMap, HitTarget};
    use ratatui::layout::Rect;

    #[test]
    fn later_semantic_regions_override_the_enclosing_pane() {
        let mut map = HitMap::new(7);
        map.push(Rect::new(10, 3, 30, 12), HitTarget::Timeline);
        map.push(
            Rect::new(11, 5, 28, 3),
            HitTarget::TimelineMessage("event-1".into()),
        );

        assert_eq!(map.generation(), 7);
        assert_eq!(
            map.hit(12, 6),
            Some(&HitTarget::TimelineMessage("event-1".into()))
        );
        assert_eq!(map.hit(12, 10), Some(&HitTarget::Timeline));
        assert_eq!(map.hit(40, 10), None);
    }

    #[test]
    fn empty_regions_and_outside_coordinates_are_noops() {
        let mut map = HitMap::new(1);
        map.push(Rect::new(2, 2, 0, 4), HitTarget::ChannelPane);
        assert_eq!(map.hit(2, 2), None);
    }
}

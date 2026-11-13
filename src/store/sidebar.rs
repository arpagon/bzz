//! The synced sidebar documents' current heads ([`crate::protocol::sidebar`]).

use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    domain::{SidebarLayout, SidebarSection},
    error::{Error, Result},
    protocol::{
        sidebar::{Lane, newer},
        sidebar_doc::{self, Tree},
    },
    store::{Store, events::u64_to_i64},
};

impl Store {
    /// Keep `tree` as the head of `lane` when it is newer than the stored one.
    pub fn record_sidebar_document(
        &mut self,
        community_id: Uuid,
        pubkey: &str,
        lane: Lane,
        event_id: &str,
        created_at: u64,
        tree: &Tree,
    ) -> Result<bool> {
        let current = self.sidebar_head(community_id, pubkey, lane)?;
        if !newer(
            (created_at, event_id),
            current.as_ref().map(|(at, id, _)| (*at, id.as_str())),
        ) {
            return Ok(false);
        }
        let tree_json =
            serde_json::to_string(tree).map_err(|error| Error::Serialization(error.to_string()))?;
        self.connection.execute(
            "INSERT INTO sidebar_documents(community_id,pubkey,lane,event_id,created_at,tree_json)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(community_id,pubkey,lane) DO UPDATE SET event_id=excluded.event_id,
               created_at=excluded.created_at,tree_json=excluded.tree_json",
            params![
                community_id.to_string(),
                pubkey,
                lane.d_tag(),
                event_id,
                u64_to_i64(created_at)?,
                tree_json
            ],
        )?;
        Ok(true)
    }

    /// The stored head of `lane`: `(created_at, event_id, tree)`.
    pub fn sidebar_head(
        &self,
        community_id: Uuid,
        pubkey: &str,
        lane: Lane,
    ) -> Result<Option<(u64, String, Tree)>> {
        let row = self
            .connection
            .query_row(
                "SELECT created_at,event_id,tree_json FROM sidebar_documents
                 WHERE community_id=?1 AND pubkey=?2 AND lane=?3",
                params![community_id.to_string(), pubkey, lane.d_tag()],
                |row| {
                    Ok((
                        u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(at, id, json)| {
            serde_json::from_str::<Tree>(&json)
                .map(|tree| (at, id, tree))
                .map_err(|error| Error::Serialization(error.to_string()))
        })
        .transpose()
    }

    /// The live sections, assignments and sort modes to show.
    pub fn sidebar_layout(&self, community_id: Uuid, pubkey: &str) -> Result<SidebarLayout> {
        let mut layout = SidebarLayout::default();
        if let Some((_, _, tree)) = self.sidebar_head(community_id, pubkey, Lane::Sections)? {
            let view = sidebar_doc::project(&tree);
            if view.within_limits() {
                layout.sections = view
                    .sections
                    .into_iter()
                    .map(|section| SidebarSection {
                        id: section.id,
                        name: section.name,
                        icon: section.icon,
                    })
                    .collect();
                layout.assignments = view
                    .assignments
                    .into_iter()
                    .filter_map(|(channel, section)| {
                        Uuid::parse_str(&channel)
                            .ok()
                            .map(|channel| (channel, section))
                    })
                    .collect();
            }
        }
        if let Some((_, _, tree)) = self.sidebar_head(community_id, pubkey, Lane::Sort)? {
            let groups = sidebar_doc::sort_groups(&tree);
            if groups.len() <= sidebar_doc::MAX_SORT_GROUPS {
                layout.sort = groups;
            }
        }
        Ok(layout)
    }
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use rusqlite::params;
    use serde_json::json;
    use uuid::Uuid;

    use crate::{
        config::{CommunityConfig, Config, IdentityConfig, KeyBackend},
        protocol::{sidebar::Lane, sidebar_doc},
        store::Store,
    };

    fn store(viewer: &str) -> (Store, Uuid) {
        let community = Uuid::new_v4();
        let identity = Uuid::new_v4();
        let config = Config {
            identities: vec![IdentityConfig {
                id: identity,
                label: "m4".into(),
                pubkey: viewer.into(),
                backend: KeyBackend::Keychain,
                key_ref: "m4".into(),
            }],
            communities: vec![CommunityConfig {
                id: community,
                label: "m4".into(),
                relay_url: "wss://buzz.example/".into(),
                identity_id: identity,
                allow_insecure_localhost: false,
                theme: None,
                identity_adapter: None,
            }],
            default_community: Some(community),
            ..Config::default()
        };
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        (store, community)
    }

    /// v0.14 M4: the newest head wins (ties on the smaller id); the layout
    /// projects live sections, assignments and sort modes.
    #[test]
    fn newest_sidebar_head_wins_and_projects_the_layout() {
        let viewer = "a".repeat(64);
        let (mut store, community) = store(&viewer);
        let channel = Uuid::new_v4();
        let tree = |name: &str| {
            let mut tree = sidebar_doc::Tree::new();
            let stamp = sidebar_doc::Stamp {
                now_ms: 1,
                dev: "0123456789abcdef".into(),
            };
            let mut edit = sidebar_doc::Edit::new(&mut tree, &stamp);
            edit.create_section("s1", name, Some("🚀"));
            edit.set(&["a", &channel.to_string()], json!("s1"));
            tree
        };
        let (old, new) = ("1".repeat(64), "2".repeat(64));
        assert!(
            store
                .record_sidebar_document(community, &viewer, Lane::Sections, &new, 20, &tree("New"))
                .unwrap()
        );
        assert!(
            !store
                .record_sidebar_document(community, &viewer, Lane::Sections, &old, 10, &tree("Old"))
                .unwrap()
        );
        assert!(
            store
                .record_sidebar_document(community, &viewer, Lane::Sections, &old, 20, &tree("Tie"))
                .unwrap()
        );
        let mut sort = sidebar_doc::Tree::new();
        sidebar_doc::Edit::new(
            &mut sort,
            &sidebar_doc::Stamp {
                now_ms: 1,
                dev: "0123456789abcdef".into(),
            },
        )
        .set(&["g", "dms"], json!("alpha"));
        store
            .record_sidebar_document(community, &viewer, Lane::Sort, &new, 5, &sort)
            .unwrap();
        let layout = store.sidebar_layout(community, &viewer).unwrap();
        assert_eq!(
            layout.sections[0].name, "Tie",
            "same second: smaller id wins"
        );
        assert_eq!(layout.sections[0].icon.as_deref(), Some("🚀"));
        assert_eq!(layout.assignments[&channel], "s1");
        assert_eq!(layout.sort["dms"], "alpha");
        assert!(
            store
                .sidebar_layout(community, &"b".repeat(64))
                .unwrap()
                .sections
                .is_empty()
        );
    }

    /// Re-ingesting the current metadata repairs channels earlier bzz stored
    /// as public because it misread the bare `["private"]` tag.
    #[test]
    fn current_metadata_repairs_a_misread_private_channel() {
        let (mut store, community) = store(&"a".repeat(64));
        let relay = Keys::generate();
        store
            .pin_relay_pubkey(community, &relay.public_key().to_hex())
            .unwrap();
        let id = Uuid::new_v4();
        let metadata = EventBuilder::new(Kind::Custom(39_000), "")
            .tags([
                Tag::parse(["d", &id.to_string()]).unwrap(),
                Tag::parse(["name", "plans"]).unwrap(),
                Tag::parse(["private"]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &metadata).unwrap();
        store
            .connection
            .execute(
                "UPDATE channels SET visibility='public' WHERE channel_id=?1",
                params![id.to_string()],
            )
            .unwrap();
        assert!(store.apply_event(community, &metadata).unwrap(), "repaired");
        let visibility: String = store
            .connection
            .query_row(
                "SELECT visibility FROM channels WHERE channel_id=?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(visibility, "private");
    }
}

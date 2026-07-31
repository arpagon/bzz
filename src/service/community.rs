use crate::{
    config::Config,
    error::{Error, Result},
    store::writer::StoreHandle,
};
use uuid::Uuid;

pub async fn purge(config: &mut Config, community_id: Uuid, store: &StoreHandle) -> Result<()> {
    if !config.remove_community(community_id) {
        return Err(Error::Config(format!(
            "community {community_id} does not exist"
        )));
    }
    store
        .call(move |store| store.purge_community(community_id))
        .await
}

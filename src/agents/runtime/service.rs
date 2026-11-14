//! Local agents on the real relay: each with its own unsealed key, its own
//! authenticated connection and its own HTTP client, as the v0.13 broker
//! gives a hosted agent. Used by the TUI while the owner's identity is
//! unlocked, and by `bzz agent run` without it.

use std::sync::Arc;

use uuid::Uuid;

use crate::{
    agents::runtime::{
        Limits,
        agent::AgentSpec,
        host::{AgentHost, HostOptions},
        relay::RelayPort,
    },
    auth::signer::SignerHandle,
    config::{CommunityConfig, HostedAgentConfig, validate_relay_url},
    diagnostics::DiagnosticHandle,
    error::{Error, Result},
    paths::Paths,
    protocol::http::HttpClient,
    realtime::supervisor::SupervisorHandle,
    store::writer::StoreHandle,
};

pub struct ServeOptions<'a> {
    pub paths: &'a Paths,
    pub store: StoreHandle,
    pub diagnostics: DiagnosticHandle,
    pub community: &'a CommunityConfig,
    /// The whole configuration's agents; those of another community or
    /// owner, and those the user runs himself, are passed over.
    pub agents: &'a [HostedAgentConfig],
    pub owner: &'a SignerHandle,
    pub owner_identity: Uuid,
    /// The community's NIP-FA adapter, when the owner's own connection
    /// carries federated-identity evidence.
    pub federated: Option<url::Url>,
    pub limits: Limits,
}

/// The local agents of one community, running.
pub struct CommunityAgents {
    pub community: Uuid,
    pub host: AgentHost,
    signers: Vec<SignerHandle>,
    supervisors: Vec<SupervisorHandle>,
    /// Agents that could not be started, each with its reason.
    pub problems: Vec<String>,
}

impl CommunityAgents {
    /// Stops every harness, closes every agent connection and forgets every
    /// agent key.
    pub async fn stop(self) {
        self.host.shutdown().await;
        for supervisor in &self.supervisors {
            supervisor.shutdown().await;
        }
        for signer in &self.signers {
            signer.lock().await;
        }
    }
}

async fn relay_pubkey(store: &StoreHandle, community: Uuid, http: &HttpClient) -> Result<String> {
    if let Some(pinned) = store
        .call(move |store| store.pinned_relay_pubkey(community))
        .await?
    {
        return Ok(pinned);
    }
    let info = http.nip11().await?;
    let pubkey = crate::protocol::http::relay_signing_pubkey(&info)
        .ok_or_else(|| Error::Protocol("the relay does not publish its signing key".into()))?
        .to_owned();
    let pinned = pubkey.clone();
    store
        .call(move |store| store.pin_relay_pubkey(community, &pinned))
        .await?;
    Ok(pubkey)
}

/// Starts the runtime for the local agents of `community` that belong to
/// `owner_identity`. `None`: there are none. Leftovers of a dead bzz are
/// stopped first, and each agent's open requests are settled before it runs.
pub async fn serve(options: ServeOptions<'_>) -> Result<Option<CommunityAgents>> {
    let community = options.community.id;
    let local = options
        .agents
        .iter()
        .filter(|agent| {
            agent.local.is_some()
                && agent.community == community
                && agent.owner_identity == options.owner_identity
        })
        .collect::<Vec<_>>();
    if local.is_empty() {
        return Ok(None);
    }
    let endpoint = validate_relay_url(
        &options.community.relay_url,
        options.community.allow_insecure_localhost,
    )?;
    let mut host_options = HostOptions::new(options.paths.data_dir.clone(), options.store.clone());
    host_options.limits = options.limits;
    let host = AgentHost::start(host_options).await?;
    let keys_dir = options.paths.keys_dir();
    let owner_pubkey = options.owner.public_key().to_hex();
    let mut running = CommunityAgents {
        community,
        host: host.clone(),
        signers: Vec::new(),
        supervisors: Vec::new(),
        problems: Vec::new(),
    };
    for agent in local {
        // Paused, moved away, or run by another bzz: listed, never started.
        if !host.runnable(agent) {
            continue;
        }
        let Some(settings) = agent.local.clone() else {
            continue;
        };
        let built = async {
            let signer = crate::broker::custody::unlock(&keys_dir, options.owner, agent).await?;
            let auth_tag = crate::broker::relay::auth_tag(&agent.auth_tag)?;
            // NIP-FI communities: the agent proves itself with its own
            // owner-authorized grant, never with the owner's session.
            let evidence = crate::auth::federated::EvidenceSlot::default();
            if let Some(adapter) = &options.federated {
                match crate::auth::federated::agent_evidence(
                    adapter,
                    &options.paths.data_dir,
                    options.owner_identity,
                    options.owner,
                    &signer,
                    &agent.auth_tag,
                )
                .await?
                {
                    Some(source) => evidence.set(source),
                    None => {
                        signer.lock().await;
                        return Err(Error::Auth(
                            "sign in (:signin) to enroll agents with the identity adapter".into(),
                        ));
                    }
                }
            }
            let http = HttpClient::new(endpoint.http_base.clone(), signer.clone())?
                .with_evidence(evidence.clone());
            let relay_pubkey = relay_pubkey(&options.store, community, &http).await?;
            let supervisor = SupervisorHandle::spawn_with_evidence(
                endpoint.websocket.clone(),
                signer.clone(),
                options.diagnostics.clone(),
                evidence,
            );
            Ok::<_, Error>(Arc::new(RelayPort::new(
                signer,
                http,
                supervisor,
                auth_tag,
                relay_pubkey,
            )))
        }
        .await;
        match built {
            Ok(port) => {
                running.signers.push(port.signer.clone());
                running.supervisors.push(port.supervisor.clone());
                host.add(
                    AgentSpec {
                        id: agent.id,
                        name: agent.label.clone(),
                        pubkey: agent.pubkey.clone(),
                        owner: owner_pubkey.clone(),
                        community,
                        local: settings,
                    },
                    port.clone(),
                )
                .await;
                host.listen(port, agent.id, owner_pubkey.clone());
            }
            Err(error) => running
                .problems
                .push(format!("agent {} unavailable: {error}", agent.label)),
        }
    }
    Ok(Some(running))
}

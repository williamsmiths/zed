use anyhow::{Result, anyhow};
use client::{Client, Subscription, UserStore};
use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, Global, Task};
use rpc::{TypedEnvelope, proto};
use std::sync::Arc;

/// Client-side store for the multi-agent Council feature. Mirrors `ChannelStore`:
/// it talks to the collab server over RPC, holds the local view of the joined
/// session, and applies server broadcasts. See docs/proposals/council-multi-agent.md.
pub struct CouncilStore {
    client: Arc<Client>,
    user_store: Entity<UserStore>,
    session: Option<proto::CouncilSession>,
    participant_id: Option<u64>,
    replica_id: Option<u32>,
    participants: Vec<proto::CouncilParticipant>,
    entries: Vec<proto::CouncilEntry>,
    work_items: Vec<proto::WorkItem>,
    _rpc_subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CouncilEvent {
    Joined,
    EntryPosted(u64),
    ParticipantsChanged,
    SessionUpdated,
    WorkItemsChanged,
}

impl EventEmitter<CouncilEvent> for CouncilStore {}

impl CouncilStore {
    pub fn new(
        client: Arc<Client>,
        user_store: Entity<UserStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            client.add_message_handler(cx.weak_entity(), Self::handle_entry_posted),
            client.add_message_handler(cx.weak_entity(), Self::handle_participant_updated),
            client.add_message_handler(cx.weak_entity(), Self::handle_session_updated),
            client.add_message_handler(cx.weak_entity(), Self::handle_work_item_updated),
        ];
        Self {
            client,
            user_store,
            session: None,
            participant_id: None,
            replica_id: None,
            participants: Vec::new(),
            entries: Vec::new(),
            work_items: Vec::new(),
            _rpc_subscriptions: subscriptions,
        }
    }

    pub fn user_store(&self) -> &Entity<UserStore> {
        &self.user_store
    }

    pub fn session(&self) -> Option<&proto::CouncilSession> {
        self.session.as_ref()
    }

    pub fn participant_id(&self) -> Option<u64> {
        self.participant_id
    }

    pub fn replica_id(&self) -> Option<u32> {
        self.replica_id
    }

    pub fn participants(&self) -> &[proto::CouncilParticipant] {
        &self.participants
    }

    pub fn entries(&self) -> &[proto::CouncilEntry] {
        &self.entries
    }

    pub fn work_items(&self) -> &[proto::WorkItem] {
        &self.work_items
    }

    fn apply_state(&mut self, state: Option<proto::CouncilState>) {
        if let Some(state) = state {
            self.session = state.session;
            self.participants = state.participants;
            self.entries = state.entries;
            self.work_items = state.work_items;
        }
    }

    /// Join (or open) the council session for a project as the given kind.
    pub fn join(
        &self,
        project_id: u64,
        kind: proto::CouncilParticipantKind,
        agent_label: String,
        model: String,
        tool: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let response = client
                .request(proto::JoinCouncil {
                    project_id,
                    kind: kind as i32,
                    agent_label,
                    model,
                    tool,
                })
                .await?;
            this.update(cx, |this, cx| {
                this.apply_state(response.state);
                this.participant_id = Some(response.participant_id);
                this.replica_id = Some(response.replica_id);
                cx.emit(CouncilEvent::Joined);
                cx.notify();
            })?;
            Ok(())
        })
    }

    /// Leave the current session.
    pub fn leave(&self, cx: &mut Context<Self>) -> Task<Result<()>> {
        let (Some(session), Some(participant_id)) = (self.session.as_ref(), self.participant_id)
        else {
            return Task::ready(Ok(()));
        };
        let session_id = session.id;
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            client
                .request(proto::LeaveCouncil {
                    session_id,
                    participant_id,
                })
                .await?;
            this.update(cx, |this, cx| {
                this.session = None;
                this.participant_id = None;
                this.replica_id = None;
                this.participants.clear();
                this.entries.clear();
                this.work_items.clear();
                cx.notify();
            })?;
            Ok(())
        })
    }

    /// Post an entry to the council blackboard.
    pub fn post_entry(
        &self,
        kind: proto::CouncilEntryKind,
        body: String,
        refs: Vec<u64>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let response = client
                .request(proto::PostCouncilEntry {
                    session_id,
                    kind: kind as i32,
                    body,
                    refs,
                })
                .await?;
            this.update(cx, |this, cx| {
                if let Some(entry) = response.entry {
                    let id = entry.id;
                    if !this.entries.iter().any(|existing| existing.id == id) {
                        this.entries.push(entry);
                        cx.emit(CouncilEvent::EntryPosted(id));
                        cx.notify();
                    }
                }
            })?;
            Ok(())
        })
    }

    /// Post a plain analysis message to the council. Convenience over
    /// `post_entry` for callers (such as agent tools) that don't need to pick an
    /// entry kind or references.
    pub fn post_message(&self, body: String, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.post_entry(proto::CouncilEntryKind::Analysis, body, Vec::new(), cx)
    }

    /// Advance the session phase (the Supervisor's prerogative).
    pub fn advance_phase(
        &self,
        phase: proto::CouncilPhase,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            client
                .request(proto::AdvanceCouncilPhase {
                    session_id,
                    phase: phase as i32,
                })
                .await?;
            Ok(())
        })
    }

    /// Change who holds final authority (Super only, enforced server-side).
    pub fn set_authority(
        &self,
        authority: proto::CouncilAuthority,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            client
                .request(proto::SetCouncilAuthority {
                    session_id,
                    authority: authority as i32,
                })
                .await?;
            Ok(())
        })
    }

    async fn handle_entry_posted(
        this: Entity<Self>,
        message: TypedEnvelope<proto::CouncilEntryPosted>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            if let Some(entry) = message.payload.entry {
                let id = entry.id;
                if !this.entries.iter().any(|existing| existing.id == id) {
                    this.entries.push(entry);
                    cx.emit(CouncilEvent::EntryPosted(id));
                    cx.notify();
                }
            }
        });
        Ok(())
    }

    async fn handle_participant_updated(
        this: Entity<Self>,
        message: TypedEnvelope<proto::CouncilParticipantUpdated>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            if let Some(participant) = message.payload.participant {
                if let Some(existing) = this
                    .participants
                    .iter_mut()
                    .find(|existing| existing.id == participant.id)
                {
                    *existing = participant;
                } else {
                    this.participants.push(participant);
                }
                cx.emit(CouncilEvent::ParticipantsChanged);
                cx.notify();
            }
        });
        Ok(())
    }

    async fn handle_session_updated(
        this: Entity<Self>,
        message: TypedEnvelope<proto::CouncilSessionUpdated>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            if let Some(session) = message.payload.session {
                if this
                    .session
                    .as_ref()
                    .is_some_and(|current| current.id == session.id)
                {
                    this.session = Some(session);
                    cx.emit(CouncilEvent::SessionUpdated);
                    cx.notify();
                }
            }
        });
        Ok(())
    }

    async fn handle_work_item_updated(
        this: Entity<Self>,
        message: TypedEnvelope<proto::WorkItemUpdated>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            if let Some(item) = message.payload.item {
                if let Some(existing) = this
                    .work_items
                    .iter_mut()
                    .find(|existing| existing.id == item.id)
                {
                    *existing = item;
                } else {
                    this.work_items.push(item);
                }
                cx.emit(CouncilEvent::WorkItemsChanged);
                cx.notify();
            }
        });
        Ok(())
    }
}

struct GlobalCouncilStore(Entity<CouncilStore>);

impl Global for GlobalCouncilStore {}

impl CouncilStore {
    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalCouncilStore>().0.clone()
    }

    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalCouncilStore>().map(|store| store.0.clone())
    }
}

pub fn init(client: &Arc<Client>, user_store: Entity<UserStore>, cx: &mut App) {
    let council_store = cx.new(|cx| CouncilStore::new(client.clone(), user_store, cx));
    cx.set_global(GlobalCouncilStore(council_store));
}

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

    /// System prompt template for a Supervisor agent.
    /// The Supervisor moderates discussion, drives phase transitions,
    /// and ultimately synthesizes the work-item list.
    pub fn supervisor_system_prompt() -> &'static str {
        concat!(
            "You are the Supervisor in a Zed Council session — a structured multi-agent deliberation room.\n",
            "\n",
            "## Your role\n",
            "- You facilitate discussion and hold the session to its purpose.\n",
            "- You alone may call AdvancePhase to move the session between phases:\n",
            "  Frame → Diverge → Converge ↔ Synthesize → Gate → Finalized.\n",
            "- When consensus forms in Converge, move to Synthesize and call SubmitTaskDraft\n",
            "  with a ranked list of concrete work items.\n",
            "- Do NOT approve your own draft; that is the Super's (human's) prerogative\n",
            "  unless the session authority is SupervisorAutonomous.\n",
            "\n",
            "## Phases\n",
            "- Frame   : Session goal is defined. Introduce context and constraints.\n",
            "- Diverge  : Peers explore the problem space broadly. Encourage diverse angles.\n",
            "- Converge : Focus on emerging consensus. Surface disagreements early.\n",
            "- Synthesize: Distil discussion into a concrete task list (WorkItem[]). Post as TaskDraft.\n",
            "- Gate     : Awaiting Super approval. Stay silent unless the Super requests clarification.\n",
            "- Finalized: Work items are approved. Announce next steps.\n",
            "\n",
            "## Guardrails\n",
            "- If a round_cap is set, you will NOT be able to re-enter Converge after the cap\n",
            "  is hit. Use your rounds wisely.\n",
            "- Keep entries concise (< 800 tokens). Surface quality, not volume.\n",
        )
    }

    /// System prompt template for a Peer agent.
    /// Peers contribute analysis, proposals, and critiques during the deliberation.
    pub fn peer_system_prompt() -> &'static str {
        concat!(
            "You are a Peer agent in a Zed Council session — a structured multi-agent deliberation room.\n",
            "\n",
            "## Your role\n",
            "- Contribute Analysis, Critique, and Proposal entries during Diverge and Converge phases.\n",
            "- You may NOT advance the phase or submit/approve task drafts — those are reserved\n",
            "  for the Supervisor and Super respectively.\n",
            "- You may NOT post Approval entries under default (human_final) authority.\n",
            "\n",
            "## Norms\n",
            "- Be direct and specific. State your position in the first sentence.\n",
            "- Reference other entries by their lamport ID when building on or critiquing them.\n",
            "- Keep entries focused (< 600 tokens each).\n",
            "- Disagree with other Peers when warranted — diversity of view is the point.\n",
            "\n",
            "## Phases\n",
            "- Frame   : Read context the Supervisor provides. Ask clarifying questions if needed.\n",
            "- Diverge  : Post Analysis entries. Explore widely.\n",
            "- Converge : Post Proposal and Critique entries. Help the group reach agreement.\n",
            "- Synthesize/Gate/Finalized: Stay silent unless addressed directly.\n",
        )
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

    /// Join the project's council as a peer with a default agent identity.
    /// Convenience for callers (such as agent tools) that just need to take part.
    pub fn join_for_project(&self, project_id: u64, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.join(
            project_id,
            proto::CouncilParticipantKind::Peer,
            "agent".to_string(),
            String::new(),
            String::new(),
            cx,
        )
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

    /// Advance the session phase (Supervisor or Super only, enforced server-side).
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

    /// Submit a proposed list of work items as a task draft (Supervisor or Super).
    /// The server creates a TaskDraft entry and advances the session to the Gate phase.
    pub fn submit_task_draft(
        &self,
        items: Vec<proto::WorkItem>,
        cx: &mut Context<Self>,
    ) -> Task<Result<u64>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            let response = client
                .request(proto::SubmitTaskDraft { session_id, items })
                .await?;
            Ok(response.draft_entry_id)
        })
    }

    /// Approve or reject a task draft (Super only, or autonomous Supervisor).
    /// On approval the server materializes work items and the session finalizes.
    pub fn approve_task_draft(
        &self,
        draft_entry_id: u64,
        approved: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<proto::WorkItem>>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            let response = client
                .request(proto::ApproveTaskDraft {
                    session_id,
                    draft_entry_id,
                    approved,
                })
                .await?;
            Ok(response.work_items)
        })
    }

    /// Upsert a work item (Super may edit status / title / description).
    pub fn upsert_work_item(
        &self,
        item: proto::WorkItem,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(session_id) = self.session.as_ref().map(|session| session.id) else {
            return Task::ready(Err(anyhow!("not in a council session")));
        };
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            client
                .request(proto::UpsertWorkItem {
                    session_id,
                    item: Some(item),
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

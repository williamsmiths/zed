use super::*;
use anyhow::anyhow;
use rpc::proto;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder,
};
use serde::{Deserialize, Serialize};

/// Items serialized into a TaskDraft entry body.
#[derive(Serialize, Deserialize)]
struct DraftItem {
    title: String,
    description: String,
    sort_order: i32,
}

fn valid_phase_transition(from: CouncilPhase, to: CouncilPhase) -> bool {
    use CouncilPhase::*;
    matches!(
        (from, to),
        (Frame, Diverge)
            | (Diverge, Converge)
            | (Converge, Synthesize)
            | (Converge, Diverge)
            | (Synthesize, Gate)
            | (Gate, Finalized)
            | (Gate, Synthesize)
    )
}

impl Database {
    /// Join (or open) the council session for a project. Creates the session if
    /// none is active yet, allocates a collaboration replica id for the new
    /// participant, and returns the full state to bootstrap the client.
    pub async fn join_council(
        &self,
        project_id: ProjectId,
        kind: CouncilParticipantKind,
        user_id: Option<UserId>,
        agent_label: String,
        model: String,
        tool: String,
    ) -> Result<(council_participant::Model, proto::CouncilState)> {
        self.transaction(move |tx| {
            let agent_label = agent_label.clone();
            let model = model.clone();
            let tool = tool.clone();
            async move {
                let existing = council_session::Entity::find()
                    .filter(council_session::Column::ProjectId.eq(project_id))
                    .filter(council_session::Column::Phase.ne(CouncilPhase::Finalized))
                    .order_by_desc(council_session::Column::Id)
                    .one(&*tx)
                    .await?;
                let session = match existing {
                    Some(session) => session,
                    None => council_session::ActiveModel {
                        project_id: ActiveValue::Set(project_id),
                        phase: ActiveValue::Set(CouncilPhase::Frame),
                        round: ActiveValue::Set(0),
                        round_cap: ActiveValue::Set(0),
                        authority: ActiveValue::Set(CouncilAuthority::HumanFinal),
                        ..Default::default()
                    }
                    .insert(&*tx)
                    .await?,
                };

                // Allocate a replica id from the collaboration range (>= 8) so
                // many agents can edit concurrently without colliding with the
                // reserved local/server/agent replica ids.
                let max_replica_id = council_participant::Entity::find()
                    .filter(council_participant::Column::SessionId.eq(session.id))
                    .order_by_desc(council_participant::Column::ReplicaId)
                    .one(&*tx)
                    .await?
                    .map_or(7, |participant| participant.replica_id);

                let participant = council_participant::ActiveModel {
                    session_id: ActiveValue::Set(session.id),
                    kind: ActiveValue::Set(kind),
                    user_id: ActiveValue::Set(user_id),
                    agent_label: ActiveValue::Set(agent_label),
                    model: ActiveValue::Set(model),
                    tool: ActiveValue::Set(tool),
                    replica_id: ActiveValue::Set(max_replica_id + 1),
                    active: ActiveValue::Set(true),
                    ..Default::default()
                }
                .insert(&*tx)
                .await?;

                // The first supervisor to join becomes the session's supervisor.
                let session = if kind == CouncilParticipantKind::Supervisor
                    && session.supervisor_participant_id.is_none()
                {
                    let mut session = session.into_active_model();
                    session.supervisor_participant_id = ActiveValue::Set(Some(participant.id));
                    session.update(&*tx).await?
                } else {
                    session
                };

                let state = self.council_state(session, &tx).await?;
                Ok((participant, state))
            }
        })
        .await
    }

    /// Mark a participant as no longer active in the session.
    pub async fn leave_council(
        &self,
        session_id: CouncilSessionId,
        participant_id: CouncilParticipantId,
    ) -> Result<council_participant::Model> {
        self.transaction(move |tx| async move {
            let participant = council_participant::Entity::find_by_id(participant_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council participant"))?;
            if participant.session_id != session_id {
                Err(anyhow!("participant does not belong to this council session"))?;
            }
            let mut participant = participant.into_active_model();
            participant.active = ActiveValue::Set(false);
            Ok(participant.update(&*tx).await?)
        })
        .await
    }

    /// Append an entry to the council blackboard, ordered by a per-session
    /// Lamport clock. Enforces that only the Super (or an autonomous Supervisor)
    /// may post an `Approval` entry.
    pub async fn post_council_entry(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        kind: CouncilEntryKind,
        body: String,
        refs: Vec<u64>,
    ) -> Result<council_entry::Model> {
        self.transaction(move |tx| {
            let body = body.clone();
            let refs = refs.clone();
            async move {
                let author = council_participant::Entity::find()
                    .filter(council_participant::Column::SessionId.eq(session_id))
                    .filter(council_participant::Column::UserId.eq(user_id))
                    .filter(council_participant::Column::Active.eq(true))
                    .order_by_desc(council_participant::Column::Id)
                    .one(&*tx)
                    .await?
                    .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;

                if kind == CouncilEntryKind::Approval {
                    let session = council_session::Entity::find_by_id(session_id)
                        .one(&*tx)
                        .await?
                        .ok_or_else(|| anyhow!("no such council session"))?;
                    let allowed = author.kind == CouncilParticipantKind::Super
                        || (author.kind == CouncilParticipantKind::Supervisor
                            && session.authority == CouncilAuthority::SupervisorAutonomous);
                    if !allowed {
                        Err(anyhow!(
                            "only the Super (or an autonomous Supervisor) may finalize"
                        ))?;
                    }
                }

                let max_lamport = council_entry::Entity::find()
                    .filter(council_entry::Column::SessionId.eq(session_id))
                    .order_by_desc(council_entry::Column::LamportValue)
                    .one(&*tx)
                    .await?
                    .map_or(0, |entry| entry.lamport_value);

                let refs = serde_json::to_string(&refs).unwrap_or_else(|_| "[]".to_string());
                let entry = council_entry::ActiveModel {
                    session_id: ActiveValue::Set(session_id),
                    author_participant_id: ActiveValue::Set(author.id),
                    lamport_value: ActiveValue::Set(max_lamport + 1),
                    lamport_replica_id: ActiveValue::Set(author.replica_id),
                    kind: ActiveValue::Set(kind),
                    body: ActiveValue::Set(body),
                    refs: ActiveValue::Set(refs),
                    ..Default::default()
                }
                .insert(&*tx)
                .await?;
                Ok(entry)
            }
        })
        .await
    }

    /// Advance the lifecycle phase of a council session.
    /// Only the Supervisor (or Super as kill-switch) may call this.
    pub async fn advance_council_phase(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        phase: CouncilPhase,
    ) -> Result<council_session::Model> {
        self.transaction(move |tx| async move {
            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;

            let caller = council_participant::Entity::find()
                .filter(council_participant::Column::SessionId.eq(session_id))
                .filter(council_participant::Column::UserId.eq(user_id))
                .filter(council_participant::Column::Active.eq(true))
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;
            if caller.kind != CouncilParticipantKind::Supervisor
                && caller.kind != CouncilParticipantKind::Super
            {
                Err(anyhow!("only the Supervisor (or Super) may advance the phase"))?;
            }

            if !valid_phase_transition(session.phase, phase) {
                Err(anyhow!(
                    "invalid phase transition: {:?} → {:?}",
                    session.phase,
                    phase
                ))?;
            }

            // Enforce round_cap: each transition to Converge counts as a new round.
            let entering_converge = phase == CouncilPhase::Converge;
            let new_round = if entering_converge {
                session.round + 1
            } else {
                session.round
            };
            if entering_converge && session.round_cap > 0 && new_round > session.round_cap {
                Err(anyhow!(
                    "round cap of {} reached; session cannot enter Converge again",
                    session.round_cap
                ))?;
            }

            let mut session = session.into_active_model();
            session.phase = ActiveValue::Set(phase);
            if entering_converge {
                session.round = ActiveValue::Set(new_round);
            }
            Ok(session.update(&*tx).await?)
        })
        .await
    }

    /// Change who holds final authority in the session.
    /// Only the Super may call this.
    pub async fn set_council_authority(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        authority: CouncilAuthority,
    ) -> Result<council_session::Model> {
        self.transaction(move |tx| async move {
            let caller = council_participant::Entity::find()
                .filter(council_participant::Column::SessionId.eq(session_id))
                .filter(council_participant::Column::UserId.eq(user_id))
                .filter(council_participant::Column::Active.eq(true))
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;
            if caller.kind != CouncilParticipantKind::Super {
                Err(anyhow!("only the Super may change the authority setting"))?;
            }

            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;
            let mut session = session.into_active_model();
            session.authority = ActiveValue::Set(authority);
            Ok(session.update(&*tx).await?)
        })
        .await
    }

    /// Set the round cap for a council session (Super only).
    /// 0 = unlimited; N = max N Converge phases allowed.
    pub async fn set_round_cap(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        round_cap: i32,
    ) -> Result<council_session::Model> {
        self.transaction(move |tx| async move {
            let caller = council_participant::Entity::find()
                .filter(council_participant::Column::SessionId.eq(session_id))
                .filter(council_participant::Column::UserId.eq(user_id))
                .filter(council_participant::Column::Active.eq(true))
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;
            if caller.kind != CouncilParticipantKind::Super {
                Err(anyhow!("only the Super may set the round cap"))?;
            }
            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;
            let mut session = session.into_active_model();
            session.round_cap = ActiveValue::Set(round_cap);
            Ok(session.update(&*tx).await?)
        })
        .await
    }

    /// Submit a draft list of work items (Supervisor or Super only).
    /// Creates a TaskDraft entry and advances the session to the Gate phase.
    pub async fn submit_task_draft(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        items: Vec<proto::WorkItem>,
    ) -> Result<(council_entry::Model, Vec<proto::WorkItem>)> {
        self.transaction(move |tx| {
            let items = items.clone();
            async move {
                let session = council_session::Entity::find_by_id(session_id)
                    .one(&*tx)
                    .await?
                    .ok_or_else(|| anyhow!("no such council session"))?;

                let author = council_participant::Entity::find()
                    .filter(council_participant::Column::SessionId.eq(session_id))
                    .filter(council_participant::Column::UserId.eq(user_id))
                    .filter(council_participant::Column::Active.eq(true))
                    .one(&*tx)
                    .await?
                    .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;
                if author.kind != CouncilParticipantKind::Supervisor
                    && author.kind != CouncilParticipantKind::Super
                {
                    Err(anyhow!("only the Supervisor (or Super) may submit a task draft"))?;
                }

                let draft_items: Vec<DraftItem> = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| DraftItem {
                        title: item.title.clone(),
                        description: item.description.clone(),
                        sort_order: item.sort_order.max(0).max(i as i32),
                    })
                    .collect();
                let body = serde_json::to_string(&draft_items)
                    .unwrap_or_else(|_| "[]".to_string());

                let max_lamport = council_entry::Entity::find()
                    .filter(council_entry::Column::SessionId.eq(session_id))
                    .order_by_desc(council_entry::Column::LamportValue)
                    .one(&*tx)
                    .await?
                    .map_or(0, |e| e.lamport_value);

                let entry = council_entry::ActiveModel {
                    session_id: ActiveValue::Set(session_id),
                    author_participant_id: ActiveValue::Set(author.id),
                    lamport_value: ActiveValue::Set(max_lamport + 1),
                    lamport_replica_id: ActiveValue::Set(author.replica_id),
                    kind: ActiveValue::Set(CouncilEntryKind::TaskDraft),
                    body: ActiveValue::Set(body),
                    refs: ActiveValue::Set("[]".to_string()),
                    ..Default::default()
                }
                .insert(&*tx)
                .await?;

                // Advance to Gate phase if currently in Synthesize.
                if session.phase == CouncilPhase::Synthesize {
                    let mut session = session.into_active_model();
                    session.phase = ActiveValue::Set(CouncilPhase::Gate);
                    session.update(&*tx).await?;
                }

                Ok((entry, items))
            }
        })
        .await
    }

    /// Approve or reject a task draft.
    /// If approved: materializes work_item rows and advances the session to Finalized.
    /// If rejected: advances back to Synthesize for another round.
    /// Only the Super (or autonomous Supervisor) may call this.
    pub async fn approve_task_draft(
        &self,
        session_id: CouncilSessionId,
        user_id: UserId,
        draft_entry_id: CouncilEntryId,
        approved: bool,
    ) -> Result<Vec<work_item::Model>> {
        self.transaction(move |tx| async move {
            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;

            let caller = council_participant::Entity::find()
                .filter(council_participant::Column::SessionId.eq(session_id))
                .filter(council_participant::Column::UserId.eq(user_id))
                .filter(council_participant::Column::Active.eq(true))
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("you are not a participant in this council session"))?;
            let allowed = caller.kind == CouncilParticipantKind::Super
                || (caller.kind == CouncilParticipantKind::Supervisor
                    && session.authority == CouncilAuthority::SupervisorAutonomous);
            if !allowed {
                Err(anyhow!(
                    "only the Super (or an autonomous Supervisor) may approve or reject a draft"
                ))?;
            }

            let draft_entry = council_entry::Entity::find_by_id(draft_entry_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such draft entry"))?;
            if draft_entry.session_id != session_id
                || draft_entry.kind != CouncilEntryKind::TaskDraft
            {
                Err(anyhow!("entry is not a task draft for this session"))?;
            }

            let project_id = session.project_id;
            let new_phase = if approved {
                CouncilPhase::Finalized
            } else {
                CouncilPhase::Synthesize
            };
            let mut session_model = session.into_active_model();
            session_model.phase = ActiveValue::Set(new_phase);
            session_model.update(&*tx).await?;

            if !approved {
                return Ok(Vec::new());
            }

            let draft_items: Vec<DraftItem> =
                serde_json::from_str(&draft_entry.body).unwrap_or_default();

            let mut work_items = Vec::with_capacity(draft_items.len());
            for item in draft_items {
                let model = work_item::ActiveModel {
                    project_id: ActiveValue::Set(project_id),
                    session_id: ActiveValue::Set(session_id),
                    source_entry_id: ActiveValue::Set(Some(draft_entry_id)),
                    title: ActiveValue::Set(item.title),
                    description: ActiveValue::Set(item.description),
                    status: ActiveValue::Set(WorkItemStatus::Todo),
                    sort_order: ActiveValue::Set(item.sort_order),
                    ..Default::default()
                }
                .insert(&*tx)
                .await?;
                work_items.push(model);
            }
            Ok(work_items)
        })
        .await
    }

    /// Upsert a work item (Super may edit title / description / status / assignee).
    pub async fn upsert_work_item(
        &self,
        session_id: CouncilSessionId,
        item: proto::WorkItem,
    ) -> Result<work_item::Model> {
        self.transaction(move |tx| {
            let item = item.clone();
            async move {
                let status = WorkItemStatus::from(item.status());
                if item.id == 0 {
                    let session = council_session::Entity::find_by_id(session_id)
                        .one(&*tx)
                        .await?
                        .ok_or_else(|| anyhow!("no such council session"))?;
                    let model = work_item::ActiveModel {
                        project_id: ActiveValue::Set(session.project_id),
                        session_id: ActiveValue::Set(session_id),
                        status: ActiveValue::Set(status),
                        title: ActiveValue::Set(item.title),
                        description: ActiveValue::Set(item.description),
                        sort_order: ActiveValue::Set(item.sort_order),
                        ..Default::default()
                    }
                    .insert(&*tx)
                    .await?;
                    Ok(model)
                } else {
                    let work_item_id = WorkItemId::from_proto(item.id);
                    let existing = work_item::Entity::find_by_id(work_item_id)
                        .one(&*tx)
                        .await?
                        .ok_or_else(|| anyhow!("no such work item"))?;
                    let mut model = existing.into_active_model();
                    model.status = ActiveValue::Set(status);
                    model.title = ActiveValue::Set(item.title);
                    model.description = ActiveValue::Set(item.description);
                    model.sort_order = ActiveValue::Set(item.sort_order);
                    Ok(model.update(&*tx).await?)
                }
            }
        })
        .await
    }

    /// The distinct user ids of the active participants in a session, used to
    /// fan council broadcasts out to their connections.
    pub async fn council_session_user_ids(
        &self,
        session_id: CouncilSessionId,
    ) -> Result<Vec<UserId>> {
        self.transaction(move |tx| async move {
            let participants = council_participant::Entity::find()
                .filter(council_participant::Column::SessionId.eq(session_id))
                .filter(council_participant::Column::Active.eq(true))
                .all(&*tx)
                .await?;
            let mut user_ids = participants
                .into_iter()
                .filter_map(|participant| participant.user_id)
                .collect::<Vec<_>>();
            user_ids.sort_unstable();
            user_ids.dedup();
            Ok(user_ids)
        })
        .await
    }

    /// Public wrapper: load full proto state by session id (for RPC handlers).
    pub async fn council_session_state(
        &self,
        session_id: CouncilSessionId,
    ) -> Result<proto::CouncilState> {
        self.transaction(move |tx| async move {
            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;
            self.council_state(session, &tx).await
        })
        .await
    }

    /// Load the full proto state of a session (participants, entries, work items).
    async fn council_state(
        &self,
        session: council_session::Model,
        tx: &DatabaseTransaction,
    ) -> Result<proto::CouncilState> {
        let participants = council_participant::Entity::find()
            .filter(council_participant::Column::SessionId.eq(session.id))
            .order_by_asc(council_participant::Column::Id)
            .all(tx)
            .await?;
        let entries = council_entry::Entity::find()
            .filter(council_entry::Column::SessionId.eq(session.id))
            .order_by_asc(council_entry::Column::LamportValue)
            .all(tx)
            .await?;
        let work_items = work_item::Entity::find()
            .filter(work_item::Column::SessionId.eq(session.id))
            .order_by_asc(work_item::Column::SortOrder)
            .all(tx)
            .await?;
        Ok(proto::CouncilState {
            session: Some(session.into()),
            participants: participants.into_iter().map(Into::into).collect(),
            entries: entries.into_iter().map(Into::into).collect(),
            work_items: work_items.into_iter().map(Into::into).collect(),
        })
    }
}

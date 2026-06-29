use super::*;
use anyhow::anyhow;
use rpc::proto;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder,
};

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

    /// Advance the lifecycle phase of a council session (driven by the Supervisor).
    pub async fn advance_council_phase(
        &self,
        session_id: CouncilSessionId,
        phase: CouncilPhase,
    ) -> Result<council_session::Model> {
        self.transaction(move |tx| async move {
            let session = council_session::Entity::find_by_id(session_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such council session"))?;
            let mut session = session.into_active_model();
            session.phase = ActiveValue::Set(phase);
            Ok(session.update(&*tx).await?)
        })
        .await
    }

    /// Change who holds final authority in the session (Super only, at the RPC layer).
    pub async fn set_council_authority(
        &self,
        session_id: CouncilSessionId,
        authority: CouncilAuthority,
    ) -> Result<council_session::Model> {
        self.transaction(move |tx| async move {
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

use super::{MessageContext, Response};
use crate::Result;
use crate::db::{
    CouncilAuthority, CouncilEntryKind, CouncilParticipantId, CouncilParticipantKind, CouncilPhase,
    CouncilSessionId, ProjectId,
};
use rpc::proto;

/// Fan a council message out to the connections of every active participant.
/// Agents that join over MCP without a user id are reached when they (re)read
/// the session state; realtime delivery to them lands with the MCP bridge (M5).
async fn broadcast_to_council<T: proto::EnvelopedMessage + Clone>(
    session: &MessageContext,
    session_id: CouncilSessionId,
    message: T,
    exclude_self: bool,
) -> Result<()> {
    let user_ids = session
        .db()
        .await
        .council_session_user_ids(session_id)
        .await?;
    let connection_pool = session.connection_pool().await;
    for user_id in user_ids {
        for connection_id in connection_pool.user_connection_ids(user_id) {
            if exclude_self && connection_id == session.connection_id {
                continue;
            }
            session.peer.send(connection_id, message.clone())?;
        }
    }
    Ok(())
}

pub async fn join_council(
    request: proto::JoinCouncil,
    response: Response<proto::JoinCouncil>,
    session: MessageContext,
) -> Result<()> {
    let project_id = ProjectId::from_proto(request.project_id);
    let kind = CouncilParticipantKind::from(request.kind());
    let (participant, state) = session
        .db()
        .await
        .join_council(
            project_id,
            kind,
            Some(session.user_id()),
            request.agent_label,
            request.model,
            request.tool,
        )
        .await?;

    let session_id = participant.session_id;
    let participant_id = participant.id.to_proto();
    let replica_id = participant.replica_id as u32;
    let participant: proto::CouncilParticipant = participant.into();

    response.send(proto::JoinCouncilResponse {
        state: Some(state),
        participant_id,
        replica_id,
    })?;

    broadcast_to_council(
        &session,
        session_id,
        proto::CouncilParticipantUpdated {
            participant: Some(participant),
        },
        true,
    )
    .await?;
    Ok(())
}

pub async fn leave_council(
    request: proto::LeaveCouncil,
    response: Response<proto::LeaveCouncil>,
    session: MessageContext,
) -> Result<()> {
    let session_id = CouncilSessionId::from_proto(request.session_id);
    let participant_id = CouncilParticipantId::from_proto(request.participant_id);
    let participant = session
        .db()
        .await
        .leave_council(session_id, participant_id)
        .await?;
    response.send(proto::Ack {})?;
    broadcast_to_council(
        &session,
        session_id,
        proto::CouncilParticipantUpdated {
            participant: Some(participant.into()),
        },
        true,
    )
    .await?;
    Ok(())
}

pub async fn post_council_entry(
    request: proto::PostCouncilEntry,
    response: Response<proto::PostCouncilEntry>,
    session: MessageContext,
) -> Result<()> {
    let session_id = CouncilSessionId::from_proto(request.session_id);
    let kind = CouncilEntryKind::from(request.kind());
    let entry = session
        .db()
        .await
        .post_council_entry(
            session_id,
            session.user_id(),
            kind,
            request.body,
            request.refs,
        )
        .await?;
    let entry: proto::CouncilEntry = entry.into();
    response.send(proto::PostCouncilEntryResponse {
        entry: Some(entry.clone()),
    })?;
    broadcast_to_council(
        &session,
        session_id,
        proto::CouncilEntryPosted { entry: Some(entry) },
        true,
    )
    .await?;
    Ok(())
}

pub async fn advance_council_phase(
    request: proto::AdvanceCouncilPhase,
    response: Response<proto::AdvanceCouncilPhase>,
    session: MessageContext,
) -> Result<()> {
    let session_id = CouncilSessionId::from_proto(request.session_id);
    let phase = CouncilPhase::from(request.phase());
    let updated = session
        .db()
        .await
        .advance_council_phase(session_id, phase)
        .await?;
    response.send(proto::Ack {})?;
    broadcast_to_council(
        &session,
        session_id,
        proto::CouncilSessionUpdated {
            session: Some(updated.into()),
        },
        false,
    )
    .await?;
    Ok(())
}

pub async fn set_council_authority(
    request: proto::SetCouncilAuthority,
    response: Response<proto::SetCouncilAuthority>,
    session: MessageContext,
) -> Result<()> {
    let session_id = CouncilSessionId::from_proto(request.session_id);
    let authority = CouncilAuthority::from(request.authority());
    let updated = session
        .db()
        .await
        .set_council_authority(session_id, authority)
        .await?;
    response.send(proto::Ack {})?;
    broadcast_to_council(
        &session,
        session_id,
        proto::CouncilSessionUpdated {
            session: Some(updated.into()),
        },
        false,
    )
    .await?;
    Ok(())
}

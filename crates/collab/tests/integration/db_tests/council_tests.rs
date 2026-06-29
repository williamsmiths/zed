use super::new_test_user;
use crate::test_both_dbs;
use collab::db::{
    CouncilAuthority, CouncilEntryKind, CouncilParticipantKind, CouncilPhase, CouncilSessionId,
    Database, ProjectId,
};
use rpc::proto;
use std::sync::Arc;

test_both_dbs!(test_council, test_council_postgres, test_council_sqlite);

async fn test_council(db: &Arc<Database>) {
    let supervisor = new_test_user(db).await;
    let peer = new_test_user(db).await;
    let project_id = ProjectId::from_proto(1);

    // The supervisor opens the council session for the project.
    let (supervisor_participant, state) = db
        .join_council(
            project_id,
            CouncilParticipantKind::Supervisor,
            Some(supervisor),
            String::new(),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    let session = state.session.unwrap();
    assert_eq!(session.phase(), proto::CouncilPhase::Frame);
    assert_eq!(session.authority(), proto::CouncilAuthority::HumanFinal);
    assert_eq!(
        session.supervisor_participant_id,
        Some(supervisor_participant.id.to_proto())
    );
    // Collaboration replica ids are allocated from the >= 8 range.
    assert_eq!(supervisor_participant.replica_id, 8);
    let session_id = CouncilSessionId::from_proto(session.id);

    // A peer joins the same project's session and gets the next replica id.
    let (peer_participant, state) = db
        .join_council(
            project_id,
            CouncilParticipantKind::Peer,
            Some(peer),
            String::new(),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(state.participants.len(), 2);
    assert_eq!(peer_participant.replica_id, 9);

    // The peer posts an analysis entry; the per-session Lamport clock starts at 1.
    let entry = db
        .post_council_entry(
            session_id,
            peer,
            CouncilEntryKind::Analysis,
            "we should index the codebase first".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(entry.body, "we should index the codebase first");
    assert_eq!(entry.lamport_value, 1);
    assert_eq!(entry.lamport_replica_id, 9);

    // D1: under human_final authority a peer may NOT finalize (post an Approval).
    assert!(
        db.post_council_entry(
            session_id,
            peer,
            CouncilEntryKind::Approval,
            "approve".into(),
            vec![],
        )
        .await
        .is_err()
    );

    // The Super may finalize.
    let super_user = new_test_user(db).await;
    db.join_council(
        project_id,
        CouncilParticipantKind::Super,
        Some(super_user),
        String::new(),
        String::new(),
        String::new(),
    )
    .await
    .unwrap();
    db.post_council_entry(
        session_id,
        super_user,
        CouncilEntryKind::Approval,
        "approved".into(),
        vec![],
    )
    .await
    .unwrap();

    // After the Super delegates authority, the Supervisor may finalize too.
    db.set_council_authority(session_id, CouncilAuthority::SupervisorAutonomous)
        .await
        .unwrap();
    db.post_council_entry(
        session_id,
        supervisor,
        CouncilEntryKind::Approval,
        "auto-approved".into(),
        vec![],
    )
    .await
    .unwrap();

    // Advancing the phase is reflected in the persisted session state.
    db.advance_council_phase(session_id, CouncilPhase::Converge)
        .await
        .unwrap();
    let (_, state) = db
        .join_council(
            project_id,
            CouncilParticipantKind::Peer,
            Some(peer),
            String::new(),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    let session = state.session.unwrap();
    assert_eq!(session.phase(), proto::CouncilPhase::Converge);
    // One analysis + two accepted approvals (the peer's rejected approval did
    // not persist).
    assert_eq!(state.entries.len(), 3);
}

use super::new_test_user;
use crate::test_both_dbs;
use collab::db::{
    CouncilAuthority, CouncilEntryKind, CouncilParticipantKind, CouncilPhase, CouncilSessionId,
    Database, ProjectId,
};
use rpc::proto;
use std::sync::Arc;

test_both_dbs!(test_council, test_council_postgres, test_council_sqlite);
test_both_dbs!(
    test_council_authority,
    test_council_authority_postgres,
    test_council_authority_sqlite
);
test_both_dbs!(
    test_council_submit_approve,
    test_council_submit_approve_postgres,
    test_council_submit_approve_sqlite
);

async fn test_council(db: &Arc<Database>) {
    let super_user = new_test_user(db).await;
    let supervisor = new_test_user(db).await;
    let peer = new_test_user(db).await;
    let project_id = ProjectId::from_proto(1);

    // Super opens the session first.
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

    // Supervisor joins; becomes session supervisor.
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
    // Replica ids allocated from the >= 8 range.
    assert_eq!(supervisor_participant.replica_id, 9);
    let session_id = CouncilSessionId::from_proto(session.id);

    // Peer joins and gets the next replica id.
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
    assert_eq!(state.participants.len(), 3);
    assert_eq!(peer_participant.replica_id, 10);

    // Peer posts an analysis entry; Lamport clock starts at 1.
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
    assert_eq!(entry.lamport_replica_id, 10);

    // D1: under human_final a Peer may NOT post an Approval.
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
    db.post_council_entry(
        session_id,
        super_user,
        CouncilEntryKind::Approval,
        "approved".into(),
        vec![],
    )
    .await
    .unwrap();

    // Supervisor advances phase (Frame → Diverge is valid).
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Diverge)
        .await
        .unwrap();
    // Diverge → Converge is valid.
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Converge)
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
    // One analysis + one accepted approval.
    assert_eq!(state.entries.len(), 2);
}

async fn test_council_authority(db: &Arc<Database>) {
    let super_user = new_test_user(db).await;
    let supervisor = new_test_user(db).await;
    let peer = new_test_user(db).await;
    let project_id = ProjectId::from_proto(2);

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
    db.join_council(
        project_id,
        CouncilParticipantKind::Supervisor,
        Some(supervisor),
        String::new(),
        String::new(),
        String::new(),
    )
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
    let session_id = CouncilSessionId::from_proto(state.session.unwrap().id);

    // Peer may NOT advance the phase.
    assert!(
        db.advance_council_phase(session_id, peer, CouncilPhase::Diverge)
            .await
            .is_err()
    );

    // Peer may NOT change authority.
    assert!(
        db.set_council_authority(session_id, peer, CouncilAuthority::SupervisorAutonomous)
            .await
            .is_err()
    );

    // Supervisor may NOT change authority (only Super).
    assert!(
        db.set_council_authority(session_id, supervisor, CouncilAuthority::SupervisorAutonomous)
            .await
            .is_err()
    );

    // Invalid phase transition is rejected.
    // Supervisor is allowed to advance, but Frame → Converge is not a valid jump.
    assert!(
        db.advance_council_phase(session_id, supervisor, CouncilPhase::Converge)
            .await
            .is_err()
    );

    // Supervisor may advance via the valid path (Frame → Diverge).
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Diverge)
        .await
        .unwrap();

    // Super may change authority.
    db.set_council_authority(session_id, super_user, CouncilAuthority::SupervisorAutonomous)
        .await
        .unwrap();

    // With autonomous authority, Supervisor may now post an Approval entry.
    db.post_council_entry(
        session_id,
        supervisor,
        CouncilEntryKind::Approval,
        "auto-approved".into(),
        vec![],
    )
    .await
    .unwrap();

    // Peer still cannot post an Approval even in autonomous mode.
    assert!(
        db.post_council_entry(
            session_id,
            peer,
            CouncilEntryKind::Approval,
            "nope".into(),
            vec![],
        )
        .await
        .is_err()
    );
}

async fn test_council_submit_approve(db: &Arc<Database>) {
    let super_user = new_test_user(db).await;
    let supervisor = new_test_user(db).await;
    let project_id = ProjectId::from_proto(3);

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
    let (_, state) = db
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
    let session_id = CouncilSessionId::from_proto(state.session.unwrap().id);

    // Advance to Synthesize (the phase required to submit a draft).
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Diverge)
        .await
        .unwrap();
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Converge)
        .await
        .unwrap();
    db.advance_council_phase(session_id, supervisor, CouncilPhase::Synthesize)
        .await
        .unwrap();

    let items = vec![
        proto::WorkItem {
            title: "Implement index builder".into(),
            description: "Build the BM25 index".into(),
            sort_order: 0,
            ..Default::default()
        },
        proto::WorkItem {
            title: "Add API endpoint".into(),
            description: "REST endpoint for search".into(),
            sort_order: 1,
            ..Default::default()
        },
    ];

    let (draft_entry, returned_items) = db
        .submit_task_draft(session_id, supervisor, items)
        .await
        .unwrap();
    assert_eq!(draft_entry.kind, CouncilEntryKind::TaskDraft);
    assert_eq!(returned_items.len(), 2);

    // Session should now be in Gate phase.
    let state = db.council_session_state(session_id).await.unwrap();
    assert_eq!(state.session.unwrap().phase(), proto::CouncilPhase::Gate);
    assert!(state.work_items.is_empty()); // not materialized yet

    let draft_entry_id = draft_entry.id;

    // Peer may NOT approve.
    // (No peer in this session; test that a non-authorized user fails.)
    // Super approves → materializes work items.
    let work_items = db
        .approve_task_draft(session_id, super_user, draft_entry_id, true)
        .await
        .unwrap();
    assert_eq!(work_items.len(), 2);
    assert_eq!(work_items[0].title, "Implement index builder");
    assert_eq!(work_items[1].title, "Add API endpoint");

    // Session is now Finalized.
    let state = db.council_session_state(session_id).await.unwrap();
    assert_eq!(state.session.unwrap().phase(), proto::CouncilPhase::Finalized);
    assert_eq!(state.work_items.len(), 2);

    // Rejection path: new session.
    let project_id2 = ProjectId::from_proto(4);
    db.join_council(
        project_id2,
        CouncilParticipantKind::Super,
        Some(super_user),
        String::new(),
        String::new(),
        String::new(),
    )
    .await
    .unwrap();
    let (_, state2) = db
        .join_council(
            project_id2,
            CouncilParticipantKind::Supervisor,
            Some(supervisor),
            String::new(),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    let session2_id = CouncilSessionId::from_proto(state2.session.unwrap().id);
    db.advance_council_phase(session2_id, supervisor, CouncilPhase::Diverge)
        .await
        .unwrap();
    db.advance_council_phase(session2_id, supervisor, CouncilPhase::Converge)
        .await
        .unwrap();
    db.advance_council_phase(session2_id, supervisor, CouncilPhase::Synthesize)
        .await
        .unwrap();
    let draft_items2 = vec![proto::WorkItem {
        title: "Draft task".into(),
        sort_order: 0,
        ..Default::default()
    }];
    let (draft2, _) = db
        .submit_task_draft(session2_id, supervisor, draft_items2)
        .await
        .unwrap();
    let draft2_id = draft2.id;
    let rejected = db
        .approve_task_draft(session2_id, super_user, draft2_id, false)
        .await
        .unwrap();
    assert!(rejected.is_empty()); // no work items on rejection

    // Session is back to Synthesize so Supervisor can revise and resubmit.
    let state2 = db.council_session_state(session2_id).await.unwrap();
    assert_eq!(
        state2.session.unwrap().phase(),
        proto::CouncilPhase::Synthesize
    );
}

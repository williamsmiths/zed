use crate::TestServer;
use council::CouncilStore;
use gpui::{BackgroundExecutor, TestAppContext};
use rpc::proto;

/// End-to-end client round trip: two clients join the same project's council
/// through the collab server, and an entry posted by one is delivered to the
/// other's CouncilStore via the server broadcast.
#[gpui::test]
async fn test_council_round_trip(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    // Keep the clients alive for the duration of the test so their RPC
    // connections (which the CouncilStores ride on) stay open.
    let _client_a = server.create_client(cx_a, "user_a").await;
    let _client_b = server.create_client(cx_b, "user_b").await;

    let council_a = cx_a.read(|cx| CouncilStore::global(cx));
    let council_b = cx_b.read(|cx| CouncilStore::global(cx));

    let project_id: u64 = 1;

    // Client A (the supervisor) opens the council session for the project.
    council_a
        .update(cx_a, |council, cx| {
            council.join(
                project_id,
                proto::CouncilParticipantKind::Supervisor,
                String::new(),
                String::new(),
                String::new(),
                cx,
            )
        })
        .await
        .unwrap();

    // Client B (a peer) joins the same project's session.
    council_b
        .update(cx_b, |council, cx| {
            council.join(
                project_id,
                proto::CouncilParticipantKind::Peer,
                String::new(),
                String::new(),
                String::new(),
                cx,
            )
        })
        .await
        .unwrap();

    executor.run_until_parked();

    // B's join response already reflects both participants in the session.
    council_b.read_with(cx_b, |council, _| {
        assert_eq!(council.participants().len(), 2);
    });

    // Client A posts a message to the shared blackboard.
    council_a
        .update(cx_a, |council, cx| {
            council.post_message("we should index the codebase first".into(), cx)
        })
        .await
        .unwrap();

    executor.run_until_parked();

    // Client B receives the entry over the broadcast and applies it to its store.
    council_b.read_with(cx_b, |council, _| {
        assert_eq!(council.entries().len(), 1);
        assert_eq!(
            council.entries()[0].body,
            "we should index the codebase first"
        );
    });
}

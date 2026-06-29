use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema as acp;
use council::CouncilStore;
use gpui::{App, Entity, Task};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ui::SharedString;

/// Contribute a message to this project's shared multi-agent Council discussion.
///
/// Use this to share analysis, a proposal, or a critique with the other agents
/// and the human in the Council for the current project. Your message becomes a
/// visible entry on the shared blackboard that every participant can read. If
/// you have not joined the project's council yet, this joins it automatically.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CouncilPostToolInput {
    /// The message to contribute to the council discussion.
    pub body: String,
}

pub struct CouncilPostTool {
    council_store: Entity<CouncilStore>,
    project: Entity<Project>,
}

impl CouncilPostTool {
    pub fn new(council_store: Entity<CouncilStore>, project: Entity<Project>) -> Self {
        Self {
            council_store,
            project,
        }
    }
}

impl AgentTool for CouncilPostTool {
    type Input = CouncilPostToolInput;
    type Output = String;

    const NAME: &'static str = "post_to_council";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        "Post to council".into()
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let council_store = self.council_store.clone();
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|err| err.to_string())?;

            let project_id = project
                .read_with(cx, |project, _| project.remote_id())
                .ok_or_else(|| {
                    "Share this project (start collaborating) before posting to the council."
                        .to_string()
                })?;

            let already_joined = council_store.read_with(cx, |council, _| {
                council.session().map(|session| session.project_id) == Some(project_id)
            });
            if !already_joined {
                let join =
                    council_store.update(cx, |council, cx| council.join_for_project(project_id, cx));
                join.await.map_err(|err| err.to_string())?;
            }

            let post =
                council_store.update(cx, |council, cx| council.post_message(input.body, cx));
            post.await.map_err(|err| err.to_string())?;
            Ok("Posted your message to the council.".to_string())
        })
    }
}

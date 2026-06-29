use crate::db::{CouncilParticipantId, CouncilParticipantKind, CouncilSessionId, UserId};
use rpc::proto;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "council_participants")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: CouncilParticipantId,
    pub session_id: CouncilSessionId,
    pub kind: CouncilParticipantKind,
    pub user_id: Option<UserId>,
    pub agent_label: String,
    pub model: String,
    pub tool: String,
    pub replica_id: i32,
    pub active: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for proto::CouncilParticipant {
    fn from(participant: Model) -> Self {
        Self {
            id: participant.id.to_proto(),
            session_id: participant.session_id.to_proto(),
            kind: proto::CouncilParticipantKind::from(participant.kind) as i32,
            user_id: participant.user_id.map(|id| id.to_proto()),
            agent_label: participant.agent_label,
            model: participant.model,
            tool: participant.tool,
            replica_id: participant.replica_id as u32,
            active: participant.active,
        }
    }
}

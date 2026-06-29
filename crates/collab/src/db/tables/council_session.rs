use crate::db::{CouncilAuthority, CouncilParticipantId, CouncilPhase, CouncilSessionId, ProjectId};
use rpc::proto;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "council_sessions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: CouncilSessionId,
    pub project_id: ProjectId,
    pub supervisor_participant_id: Option<CouncilParticipantId>,
    pub phase: CouncilPhase,
    pub round: i32,
    pub authority: CouncilAuthority,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for proto::CouncilSession {
    fn from(session: Model) -> Self {
        Self {
            id: session.id.to_proto(),
            project_id: session.project_id.to_proto(),
            supervisor_participant_id: session.supervisor_participant_id.map(|id| id.to_proto()),
            phase: proto::CouncilPhase::from(session.phase) as i32,
            round: session.round as u32,
            authority: proto::CouncilAuthority::from(session.authority) as i32,
        }
    }
}

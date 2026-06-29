use crate::db::{CouncilEntryId, CouncilEntryKind, CouncilParticipantId, CouncilSessionId};
use rpc::proto;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "council_entries")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: CouncilEntryId,
    pub session_id: CouncilSessionId,
    pub author_participant_id: CouncilParticipantId,
    pub lamport_value: i32,
    pub lamport_replica_id: i32,
    pub kind: CouncilEntryKind,
    pub body: String,
    pub refs: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for proto::CouncilEntry {
    fn from(entry: Model) -> Self {
        Self {
            id: entry.id.to_proto(),
            session_id: entry.session_id.to_proto(),
            author_participant_id: entry.author_participant_id.to_proto(),
            lamport_value: entry.lamport_value as u32,
            lamport_replica_id: entry.lamport_replica_id as u32,
            kind: proto::CouncilEntryKind::from(entry.kind) as i32,
            body: entry.body,
            refs: serde_json::from_str(&entry.refs).unwrap_or_default(),
        }
    }
}

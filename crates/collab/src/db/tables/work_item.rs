use crate::db::{
    CouncilEntryId, CouncilParticipantId, CouncilSessionId, ProjectId, WorkItemId, WorkItemStatus,
};
use rpc::proto;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "work_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: WorkItemId,
    pub project_id: ProjectId,
    pub session_id: CouncilSessionId,
    pub source_entry_id: Option<CouncilEntryId>,
    pub title: String,
    pub description: String,
    pub status: WorkItemStatus,
    pub assignee_participant_id: Option<CouncilParticipantId>,
    pub parent_id: Option<WorkItemId>,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for proto::WorkItem {
    fn from(item: Model) -> Self {
        Self {
            id: item.id.to_proto(),
            project_id: item.project_id.to_proto(),
            session_id: item.session_id.to_proto(),
            source_entry_id: item.source_entry_id.map(|id| id.to_proto()),
            title: item.title,
            description: item.description,
            status: proto::WorkItemStatus::from(item.status) as i32,
            assignee_participant_id: item.assignee_participant_id.map(|id| id.to_proto()),
            parent_id: item.parent_id.map(|id| id.to_proto()),
            sort_order: item.sort_order,
        }
    }
}

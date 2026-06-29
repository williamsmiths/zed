use crate::Result;
use rpc::proto;
use sea_orm::{DbErr, entity::prelude::*};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[macro_export]
macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            DeriveValueType,
        )]
        #[allow(missing_docs)]
        #[serde(transparent)]
        pub struct $name(pub i32);

        impl $name {
            #[allow(unused)]
            #[allow(missing_docs)]
            pub const MAX: Self = Self(i32::MAX);

            #[allow(unused)]
            #[allow(missing_docs)]
            pub fn from_proto(value: u64) -> Self {
                debug_assert!(value != 0);
                Self(value as i32)
            }

            #[allow(unused)]
            #[allow(missing_docs)]
            pub fn to_proto(self) -> u64 {
                self.0 as u64
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl sea_orm::TryFromU64 for $name {
            fn try_from_u64(n: u64) -> Result<Self, DbErr> {
                Ok(Self(n.try_into().map_err(|_| {
                    DbErr::ConvertFromU64(concat!(
                        "error converting ",
                        stringify!($name),
                        " to u64"
                    ))
                })?))
            }
        }

        impl sea_orm::sea_query::Nullable for $name {
            fn null() -> Value {
                Value::Int(None)
            }
        }
    };
}

id_type!(BufferId);
id_type!(ChannelBufferCollaboratorId);
id_type!(ChannelChatParticipantId);
id_type!(ChannelId);
id_type!(ChannelMemberId);
id_type!(ContactId);
id_type!(ExtensionId);
id_type!(FlagId);
id_type!(FollowerId);
id_type!(HostedProjectId);
id_type!(MessageId);
id_type!(NotificationId);
id_type!(NotificationKindId);
id_type!(ProjectCollaboratorId);
id_type!(ProjectId);
id_type!(ReplicaId);
id_type!(RoomId);
id_type!(RoomParticipantId);
id_type!(ServerId);
id_type!(SignupId);
id_type!(UserId);
id_type!(CouncilSessionId);
id_type!(CouncilParticipantId);
id_type!(CouncilEntryId);
id_type!(WorkItemId);

/// The lifecycle phase of a council session.
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Hash, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum CouncilPhase {
    #[sea_orm(string_value = "frame")]
    Frame,
    #[sea_orm(string_value = "diverge")]
    Diverge,
    #[sea_orm(string_value = "converge")]
    Converge,
    #[sea_orm(string_value = "synthesize")]
    Synthesize,
    #[sea_orm(string_value = "gate")]
    Gate,
    #[sea_orm(string_value = "finalized")]
    Finalized,
}

impl From<proto::CouncilPhase> for CouncilPhase {
    fn from(value: proto::CouncilPhase) -> Self {
        match value {
            proto::CouncilPhase::Frame => CouncilPhase::Frame,
            proto::CouncilPhase::Diverge => CouncilPhase::Diverge,
            proto::CouncilPhase::Converge => CouncilPhase::Converge,
            proto::CouncilPhase::Synthesize => CouncilPhase::Synthesize,
            proto::CouncilPhase::Gate => CouncilPhase::Gate,
            proto::CouncilPhase::Finalized => CouncilPhase::Finalized,
        }
    }
}

impl From<CouncilPhase> for proto::CouncilPhase {
    fn from(value: CouncilPhase) -> Self {
        match value {
            CouncilPhase::Frame => proto::CouncilPhase::Frame,
            CouncilPhase::Diverge => proto::CouncilPhase::Diverge,
            CouncilPhase::Converge => proto::CouncilPhase::Converge,
            CouncilPhase::Synthesize => proto::CouncilPhase::Synthesize,
            CouncilPhase::Gate => proto::CouncilPhase::Gate,
            CouncilPhase::Finalized => proto::CouncilPhase::Finalized,
        }
    }
}

/// Who has final authority in a council session.
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Hash, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum CouncilAuthority {
    #[sea_orm(string_value = "human_final")]
    HumanFinal,
    #[sea_orm(string_value = "supervisor_autonomous")]
    SupervisorAutonomous,
}

impl From<proto::CouncilAuthority> for CouncilAuthority {
    fn from(value: proto::CouncilAuthority) -> Self {
        match value {
            proto::CouncilAuthority::HumanFinal => CouncilAuthority::HumanFinal,
            proto::CouncilAuthority::SupervisorAutonomous => {
                CouncilAuthority::SupervisorAutonomous
            }
        }
    }
}

impl From<CouncilAuthority> for proto::CouncilAuthority {
    fn from(value: CouncilAuthority) -> Self {
        match value {
            CouncilAuthority::HumanFinal => proto::CouncilAuthority::HumanFinal,
            CouncilAuthority::SupervisorAutonomous => {
                proto::CouncilAuthority::SupervisorAutonomous
            }
        }
    }
}

/// The role a participant plays in a council session.
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Hash, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum CouncilParticipantKind {
    #[sea_orm(string_value = "super")]
    Super,
    #[sea_orm(string_value = "supervisor")]
    Supervisor,
    #[sea_orm(string_value = "peer")]
    Peer,
}

impl From<proto::CouncilParticipantKind> for CouncilParticipantKind {
    fn from(value: proto::CouncilParticipantKind) -> Self {
        match value {
            proto::CouncilParticipantKind::Super => CouncilParticipantKind::Super,
            proto::CouncilParticipantKind::Supervisor => CouncilParticipantKind::Supervisor,
            proto::CouncilParticipantKind::Peer => CouncilParticipantKind::Peer,
        }
    }
}

impl From<CouncilParticipantKind> for proto::CouncilParticipantKind {
    fn from(value: CouncilParticipantKind) -> Self {
        match value {
            CouncilParticipantKind::Super => proto::CouncilParticipantKind::Super,
            CouncilParticipantKind::Supervisor => proto::CouncilParticipantKind::Supervisor,
            CouncilParticipantKind::Peer => proto::CouncilParticipantKind::Peer,
        }
    }
}

/// The kind of a council entry (one message on the shared blackboard).
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Hash, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum CouncilEntryKind {
    #[sea_orm(string_value = "directive")]
    Directive,
    #[sea_orm(string_value = "analysis")]
    Analysis,
    #[sea_orm(string_value = "critique")]
    Critique,
    #[sea_orm(string_value = "proposal")]
    Proposal,
    #[sea_orm(string_value = "task_draft")]
    TaskDraft,
    #[sea_orm(string_value = "approval")]
    Approval,
}

impl From<proto::CouncilEntryKind> for CouncilEntryKind {
    fn from(value: proto::CouncilEntryKind) -> Self {
        match value {
            proto::CouncilEntryKind::Directive => CouncilEntryKind::Directive,
            proto::CouncilEntryKind::Analysis => CouncilEntryKind::Analysis,
            proto::CouncilEntryKind::Critique => CouncilEntryKind::Critique,
            proto::CouncilEntryKind::Proposal => CouncilEntryKind::Proposal,
            proto::CouncilEntryKind::TaskDraft => CouncilEntryKind::TaskDraft,
            proto::CouncilEntryKind::Approval => CouncilEntryKind::Approval,
        }
    }
}

impl From<CouncilEntryKind> for proto::CouncilEntryKind {
    fn from(value: CouncilEntryKind) -> Self {
        match value {
            CouncilEntryKind::Directive => proto::CouncilEntryKind::Directive,
            CouncilEntryKind::Analysis => proto::CouncilEntryKind::Analysis,
            CouncilEntryKind::Critique => proto::CouncilEntryKind::Critique,
            CouncilEntryKind::Proposal => proto::CouncilEntryKind::Proposal,
            CouncilEntryKind::TaskDraft => proto::CouncilEntryKind::TaskDraft,
            CouncilEntryKind::Approval => proto::CouncilEntryKind::Approval,
        }
    }
}

/// The status of a work item.
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Hash, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum WorkItemStatus {
    #[sea_orm(string_value = "todo")]
    Todo,
    #[sea_orm(string_value = "in_progress")]
    InProgress,
    #[sea_orm(string_value = "blocked")]
    Blocked,
    #[sea_orm(string_value = "done")]
    Done,
    #[sea_orm(string_value = "cancelled")]
    Cancelled,
}

impl From<proto::WorkItemStatus> for WorkItemStatus {
    fn from(value: proto::WorkItemStatus) -> Self {
        match value {
            proto::WorkItemStatus::Todo => WorkItemStatus::Todo,
            proto::WorkItemStatus::InProgress => WorkItemStatus::InProgress,
            proto::WorkItemStatus::Blocked => WorkItemStatus::Blocked,
            proto::WorkItemStatus::Done => WorkItemStatus::Done,
            proto::WorkItemStatus::Cancelled => WorkItemStatus::Cancelled,
        }
    }
}

impl From<WorkItemStatus> for proto::WorkItemStatus {
    fn from(value: WorkItemStatus) -> Self {
        match value {
            WorkItemStatus::Todo => proto::WorkItemStatus::Todo,
            WorkItemStatus::InProgress => proto::WorkItemStatus::InProgress,
            WorkItemStatus::Blocked => proto::WorkItemStatus::Blocked,
            WorkItemStatus::Done => proto::WorkItemStatus::Done,
            WorkItemStatus::Cancelled => proto::WorkItemStatus::Cancelled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValueType)]
pub struct SharedThreadId(pub Uuid);

impl SharedThreadId {
    pub fn from_proto(id: String) -> Option<Self> {
        Uuid::parse_str(&id).ok().map(SharedThreadId)
    }

    pub fn to_proto(self) -> String {
        self.0.to_string()
    }
}

impl sea_orm::TryFromU64 for SharedThreadId {
    fn try_from_u64(_n: u64) -> std::result::Result<Self, DbErr> {
        Err(DbErr::ConvertFromU64(
            "SharedThreadId uses UUID and cannot be converted from u64",
        ))
    }
}

impl sea_orm::sea_query::Nullable for SharedThreadId {
    fn null() -> Value {
        Value::Uuid(None)
    }
}

impl std::fmt::Display for SharedThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// ChannelRole gives you permissions for both channels and calls.
#[derive(
    Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Default, Hash, Serialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum ChannelRole {
    /// Admin can read/write and change permissions.
    #[sea_orm(string_value = "admin")]
    Admin,
    /// Member can read/write, but not change permissions.
    #[sea_orm(string_value = "member")]
    #[default]
    Member,
    /// Talker can read, but not write.
    /// They can use microphones and the channel chat
    #[sea_orm(string_value = "talker")]
    Talker,
    /// Guest can read, but not write.
    /// They can not use microphones but can use the chat.
    #[sea_orm(string_value = "guest")]
    Guest,
    /// Banned may not read.
    #[sea_orm(string_value = "banned")]
    Banned,
}

impl ChannelRole {
    /// Returns true if this role is more powerful than the other role.
    pub fn should_override(&self, other: Self) -> bool {
        use ChannelRole::*;
        match self {
            Admin => matches!(other, Member | Banned | Talker | Guest),
            Member => matches!(other, Banned | Talker | Guest),
            Talker => matches!(other, Guest),
            Banned => matches!(other, Guest),
            Guest => false,
        }
    }

    /// Returns the maximal role between the two
    pub fn max(&self, other: Self) -> Self {
        if self.should_override(other) {
            *self
        } else {
            other
        }
    }

    pub fn can_see_channel(&self, visibility: ChannelVisibility) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member => true,
            Guest | Talker => visibility == ChannelVisibility::Public,
            Banned => false,
        }
    }

    /// True if the role allows access to all descendant channels
    pub fn can_see_all_descendants(&self) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member => true,
            Guest | Talker | Banned => false,
        }
    }

    /// True if the role only allows access to public descendant channels
    pub fn can_only_see_public_descendants(&self) -> bool {
        use ChannelRole::*;
        match self {
            Guest | Talker => true,
            Admin | Member | Banned => false,
        }
    }

    /// True if the role can share screen/microphone/projects into rooms.
    pub fn can_use_microphone(&self) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member | Talker => true,
            Guest | Banned => false,
        }
    }

    /// True if the role can edit shared projects.
    pub fn can_edit_projects(&self) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member => true,
            Talker | Guest | Banned => false,
        }
    }

    /// True if the role can read shared projects.
    pub fn can_read_projects(&self) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member | Guest | Talker => true,
            Banned => false,
        }
    }

    pub fn requires_cla(&self) -> bool {
        use ChannelRole::*;
        match self {
            Admin | Member => true,
            Banned | Guest | Talker => false,
        }
    }
}

impl From<proto::ChannelRole> for ChannelRole {
    fn from(value: proto::ChannelRole) -> Self {
        match value {
            proto::ChannelRole::Admin => ChannelRole::Admin,
            proto::ChannelRole::Member => ChannelRole::Member,
            proto::ChannelRole::Talker => ChannelRole::Talker,
            proto::ChannelRole::Guest => ChannelRole::Guest,
            proto::ChannelRole::Banned => ChannelRole::Banned,
        }
    }
}

impl From<ChannelRole> for proto::ChannelRole {
    fn from(val: ChannelRole) -> Self {
        match val {
            ChannelRole::Admin => proto::ChannelRole::Admin,
            ChannelRole::Member => proto::ChannelRole::Member,
            ChannelRole::Talker => proto::ChannelRole::Talker,
            ChannelRole::Guest => proto::ChannelRole::Guest,
            ChannelRole::Banned => proto::ChannelRole::Banned,
        }
    }
}

impl From<ChannelRole> for i32 {
    fn from(val: ChannelRole) -> Self {
        let proto: proto::ChannelRole = val.into();
        proto.into()
    }
}

/// ChannelVisibility controls whether channels are public or private.
#[derive(Eq, PartialEq, Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Default, Hash)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum ChannelVisibility {
    /// Public channels are visible to anyone with the link. People join with the Guest role by default.
    #[sea_orm(string_value = "public")]
    Public,
    /// Members channels are only visible to members of this channel or its parents.
    #[sea_orm(string_value = "members")]
    #[default]
    Members,
}

impl From<proto::ChannelVisibility> for ChannelVisibility {
    fn from(value: proto::ChannelVisibility) -> Self {
        match value {
            proto::ChannelVisibility::Public => ChannelVisibility::Public,
            proto::ChannelVisibility::Members => ChannelVisibility::Members,
        }
    }
}

impl From<ChannelVisibility> for proto::ChannelVisibility {
    fn from(val: ChannelVisibility) -> Self {
        match val {
            ChannelVisibility::Public => proto::ChannelVisibility::Public,
            ChannelVisibility::Members => proto::ChannelVisibility::Members,
        }
    }
}

impl From<ChannelVisibility> for i32 {
    fn from(val: ChannelVisibility) -> Self {
        let proto: proto::ChannelVisibility = val.into();
        proto.into()
    }
}

/// Indicate whether a [Buffer] has permissions to edit.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Capability {
    /// The buffer is a mutable replica.
    ReadWrite,
    /// The buffer is a read-only replica.
    ReadOnly,
}

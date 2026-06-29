use council::{CouncilEvent, CouncilStore};
use gpui::{
    Action, AsyncWindowContext, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Subscription, WeakEntity, actions,
};
use rpc::proto;
use ui::{Color, IconName, Label, LabelSize, prelude::*};
use workspace::{
    Panel, Workspace,
    dock::{DockPosition, PanelEvent},
};

const WORK_ITEM_PANEL_KEY: &str = "WorkItemPanel";

actions!(council_ui, [ToggleWorkItemPanel]);

pub fn init(cx: &mut App) {
    cx.observe_new::<Workspace>(move |workspace, window, cx| {
        let Some(window) = window else { return };
        let Some(council) = CouncilStore::try_global(cx) else { return };
        let panel = cx.new(|cx| {
            let focus_handle = cx.focus_handle();
            let subscriptions =
                vec![cx.subscribe(&council, |_, _, event: &CouncilEvent, cx| {
                    if matches!(event, CouncilEvent::WorkItemsChanged | CouncilEvent::Joined) {
                        cx.notify();
                    }
                })];
            WorkItemPanel {
                council_store: council,
                focus_handle,
                _subscriptions: subscriptions,
            }
        });
        workspace.add_panel(panel, window, cx);
    })
    .detach();
}

pub struct WorkItemPanel {
    council_store: Entity<CouncilStore>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl WorkItemPanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |_workspace, _window, cx| {
            let council = CouncilStore::try_global(cx)
                .ok_or_else(|| anyhow::anyhow!("CouncilStore not initialized"))?;
            let panel = cx.new(|cx| {
                let focus_handle = cx.focus_handle();
                let subscriptions =
                    vec![cx.subscribe(&council, |_, _, event: &CouncilEvent, cx| {
                        if matches!(event, CouncilEvent::WorkItemsChanged | CouncilEvent::Joined) {
                            cx.notify();
                        }
                    })];
                WorkItemPanel {
                    council_store: council,
                    focus_handle,
                        _subscriptions: subscriptions,
                }
            });
            anyhow::Ok(panel)
        })?
    }
}

impl EventEmitter<PanelEvent> for WorkItemPanel {}

impl Focusable for WorkItemPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WorkItemPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.council_store.read(cx).work_items().to_vec();
        let grouped = group_by_status(&items);

        v_flex()
            .id("work-items-panel")
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().colors().panel_background)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(
                        Label::new("Work Items")
                            .size(LabelSize::Default)
                            .weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .when(items.is_empty(), |el| {
                        el.child(
                            div().p_4().child(
                                Label::new(
                                    "No work items yet — approve a task draft to populate this panel.",
                                )
                                .color(Color::Muted),
                            ),
                        )
                    })
                    .children(grouped.into_iter().map(|(status_label, group_items)| {
                        v_flex()
                            .px_2()
                            .pt_2()
                            .gap_1()
                            .child(
                                Label::new(status_label)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .weight(FontWeight::SEMIBOLD),
                            )
                            .children(group_items.into_iter().map(|item| {
                                h_flex()
                                    .gap_2()
                                    .px_1()
                                    .py_1()
                                    .rounded_sm()
                                    .child(
                                        div()
                                            .size_3()
                                            .rounded_full()
                                            .bg(status_color(item.status(), cx)),
                                    )
                                    .child(Label::new(item.title.clone()))
                            }))
                    })),
            )
    }
}

fn group_by_status(items: &[proto::WorkItem]) -> Vec<(SharedString, Vec<&proto::WorkItem>)> {
    let order = [
        (proto::WorkItemStatus::InProgress, "In Progress"),
        (proto::WorkItemStatus::Todo, "To Do"),
        (proto::WorkItemStatus::Blocked, "Blocked"),
        (proto::WorkItemStatus::Done, "Done"),
        (proto::WorkItemStatus::Cancelled, "Cancelled"),
    ];
    order
        .iter()
        .filter_map(|(status, label)| {
            let group: Vec<_> = items
                .iter()
                .filter(|item| item.status() == *status)
                .collect();
            if group.is_empty() {
                None
            } else {
                Some((SharedString::from(*label), group))
            }
        })
        .collect()
}

fn status_color(status: proto::WorkItemStatus, cx: &App) -> gpui::Hsla {
    let s = cx.theme().status();
    match status {
        proto::WorkItemStatus::InProgress => s.success,
        proto::WorkItemStatus::Todo => cx.theme().colors().border,
        proto::WorkItemStatus::Blocked => s.error,
        proto::WorkItemStatus::Done => s.success.opacity(0.6),
        proto::WorkItemStatus::Cancelled => cx.theme().colors().text_disabled,
    }
}

impl Panel for WorkItemPanel {
    fn persistent_name() -> &'static str {
        WORK_ITEM_PANEL_KEY
    }

    fn panel_key() -> &'static str {
        WORK_ITEM_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(260.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Check)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Work Items")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleWorkItemPanel)
    }

    fn activation_priority(&self) -> u32 {
        6
    }
}

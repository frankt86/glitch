use crate::permissions::PendingApproval;
use dioxus::prelude::*;
use glitch_mcp::pipe::ApprovalDecision;

#[component]
pub fn PermissionModal(
    pending: Signal<Vec<PendingApproval>>,
    on_decision: EventHandler<(String, ApprovalDecision)>,
) -> Element {
    let next = pending.read().first().cloned();
    let Some(approval) = next else {
        return rsx! { Fragment {} };
    };

    let queue_count = pending.read().len();
    let id_allow = approval.id.clone();
    let id_deny = approval.id.clone();

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal-card",
                header { class: "modal-header",
                    span { class: "modal-eyebrow", "Claude wants to use:" }
                    span { class: "modal-tool", "{approval.tool_name}" }
                    if queue_count > 1 {
                        span { class: "modal-queue", "+{queue_count - 1} queued" }
                    }
                }
                div { class: "modal-summary", "{approval.summary}" }
                details { class: "modal-input",
                    summary { "input" }
                    pre { class: "modal-input-body", "{approval.input_pretty}" }
                }
                footer { class: "modal-actions",
                    button {
                        class: "btn",
                        onclick: move |_| {
                            on_decision.call((id_deny.clone(), ApprovalDecision::deny("user denied")))
                        },
                        "Deny"
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            on_decision.call((id_allow.clone(), ApprovalDecision::allow_unchanged()))
                        },
                        "Allow"
                    }
                }
            }
        }
    }
}

//! Markdown panel V2 handler.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};

use super::helpers::optional_uuid;
use super::Response;

pub(super) fn handle_markdown_open(
    id: Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Response {
    let Some(file_path) = params.get("file").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Provide 'file'");
    };
    let workspace_id = match optional_uuid(&id, params, "workspace_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let panel = crate::model::panel::Panel::new_markdown(file_path);
    let panel_id = panel.id;

    let mut tm = lock_or_recover(&state.tab_manager);
    let ws_id = workspace_id.unwrap_or_else(|| tm.selected().map(|ws| ws.id).unwrap_or_default());

    if let Some(ws) = tm.workspace_mut(ws_id) {
        ws.panels.insert(panel_id, panel);
        if let Some(focused) = ws.focused_panel_id {
            ws.layout.add_panel_to_pane(focused, panel_id);
        } else {
            let first_panel = ws.layout.all_panel_ids().into_iter().next();
            if let Some(target) = first_panel {
                ws.layout.add_panel_to_pane(target, panel_id);
            }
        }
        ws.previous_focused_panel_id = ws.focused_panel_id;
        ws.focused_panel_id = Some(panel_id);
    } else {
        return Response::error(id, "not_found", "Workspace not found");
    }
    drop(tm);
    state.notify_ui_refresh();

    Response::success(
        id,
        serde_json::json!({
            "panel_id": panel_id.to_string(),
            "file": file_path,
        }),
    )
}

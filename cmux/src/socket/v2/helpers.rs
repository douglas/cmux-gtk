//! Shared parameter-parsing helpers for V2 handlers.

use std::sync::Arc;

use serde_json::Value;

use crate::app::{lock_or_recover, SharedState};

use super::Response;

/// Resolve a panel UUID from `panel` or `surface` params, falling back to the
/// focused panel in the selected workspace.
pub(super) fn resolve_panel_id(
    id: &Value,
    params: &Value,
    state: &Arc<SharedState>,
) -> Result<uuid::Uuid, Response> {
    let panel_str = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .and_then(|v| v.as_str());

    if let Some(s) = panel_str {
        uuid::Uuid::parse_str(s)
            .map_err(|_| Response::error(id.clone(), "invalid_params", "Invalid panel UUID"))
    } else {
        let tm = lock_or_recover(&state.tab_manager);
        let ws = tm
            .selected()
            .ok_or_else(|| Response::error(id.clone(), "not_found", "No workspace selected"))?;
        ws.focused_panel_id
            .ok_or_else(|| Response::error(id.clone(), "not_found", "No focused panel"))
    }
}

pub(super) fn mark_workspace_read(state: &Arc<SharedState>, workspace_id: uuid::Uuid) {
    lock_or_recover(&state.notifications).mark_workspace_read(workspace_id);

    if let Some(workspace) = lock_or_recover(&state.tab_manager).workspace_mut(workspace_id) {
        workspace.mark_notifications_read();
    }
}

/// Parse a workspace UUID from `workspace` or `workspace_id` params.
/// Returns `Err(())` if the key exists but the value is not a valid UUID.
/// Returns `Ok(None)` if neither key is present.
pub(super) fn parse_workspace_param(params: &Value) -> Result<Option<uuid::Uuid>, ()> {
    let val = params
        .get("workspace")
        .or_else(|| params.get("workspace_id"));
    match val {
        Some(v) if v.is_null() => Ok(None),
        Some(v) => match v.as_str().map(uuid::Uuid::parse_str) {
            Some(Ok(id)) => Ok(Some(id)),
            _ => Err(()),
        },
        None => Ok(None),
    }
}

pub(super) fn parse_usize_param(
    id: &Value,
    params: &Value,
    key: &str,
) -> Result<Option<usize>, Response> {
    match params.get(key) {
        Some(v) => match v.as_u64() {
            Some(value) => usize::try_from(value).map(Some).map_err(|_| {
                Response::error(
                    id.clone(),
                    "invalid_params",
                    &format!("'{key}' is out of range"),
                )
            }),
            None => Err(Response::error(
                id.clone(),
                "invalid_params",
                &format!("'{key}' must be a non-negative integer"),
            )),
        },
        None => Ok(None),
    }
}

/// Extract a required panel_id from params (checks "panel", "surface", "panel_id" keys).
pub(crate) fn require_panel_id(id: &Value, params: &Value) -> Result<uuid::Uuid, Response> {
    let val = params
        .get("panel")
        .or_else(|| params.get("surface"))
        .or_else(|| params.get("panel_id"));
    match val {
        Some(v) if !v.is_null() => {
            let s = v.as_str().ok_or_else(|| {
                Response::error(id.clone(), "invalid_params", "panel must be a string UUID")
            })?;
            uuid::Uuid::parse_str(s).map_err(|_| {
                Response::error(id.clone(), "invalid_params", "Invalid panel UUID format")
            })
        }
        _ => Err(Response::error(
            id.clone(),
            "invalid_params",
            "Provide 'panel' UUID",
        )),
    }
}

/// Extract an optional UUID parameter.
pub(super) fn optional_uuid(
    id: &Value,
    params: &Value,
    key: &str,
) -> Result<Option<uuid::Uuid>, Response> {
    match params.get(key) {
        Some(v) if !v.is_null() => {
            let s = v.as_str().ok_or_else(|| {
                Response::error(
                    id.clone(),
                    "invalid_params",
                    &format!("'{key}' must be a string UUID"),
                )
            })?;
            uuid::Uuid::parse_str(s).map(Some).map_err(|_| {
                Response::error(
                    id.clone(),
                    "invalid_params",
                    &format!("Invalid UUID for '{key}'"),
                )
            })
        }
        _ => Ok(None),
    }
}

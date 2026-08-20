//! `graph.*` socket methods — thin wrappers over `goal::graph` operations.

use std::sync::Arc;

use serde_json::Value;

use super::Response;
use crate::app::SharedState;
use crate::goal::graph;

fn name_param<'a>(params: &'a Value) -> Result<&'a str, ()> {
    params
        .get("name")
        .or_else(|| params.get("graph"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or(())
}

fn wrap(id: Value, result: Result<serde_json::Value, String>) -> Response {
    match result {
        Ok(v) => Response::success(id, v),
        Err(e) => Response::error(id, "invalid_params", &e),
    }
}

pub(super) fn handle_graph_create(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    // `goal` (a .md file) or `goal_text` (inline) — see resolve_goal_source.
    let source = match crate::goal::resolve_goal_source(params) {
        Ok(s) => s,
        Err((code, msg)) => return Response::error(id, code, &msg),
    };
    // Name: explicit, else the repo directory (goal file) or a slug of the
    // goal text (inline).
    let name = name_param(params).map(str::to_string).unwrap_or_else(|_| {
        if source.inline {
            source.name.clone()
        } else {
            source.repo_name.clone()
        }
    });
    wrap(id, graph::create_graph(state, &name, &source, params))
}

pub(super) fn handle_graph_approve(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    wrap(id, graph::approve_graph(state, name))
}

pub(super) fn handle_graph_revise(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    let note = params.get("note").and_then(|v| v.as_str()).unwrap_or("");
    if note.is_empty() {
        return Response::error(id, "invalid_params", "Missing 'note' (what to change)");
    }
    wrap(id, graph::revise_graph(state, name, note))
}

pub(super) fn handle_graph_status(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let name = params
        .get("name")
        .or_else(|| params.get("graph"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    wrap(id, graph::graph_status(state, name))
}

pub(super) fn handle_graph_pause(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    wrap(id, graph::pause_graph(state, name))
}

pub(super) fn handle_graph_resume(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    wrap(id, graph::resume_graph(state, name))
}

pub(super) fn handle_graph_stop(id: Value, params: &Value, state: &Arc<SharedState>) -> Response {
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    wrap(id, graph::stop_graph(state, name))
}

//! `graph.*` socket methods — thin wrappers over `goal::graph` operations.

use std::path::Path;
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
    let Ok(name) = name_param(params) else {
        return Response::error(id, "invalid_params", "Missing graph name");
    };
    let Some(goal_path) = params.get("goal").and_then(|v| v.as_str()) else {
        return Response::error(id, "invalid_params", "Missing 'goal' (path to a goal .md file)");
    };
    let goal_path = Path::new(goal_path);
    if !goal_path.is_absolute() {
        return Response::error(id, "invalid_params", "'goal' must be an absolute path");
    }
    wrap(id, graph::create_graph(state, name, goal_path, params))
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

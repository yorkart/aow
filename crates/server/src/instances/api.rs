use super::*;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

pub(crate) async fn list(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    Ok(Json(
        json!({"items":graph(&state)?.into_values().collect::<Vec<_>>()}),
    ))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let reference = reference.parse::<InstanceRef>().map_err(invalid)?;
    let graph = graph(&state)?;
    let instance = graph.get(&reference).ok_or_else(|| missing(&reference))?;
    let mut ancestors = Vec::new();
    let mut current = instance.parent.as_ref();
    let mut visited = BTreeSet::from([&reference]);
    while let Some(id) = current {
        if !visited.insert(id) {
            return Err(invalid("instance ancestry contains a cycle"));
        }
        let item = graph.get(id).ok_or_else(|| missing(id))?;
        ancestors.push(item);
        current = item.parent.as_ref();
    }
    let children: Vec<_> = graph
        .values()
        .filter(|item| item.parent.as_ref() == Some(&reference))
        .collect();
    Ok(Json(
        json!({"instance":instance, "ancestors":ancestors, "children":children}),
    ))
}

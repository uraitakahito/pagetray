//! ページの本文を返す。URL は、前の置き場（Nginx）と同じ形。

use crate::api::is_segment;
use crate::{AppState, error, internal};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
pub use pagetray_api::is_page_path;
use pagetray_api::parse_file;

pub async fn show(
    State(state): State<AppState>,
    Path((project, key, file)): Path<(String, String, String)>,
) -> Response {
    let Some((kind, n)) = parse_file(&file).filter(|_| is_segment(&project) && is_segment(&key))
    else {
        return not_found();
    };
    match state.db.body(&project, &key, kind, n).await {
        Ok(Some(body)) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                // 隔てた枠として出す。ページの中の script は動くが、置き場の名で API を呼べない。
                (header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            body,
        )
            .into_response(),
        Ok(None) => match state.db.removed_at(&project, &key, kind, n).await {
            Ok(Some(at)) => error(StatusCode::GONE, &format!("そのページは消した（{at}）")),
            Ok(None) => not_found(),
            Err(e) => internal(e),
        },
        Err(e) => internal(e),
    }
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "そのページは無い")
}

//! file の中身を返す（`/<project>/<key>/files/<語>-<n>.<拡張子>`）。

use crate::api::is_segment;
use crate::{AppState, error, internal};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use pagetray_api::parse_file_name;

pub async fn show(
    State(state): State<AppState>,
    Path((project, key, file)): Path<(String, String, String)>,
) -> Response {
    let Some((name, ext, n)) =
        parse_file_name(&file).filter(|_| is_segment(&project) && is_segment(&key))
    else {
        return not_found();
    };
    match state.db.file_body(&project, &key, name, ext, n).await {
        Ok(Some((content_type, body))) => (
            [
                (header::CONTENT_TYPE, content_type.as_str()),
                // 直に開いても、中の script は動かさない。
                (header::CONTENT_SECURITY_POLICY, "sandbox"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                // 同じ URL の中身は書き換えない（上げ直すと次の番号になる）。
                (header::CACHE_CONTROL, "max-age=31536000, immutable"),
            ],
            body,
        )
            .into_response(),
        Ok(None) => match state.db.file_removed_at(&project, &key, name, ext, n).await {
            Ok(Some(at)) => error(StatusCode::GONE, &format!("その file は消した（{at}）")),
            Ok(None) => not_found(),
            Err(e) => internal(e),
        },
        Err(e) => internal(e),
    }
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "その file は無い")
}

//! 操作に共通の誤りの作り方（castlist の新しい server の crates/server/src/errors.rs と同じ形）。
use axum::http::{HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use pagetray_sdk::error::{
    Conflict, CreatePageError, InternalError, Unavailable, ValidationException,
};

use pagetray_sdk::model::ValidationExceptionField;

use crate::db::WriteError;

/// server の中の失敗（DB の誤りなど）。訳は記録に書き、答えは決まった字にする（中の誤りの字を外に出さない）。
pub fn internal(operation: &str, error: &dyn std::fmt::Display) -> InternalError {
    tracing::error!(operation, %error, "server の中で失敗した");
    InternalError {
        message: "pagetray の中で失敗した（記録を見る）".to_owned(),
    }
}

/// 生成した層の外（守りの層・手の道）で返す誤りの答え。生成した層の誤りと同じ restJson1 の形にする:
/// header `X-Amzn-Errortype` に定義の型の名（`kind`）、本文は JSON の `{"message":…}`。
pub fn reply(status: StatusCode, kind: &'static str, message: &str) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (HeaderName::from_static("x-amzn-errortype"), kind),
        ],
        serde_json::json!({ "message": message }).to_string(),
    )
        .into_response()
}

/// 混んでいる（503。送り直せば通る見込み）。
pub fn busy() -> Unavailable {
    Unavailable {
        message: "混んでいる。少し待ってから送り直す".to_owned(),
    }
}

/// 読む操作の失敗: 混んでいる（503。送り直せば通る見込み）か、server の中の失敗（500）。
pub enum Failure {
    Busy,
    Internal(InternalError),
}

impl Failure {
    /// DB の誤り。接続を待ちきれなければ混んでいる、ほかは中の失敗。
    pub fn db(operation: &str, error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::PoolTimedOut => Failure::Busy,
            error => Failure::Internal(internal(operation, &error)),
        }
    }

    /// 生成した層の外の道（手の道）の答え（[`reply`] の形。型は生成した層と同じ `Unavailable`・`InternalError`）。
    pub fn reply(self) -> Response {
        match self {
            Failure::Busy => reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Unavailable",
                &busy().message,
            ),
            Failure::Internal(e) => reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &e.message,
            ),
        }
    }
}

/// 書いて足す操作の失敗: 中身の誤り（400）・castlist に登録の無い送り手（400）・番号が使われている（409）・
/// 混んでいる（503）・castlist に聞けない（503）・中の失敗（500）。
pub enum PutFailure {
    Invalid(String),
    /// 欄の path は `/actorRef`（castlist の PutEvent が、引けない `by` を 400 にするのと同じ）。
    UnknownSender(String),
    Conflict(String),
    Busy,
    Unreachable(String),
    Internal(InternalError),
}

impl PutFailure {
    /// 書けない訳を、答えの誤りにする。`taken` は番号が使われている時の訳。
    pub fn write(operation: &str, error: WriteError, taken: &str) -> Self {
        match error {
            WriteError::Busy => PutFailure::Busy,
            WriteError::Taken => PutFailure::Conflict(taken.to_owned()),
            WriteError::Db(e) => PutFailure::Internal(internal(operation, &e)),
        }
    }
}

macro_rules! from_put_failure {
    ($($error:ty),*) => {$(
        impl From<PutFailure> for $error {
            fn from(failure: PutFailure) -> Self {
                match failure {
                    PutFailure::Invalid(message) => ValidationException {
                        message,
                        field_list: None,
                    }
                    .into(),
                    PutFailure::UnknownSender(message) => ValidationException {
                        field_list: Some(vec![ValidationExceptionField {
                            path: "/actorRef".to_owned(),
                            message: message.clone(),
                        }]),
                        message,
                    }
                    .into(),
                    PutFailure::Conflict(message) => Conflict { message }.into(),
                    PutFailure::Busy => busy().into(),
                    PutFailure::Unreachable(message) => Unavailable { message }.into(),
                    PutFailure::Internal(e) => e.into(),
                }
            }
        }
    )*};
}

from_put_failure!(CreatePageError);

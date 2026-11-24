//! ListPages: ページを探す（`GET /api/pages`）。まだ作っていない（試験を先に書くための形だけ）。
use std::sync::Arc;

use pagetray_sdk::error::ListPagesError;
use pagetray_sdk::input::ListPagesInput;
use pagetray_sdk::output::ListPagesOutput;

use crate::db::Db;
use crate::errors::internal;

pub async fn list_pages(
    _db: Arc<Db>,
    _input: ListPagesInput,
) -> Result<ListPagesOutput, ListPagesError> {
    Err(internal("ListPages", &"まだ作っていない").into())
}

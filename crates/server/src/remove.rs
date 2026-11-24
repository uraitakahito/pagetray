//! DeletePages: 合うページを消す（`DELETE /api/pages`）。まだ作っていない（試験を先に書くための形だけ）。
use std::sync::Arc;

use pagetray_sdk::error::DeletePagesError;
use pagetray_sdk::input::DeletePagesInput;
use pagetray_sdk::output::DeletePagesOutput;

use crate::db::Db;
use crate::errors::internal;

pub async fn delete_pages(
    _db: Arc<Db>,
    _input: DeletePagesInput,
) -> Result<DeletePagesOutput, DeletePagesError> {
    Err(internal("DeletePages", &"まだ作っていない").into())
}

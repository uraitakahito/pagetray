//! file の操作（CreateFile・ListFiles・DeleteFiles）と、file の中身の道。まだ作っていない（試験を先に書くための形だけ）。
use std::sync::Arc;

use pagetray_sdk::error::{CreateFileError, DeleteFilesError, ListFilesError};
use pagetray_sdk::input::{CreateFileInput, DeleteFilesInput, ListFilesInput};
use pagetray_sdk::output::{CreateFileOutput, DeleteFilesOutput, ListFilesOutput};

use crate::db::Db;
use crate::errors::internal;

pub async fn create_file(
    _db: Arc<Db>,
    _input: CreateFileInput,
) -> Result<CreateFileOutput, CreateFileError> {
    Err(internal("CreateFile", &"まだ作っていない").into())
}

pub async fn list_files(
    _db: Arc<Db>,
    _input: ListFilesInput,
) -> Result<ListFilesOutput, ListFilesError> {
    Err(internal("ListFiles", &"まだ作っていない").into())
}

pub async fn delete_files(
    _db: Arc<Db>,
    _input: DeleteFilesInput,
) -> Result<DeleteFilesOutput, DeleteFilesError> {
    Err(internal("DeleteFiles", &"まだ作っていない").into())
}

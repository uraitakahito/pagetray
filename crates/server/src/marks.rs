//! 返事の状態（SetReplyState）とレビューの印（AddReviewMarks）を書く。まだ作っていない（試験を先に書くための形だけ）。
use std::sync::Arc;

use pagetray_sdk::error::{AddReviewMarksError, SetReplyStateError};
use pagetray_sdk::input::{AddReviewMarksInput, SetReplyStateInput};
use pagetray_sdk::output::{AddReviewMarksOutput, SetReplyStateOutput};

use crate::db::Db;
use crate::errors::internal;

pub async fn set_reply_state(
    _db: Arc<Db>,
    _input: SetReplyStateInput,
) -> Result<SetReplyStateOutput, SetReplyStateError> {
    Err(internal("SetReplyState", &"まだ作っていない").into())
}

pub async fn add_review_marks(
    _db: Arc<Db>,
    _input: AddReviewMarksInput,
) -> Result<AddReviewMarksOutput, AddReviewMarksError> {
    Err(internal("AddReviewMarks", &"まだ作っていない").into())
}

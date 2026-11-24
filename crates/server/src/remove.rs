//! DeletePages: 合うページをみな消す（`DELETE /api/pages?project&key&kind&n`。戻せない）。
//! 消した番号は残り、同じ番号は 2 度使わない（消したページの URL は 410）。消した時は、新着の知らせを送らない。
//! 消してよいのは TPM 担当だけという決まりは、呼ぶ側（roster）と守りの層（Origin）が持つ。ここでは人を見分けない。
use std::sync::Arc;

use pagetray_sdk::error::DeletePagesError;
use pagetray_sdk::input::DeletePagesInput;
use pagetray_sdk::output::DeletePagesOutput;

use crate::db::{Db, Target};
use crate::errors::{Failure, invalid};
use crate::pages::{is_dot, page_path};

const OPERATION: &str = "DeletePages";

pub async fn delete_pages(
    db: Arc<Db>,
    input: DeletePagesInput,
) -> Result<DeletePagesOutput, DeletePagesError> {
    // 形は定義が見てある。. と .. と、番号だけの名指しは、ここで断る。
    if is_dot(&input.project) {
        return Err(invalid("project に . と .. は使えない").into());
    }
    if input.key.as_deref().is_some_and(is_dot) {
        return Err(invalid("key に . と .. は使えない").into());
    }
    let n = match &input.n {
        None => None,
        Some(_) if input.key.is_none() || input.kind.is_none() => {
            return Err(invalid("n を書くときは key と kind も書く").into());
        }
        Some(n) => Some(n.parse().map_err(|_| invalid("n は 1 以上の数"))?),
    };
    let removed = db
        .remove(&Target {
            project: &input.project,
            key: input.key.as_deref(),
            kind: input.kind.as_deref(),
            n,
        })
        .await
        .map_err(|e| Failure::write(OPERATION, e))?;
    let removed: Vec<String> = removed
        .iter()
        .map(|(project, key, kind, n)| page_path(project, key, kind, *n))
        .collect();
    for path in &removed {
        tracing::info!("remove {path}");
    }
    Ok(DeletePagesOutput { removed })
}

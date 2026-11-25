//! file の操作（CreateFile・ListFiles・DeleteFiles）と、file の中身を返す手の道
//! （`GET /{project}/{key}/files/{語}-{n}.{拡張子}`。今の server と同じ URL）。
//!
//! - 受ける種類（拡張子・返す時の Content-Type・中身の見方）は [`FILE_TYPES`]（今の crates/api と同じ表）。
//! - 置く時の検査の順: 引数の形（生成した層の 400）→ 引数の決まり（400）→ 種類・Content-Type・中身（415）→
//!   castlist（400・503）→ DB（409・503）。前で断れば、後は見ない（castlist に聞かない・何も残さない）。
//! - 一覧の UI の form から上げた物（守りの層の印 [`FromUi`]）は、役割 [`UI_ROLE`]・workspace 空で残し、番号・時刻・
//!   付随する id を読まない（browser から担当を名乗らせない・取り込ませない）。
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use pagetray_sdk::error::{
    CreateFileError, DeleteFilesError, ListFilesError, UnsupportedMediaType,
};
use pagetray_sdk::input::{CreateFileInput, DeleteFilesInput, ListFilesInput};
use pagetray_sdk::output::{CreateFileOutput, DeleteFilesOutput, ListFilesOutput};
use pagetray_sdk::server::request::extension::Extension;
use sha2::{Digest, Sha256};

use crate::castlist::Castlist;
use crate::db::{Db, FileKey, FileTarget, NewFile};
use crate::errors::{Failure, PutFailure, internal, invalid, reply};
use crate::events::{Events, Note};
use crate::guard::{FromUi, RequestHost};
use crate::pages::{self, Checked, is_dot, is_segment};
use crate::record::FileRecord;
use crate::search::{FileQuery, LIMIT};
use crate::token;

/// 一覧の UI から上げた file の役割。
pub const UI_ROLE: &str = "product-manager";

/// 頁の印の頭（ページの検索の印と分ける）。
const TOKEN_KIND: &str = "files";

/// 中身の見方。拡張子と合わなければ受けない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sniff {
    Png,
    Jpeg,
    Gif,
    Webp,
    Pdf,
    /// UTF-8 として読めて、NUL を含まない。
    Text,
}

/// 受ける file の種類（拡張子・返す時の Content-Type・中身の見方）。ここに無い拡張子は受けない（415）。
pub const FILE_TYPES: &[(&str, &str, Sniff)] = &[
    ("png", "image/png", Sniff::Png),
    ("jpg", "image/jpeg", Sniff::Jpeg),
    ("jpeg", "image/jpeg", Sniff::Jpeg),
    ("gif", "image/gif", Sniff::Gif),
    ("webp", "image/webp", Sniff::Webp),
    ("pdf", "application/pdf", Sniff::Pdf),
    ("txt", "text/plain; charset=utf-8", Sniff::Text),
    ("log", "text/plain; charset=utf-8", Sniff::Text),
    ("md", "text/markdown; charset=utf-8", Sniff::Text),
    ("csv", "text/csv; charset=utf-8", Sniff::Text),
    ("json", "application/json; charset=utf-8", Sniff::Text),
];

/// 拡張子の Content-Type と中身の見方。受けない拡張子なら None。
pub(crate) fn file_type(ext: &str) -> Option<(&'static str, Sniff)> {
    FILE_TYPES
        .iter()
        .find(|(e, _, _)| *e == ext)
        .map(|&(_, mime, sniff)| (mime, sniff))
}

/// 中身が、その見方に合うか。
fn sniff(kind: Sniff, bytes: &[u8]) -> bool {
    match kind {
        Sniff::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        Sniff::Jpeg => bytes.starts_with(b"\xff\xd8\xff"),
        Sniff::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        Sniff::Webp => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        Sniff::Pdf => bytes.starts_with(b"%PDF-"),
        Sniff::Text => !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok(),
    }
}

/// Content-Type の種類の所（`;` の前）が同じか。大文字・小文字と、後ろの引数（charset など）は問わない。
fn same_media_type(given: &str, want: &str) -> bool {
    let media = |s: &str| s.split(';').next().unwrap_or("").trim().to_owned();
    media(given).eq_ignore_ascii_case(&media(want))
}

/// 拡張子の種類と、要求の Content-Type と、中身の頭を見る。受ければ、返す時の Content-Type。受けなければ 415 の訳。
fn accept(ext: &str, content_type: &str, body: &[u8]) -> Result<&'static str, String> {
    let Some((mime, kind)) = file_type(ext) else {
        let taken: Vec<&str> = FILE_TYPES.iter().map(|(ext, _, _)| *ext).collect();
        return Err(format!(
            "その拡張子は受けない（受けるのは {}）",
            taken.join("・")
        ));
    };
    if !same_media_type(content_type, mime) {
        return Err(format!(".{ext} の Content-Type は {mime}"));
    }
    if !sniff(kind, body) {
        return Err(format!("中身が .{ext} の形でない"));
    }
    Ok(mime)
}

/// file の名の語の形: 英小文字・数字・`-`・`_` で 1〜64 字。英小文字か数字で始まる（定義の `FileStem` と同じ）。
pub(crate) fn is_file_stem(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && matches!(s.as_bytes()[0], b'a'..=b'z' | b'0'..=b'9')
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

/// file の中身の path。
pub fn file_path(project: &str, key: &str, name: &str, ext: &str, n: i64) -> String {
    format!("/{project}/{key}/files/{name}-{n}.{ext}")
}

/// `<語>-<n>.<拡張子>` を分ける。番号は、頭に 0 の無い 1 以上の数だけ。拡張子は受ける物だけ。
fn parse_file_name(file: &str) -> Option<(&str, &str, i64)> {
    let (rest, ext) = file.split_once('.')?;
    let (name, n) = rest.rsplit_once('-')?;
    let ok = is_file_stem(name)
        && file_type(ext).is_some()
        && !n.starts_with('0')
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (name, ext, n))
}

/// 置く時の名（`<語>.<拡張子>`。形は定義の `FileName` が見てある）を、語と拡張子に分ける。
fn split_name(name: &str) -> (&str, &str) {
    name.split_once('.').unwrap_or((name, ""))
}

/// file を 1 つ置く。一覧の UI から上げた物（`ui`）は、役割・workspace・番号・時刻・送り手を置き場が決める。
pub async fn create_file(
    db: Arc<Db>,
    castlist: Option<Castlist>,
    events: Events,
    input: CreateFileInput,
    Extension(RequestHost(host)): Extension<RequestHost>,
    ui: Option<Extension<FromUi>>,
) -> Result<CreateFileOutput, CreateFileError> {
    let ui = ui.is_some();
    let (role, workspace) = if ui {
        (UI_ROLE, "")
    } else {
        let role = input.role.as_deref().ok_or_else(|| {
            PutFailure::Invalid(
                "role が要る（英小文字で始まり、英小文字・数字・- で 32 字まで）".to_owned(),
            )
        })?;
        (role, input.workspace.as_deref().unwrap_or(""))
    };
    let Checked {
        n,
        created_at,
        actor,
    } = if ui {
        pages::check(&input.project, &input.key, None, None, (None, None))
    } else {
        pages::check(
            &input.project,
            &input.key,
            input.n.as_deref(),
            input.created_at.as_ref(),
            (input.actor_issuer.as_ref(), input.actor_ref.as_ref()),
        )
    }
    .map_err(PutFailure::Invalid)?;
    let (name, ext) = split_name(&input.name);
    let body = input.body.as_ref();
    let content_type = accept(ext, &input.content_type, body)
        .map_err(|message| UnsupportedMediaType { message })?;
    let sha256: String = Sha256::digest(body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // castlist には、断る物を見終えてから、書く取引を始める前に聞く（待つ間、書く接続を握らない）。
    let sender = pages::ask(castlist.as_ref(), actor).await?;
    let row = db
        .put_file(&NewFile {
            project: &input.project,
            key: &input.key,
            name,
            ext,
            content_type,
            sha256: &sha256,
            role,
            workspace,
            body,
            n,
            created_at: created_at.as_deref(),
            sender: sender.as_ref(),
        })
        .await
        .map_err(|e| PutFailure::write("CreateFile", e, "その番号は、もう使われている"))?;
    let record =
        FileRecord::try_from(row).map_err(|e| PutFailure::Internal(internal("CreateFile", &e)))?;
    // 答えの記録に直せてから知らせる（直せずに 500 を返す書き込みを、画面に知らせない）。
    events.send(Note::File(record.clone().into()));
    tracing::info!(
        "put {} role={role} workspace={workspace} size={}",
        record.path(),
        body.len()
    );
    let url = format!("http://{host}{}", record.path());
    Ok(record.created(url))
}

/// file を探す（`GET /api/files`）。値の形は定義が見てあるので、ここでは頁の印を見る。
pub async fn list_files(
    db: Arc<Db>,
    input: ListFilesInput,
) -> Result<ListFilesOutput, ListFilesError> {
    const OPERATION: &str = "ListFiles";
    let oldest = input.sort.as_deref() == Some("createdAt");
    let after = input
        .next_token
        .as_deref()
        .map(|t| token::after(t, TOKEN_KIND, "file の検索", oldest).map_err(invalid))
        .transpose()?;
    let query = FileQuery {
        project: input.project,
        key: input.key,
        role: input.role,
        workspace: input.workspace,
        q: input.q,
        actor: input.actor,
        oldest,
        after,
        // 1〜1000 は定義が見てある。
        limit: input.max_results.map_or(LIMIT, i64::from),
    };
    let page = db
        .list_files(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = page
        .more
        .then(|| page.rows.last())
        .flatten()
        .map(|last| token::next(TOKEN_KIND, oldest, &last.created_at, last.id));
    let files = page
        .rows
        .into_iter()
        .map(|row| {
            FileRecord::try_from(row)
                .map(Into::into)
                .map_err(|e| Failure::data(OPERATION, &e))
        })
        .collect::<Result<_, _>>()?;
    Ok(ListFilesOutput { files, next_token })
}

/// 合う file をみな消す（`DELETE /api/files?project&key&name&n`。戻せない）。消した番号は残り、同じ番号は 2 度使わない
/// （消した file の URL は 410）。ページは消えない。消した時は、新着の知らせを送らない。
pub async fn delete_files(
    db: Arc<Db>,
    input: DeleteFilesInput,
) -> Result<DeleteFilesOutput, DeleteFilesError> {
    // 形は定義が見てある。. と .. と、番号だけの名指しは、ここで断る。
    if is_dot(&input.project) {
        return Err(invalid("project に . と .. は使えない").into());
    }
    if input.key.as_deref().is_some_and(is_dot) {
        return Err(invalid("key に . と .. は使えない").into());
    }
    let n = match &input.n {
        None => None,
        Some(_) if input.key.is_none() || input.name.is_none() => {
            return Err(invalid("n を書くときは key と name も書く").into());
        }
        Some(n) => Some(n.parse().map_err(|_| invalid("n は 1 以上の数"))?),
    };
    let removed = db
        .remove_files(&FileTarget {
            project: &input.project,
            key: input.key.as_deref(),
            name: input.name.as_deref().map(split_name),
            n,
        })
        .await
        .map_err(|e| Failure::write("DeleteFiles", e))?;
    let removed: Vec<String> = removed
        .iter()
        .map(|(project, key, name, ext, n)| file_path(project, key, name, ext, *n))
        .collect();
    for path in &removed {
        tracing::info!("remove {path}");
    }
    Ok(DeleteFilesOutput { removed })
}

/// file の中身を、置いたままの byte と、置いた時に決めた Content-Type で返す。
pub async fn show(
    State(db): State<Arc<Db>>,
    Path((project, key, file)): Path<(String, String, String)>,
) -> Response {
    let Some((name, ext, n)) =
        parse_file_name(&file).filter(|_| is_segment(&project) && is_segment(&key))
    else {
        return not_found();
    };
    let file = FileKey {
        project: &project,
        key: &key,
        name,
        ext,
        n,
    };
    match db.file_body(&file).await {
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
        Ok(None) => match db.file_removed_at(&file).await {
            Ok(Some(at)) => reply(
                StatusCode::GONE,
                "Gone",
                &format!("その file は消した（{at}）"),
            ),
            Ok(None) => not_found(),
            Err(e) => Failure::db("file", e).reply(),
        },
        Err(e) => Failure::db("file", e).reply(),
    }
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "NotFound", "その file は無い")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_split_into_stem_number_and_ext() {
        assert_eq!(parse_file_name("screen-1.png"), Some(("screen", "png", 1)));
        assert_eq!(
            parse_file_name("my-shot_2-12.jpeg"),
            Some(("my-shot_2", "jpeg", 12))
        );
        assert_eq!(
            file_path("p", "K-1", "screen", "png", 2),
            "/p/K-1/files/screen-2.png"
        );
        for ng in [
            "screen.png",
            "screen-0.png",
            "screen-01.png",
            "screen-.png",
            "screen-1",
            "screen-1.svg",
            "screen-1.PNG",
            "Screen-1.png",
            "-1.png",
            "screen-1.png.png",
            "screen-99999999999999999999.png",
        ] {
            assert_eq!(parse_file_name(ng), None, "{ng}");
        }
    }

    #[test]
    fn only_listed_types_are_taken() {
        assert_eq!(file_type("jpg"), file_type("jpeg"));
        assert_eq!(file_type("md").unwrap().0, "text/markdown; charset=utf-8");
        for ng in ["svg", "html", "mp4", "PNG", "exe", ""] {
            assert_eq!(file_type(ng), None, "{ng}");
        }
    }

    #[test]
    fn contents_are_sniffed() {
        assert!(sniff(Sniff::Png, b"\x89PNG\r\n\x1a\nrest"));
        assert!(!sniff(Sniff::Png, b"%PDF-1.7"));
        assert!(sniff(Sniff::Jpeg, b"\xff\xd8\xff\xe0"));
        assert!(sniff(Sniff::Gif, b"GIF89a..."));
        assert!(!sniff(Sniff::Gif, b"GIF90a..."));
        assert!(sniff(Sniff::Webp, b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!sniff(Sniff::Webp, b"RIFF\0\0\0\0WAVE"));
        assert!(!sniff(Sniff::Webp, b"RIFF"));
        assert!(sniff(Sniff::Pdf, b"%PDF-1.7"));
        assert!(sniff(Sniff::Text, "字の file\n".as_bytes()));
        assert!(!sniff(Sniff::Text, b"a\0b"));
        assert!(!sniff(Sniff::Text, b"\xff\xfe"));
    }

    #[test]
    fn media_types_compare_before_the_parameters_in_any_case() {
        assert!(same_media_type(
            "Text/Markdown; charset=UTF-8",
            "text/markdown; charset=utf-8"
        ));
        assert!(same_media_type(" image/png ", "image/png"));
        assert!(!same_media_type("image/png-x", "image/png"));
        assert!(!same_media_type("", "image/png"));
    }
}

//! castlist（送り手の本当の id と名を持つ、pagetray を動かす機械の中の別のサーバー）に、送り手を聞く。
//! まだ作っていない（試験を先に書くための形だけ。居場所の形を見ず、聞く処理も無い）。

/// 画面の link の頭の既定。本番の castlist を、host の browser から開く所。
pub const DEFAULT_LINK: &str = "http://127.0.0.1:8802";

/// castlist の居場所。
#[derive(Clone, Debug)]
pub struct Castlist {
    api: String,
    link: String,
}

impl Castlist {
    /// 環境変数（`PAGETRAY_CASTLIST_URL`・`PAGETRAY_CASTLIST_LINK_URL`）から作る（[`Castlist::configure`]）。
    pub fn from_env() -> Result<Option<Self>, String> {
        Self::configure(
            std::env::var("PAGETRAY_CASTLIST_URL").ok().as_deref(),
            std::env::var("PAGETRAY_CASTLIST_LINK_URL").ok().as_deref(),
        )
    }

    /// 環境変数の値から作る。まだ形を見ない（`api` が無い・空なら `None`、ほかは空の居場所）。
    pub fn configure(api: Option<&str>, _link: Option<&str>) -> Result<Option<Self>, String> {
        Ok(api.filter(|v| !v.is_empty()).map(|_| Self {
            api: String::new(),
            link: String::new(),
        }))
    }

    /// 聞く先（`http://<host>:<port>`）。
    pub fn api(&self) -> &str {
        &self.api
    }

    /// 画面から castlist のアクターを開く URL の頭（`http://<host>:<port>`）。
    pub fn link(&self) -> &str {
        &self.link
    }
}

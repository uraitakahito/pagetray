//! HTML から、題と、全文検索の索引に入れる字を取り出す（試験を先に書く commit の仮の物。次の commit で作る）。

/// `<title>` の中身。無ければ空。
pub fn title(_html: &str) -> String {
    String::new()
}

/// tag・script・style・comment を除いた字。
pub fn plain(_html: &str) -> String {
    String::new()
}

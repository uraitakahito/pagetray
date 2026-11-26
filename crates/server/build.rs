// 移行の SQL は sqlx::migrate!() が埋め込む。SQL だけを変えた時も build し直す。
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}

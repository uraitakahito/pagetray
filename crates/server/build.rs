// 移行の SQL は sqlx::migrate!() が埋め込む。SQL だけを変えた時も build し直す。
fn main() {
    println!("cargo:rerun-if-changed=migrations");
    // 移す命令（src/migrate.rs）が、今の server の移行（repo の頭の migrations/）の版と中身の写しを埋め込む。
    println!("cargo:rerun-if-changed=../../migrations");
}

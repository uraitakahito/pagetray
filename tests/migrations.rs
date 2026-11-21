//! 当てた移行の SQL が変わっていないかを見る。
//!
//! sqlx は、当てた SQL の SHA-384 を DB（`_sqlx_migrations`）に覚えていて、起きる時に突き合わせる。
//! 当てた SQL を 1 字でも変えると、動いている DB では起きなくなる。CI には動いている DB が無いので、ここで止める。
//! 表を変える時は、次の番号の file を足して、下の一覧にも 1 行足す（前からある行は変えない）。

/// 版ごとの SHA-384（`shasum -a 384 migrations/*.sql` と同じ値。動いている DB が覚えている値でもある）。
const APPLIED: &[(i64, &str)] = &[
    (
        1,
        "8d9d677ffca4bb5056117010e6cca50b00f6499f637a5ee20d9a8e1fd3d46588eede801fa0bb84394afc52eeffb4467c",
    ),
    (
        2,
        "f26b7def147cf7927993bacabb7961ce5e4fc683334a28f8032699f47040830a665dd27fd305bf0e614cd15e8bd46fcd",
    ),
];

#[test]
fn applied_migrations_are_never_edited() {
    let now: Vec<(i64, String)> = sqlx::migrate!()
        .iter()
        .map(|m| {
            let hex = m.checksum.iter().map(|b| format!("{b:02x}")).collect();
            (m.version, hex)
        })
        .collect();
    let want: Vec<(i64, String)> = APPLIED.iter().map(|(v, h)| (*v, (*h).to_owned())).collect();
    assert_eq!(
        now, want,
        "移行の SQL が変わった。当てた SQL は変えず、次の番号の file を足す"
    );
}

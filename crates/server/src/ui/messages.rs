//! メッセージの一覧の画面（`/messages`）。ページの一覧（[`super`]）とは別の画面で、操作の帯の link で行き来する。
//!
//! - 置き場は、メッセージの記録を見せるだけ（状態は、呼ぶ側が書いた字を、画面の字にして出す）。
//! - 作りはページの一覧と同じ: サーバーが HTML を組み（template は repo の templates/messages.html）、字はみな HTML として
//!   逃がす（本文も。HTML として出さない）。絞り込みは link で動き、条件は URL に出る。
//! - 引数: `project`・`key`・`to`・`from`・`state`・`at`（続きの印）。空は書かないのと同じ。知らない `state` は無視。
//! - 「まだ届いていない n 件」と絞り込みの候補は、絞りに関わらず置き場の全部（[`crate::facets::MessageFacets`]）。
use std::sync::LazyLock;

use askama::Template;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::HeaderValue;
use axum::response::Response;
use serde::Deserialize;

use super::{Choice, Counter, FilterGroup, PAGE_SIZE, Ui, encode, screen, short_time, unreadable};
use crate::db::MessageRow;
use crate::errors::Failure;
use crate::facets::Count;
use crate::messages::{TOKEN_KIND, sort_name};
use crate::search::MessageQuery;
use crate::token;

/// 状態の字（定義の `MessageState` と同じ）と、画面に出す字。絞り込みの候補も、この順に出す。
const STATES: [(&str, &str); 5] = [
    ("waiting", "まだ届いていない"),
    ("delivered", "届いた"),
    ("undeliverable", "渡せない"),
    ("expired", "期限切れ"),
    ("carried", "cmux で運んだ"),
];

/// 本文をそのまま出す長さ（字の数と行の数）。超えれば、頭だけ見せて、開くと全部。
const TEXT_CHARS: usize = 200;
const TEXT_LINES: usize = 5;
/// 長い本文の、頭として見せる字の数。
const HEAD_CHARS: usize = 120;

/// template（repo の templates/messages.html）。inline の style と script の hash を、ここから取る。
const TEMPLATE: &str = include_str!("../../../../templates/messages.html");

/// メッセージの一覧の答えに付ける Content-Security-Policy（ページの一覧と同じ形。hash は、この template の物）。
static CSP: LazyLock<HeaderValue> = LazyLock::new(|| super::csp(TEMPLATE));

#[derive(Clone, Default, Deserialize)]
pub(super) struct View {
    project: Option<String>,
    key: Option<String>,
    /// 受け手の名。
    to: Option<String>,
    /// 送り手の名。
    from: Option<String>,
    /// 状態（定義の `MessageState` の字）。
    state: Option<String>,
    /// 並びの続きの印（前の頁の最後の行の、送った時刻と id）。
    at: Option<String>,
}

impl View {
    /// この条件の一覧の URL。空の条件は出さない。
    fn href(&self) -> String {
        let pairs = [
            ("project", &self.project),
            ("key", &self.key),
            ("to", &self.to),
            ("from", &self.from),
            ("state", &self.state),
            ("at", &self.at),
        ];
        let query: Vec<String> = pairs
            .into_iter()
            .filter_map(|(name, value)| Some(format!("{name}={}", encode(value.as_deref()?))))
            .collect();
        if query.is_empty() {
            "/messages".to_owned()
        } else {
            format!("/messages?{}", query.join("&"))
        }
    }

    /// 条件を 1 つ変えた一覧の URL。並びの続きの境は、条件が変わると意味が無いので外す。
    fn with(&self, change: impl FnOnce(&mut View)) -> String {
        let mut next = self.clone();
        next.at = None;
        change(&mut next);
        next.href()
    }
}

/// 一覧の列の、メッセージ 1 件分。
struct Row {
    project: String,
    id: String,
    /// Key を書かずに置いた記録は空。
    key: String,
    heading: String,
    from_name: String,
    to_name: String,
    /// 本文（字のまま。template が逃がす）。
    text: String,
    /// 長い本文の頭（改行を空白にした、初めの [`HEAD_CHARS`] 字）。短い本文には無い。
    head: Option<String>,
    sent_at: String,
    /// script が動かない時に出す時刻（UTC）。
    time: String,
    state: &'static str,
    state_label: &'static str,
    /// 届いた時刻（残した字と、script が動かない時に出す字）。
    delivered: Option<(String, String)>,
    /// どの時に渡したかの名。書いていなければ空。
    via: String,
    /// 渡し直した数（渡した回数 − 1）。渡した回数が 2 以上の時だけ出す。
    again: i64,
    /// 最後に書き足された時刻（script が、行が替わったかを見る）。
    updated_at: String,
}

impl Row {
    /// 届いた時刻・渡した時・渡し直しの数の、どれかを出すか。
    fn has_notes(&self) -> bool {
        self.delivered.is_some() || !self.via.is_empty() || self.again > 0
    }
}

#[derive(Template)]
#[template(path = "messages.html")]
struct Messages {
    /// まだ届いていないメッセージの数（絞り込みに関わらず、置き場の全部）。tab の題に出す。
    waiting: i64,
    /// 操作の帯の「まだ届いていない n 件」の釦（`#undelivered`）。
    counter: Counter,
    reload_href: String,
    reset_href: String,
    filtered: bool,
    filters: Vec<FilterGroup>,
    rows: Vec<Row>,
    more_href: Option<String>,
}

/// `/messages`。引数が読めなければ（同じ名を 2 度書いた など）400。
pub(super) async fn index(
    State(ui): State<Ui>,
    view: Result<Query<View>, QueryRejection>,
) -> Response {
    let Ok(Query(view)) = view else {
        return unreadable();
    };
    screen(page(&ui, view).await, &CSP)
}

/// 状態の名（DB の字を 'static にする）と、画面に出す字。知らない字なら None。
fn state_view(state: &str) -> Option<(&'static str, &'static str)> {
    STATES.into_iter().find(|(name, _)| *name == state)
}

/// 条件を揃え、DB を読み、画面を組む。
async fn page(ui: &Ui, mut view: View) -> Result<String, Failure> {
    // 空の欄は、条件が無いのと同じに扱う。知らない状態も。
    for field in [
        &mut view.project,
        &mut view.key,
        &mut view.to,
        &mut view.from,
        &mut view.state,
        &mut view.at,
    ] {
        *field = field.take().filter(|v| !v.trim().is_empty());
    }
    view.state = view.state.filter(|s| state_view(s).is_some());
    // 読めない続きの印は、書かないのと同じ（頭から出す）。
    let after = view
        .at
        .as_deref()
        .and_then(|at| token::after_by(at, TOKEN_KIND, "画面の続き", sort_name(false)).ok());
    if after.is_none() {
        view.at = None;
    }
    let query = MessageQuery {
        project: view.project.clone(),
        key: view.key.clone(),
        to: view.to.clone(),
        from: view.from.clone(),
        state: view.state.clone(),
        after,
        limit: PAGE_SIZE,
        ..MessageQuery::default()
    };
    let (found, facets) = tokio::try_join!(ui.db.list_messages(&query), ui.db.message_facets())
        .map_err(|e| Failure::db("ui", e))?;

    let more_href = found.more.then(|| found.rows.last()).flatten().map(|last| {
        let at = token::next_by(TOKEN_KIND, sort_name(false), &last.sent_at, last.id);
        view.with(|v| v.at = Some(at))
    });
    let waiting = facets
        .states
        .iter()
        .find(|c| c.value == "waiting")
        .map_or(0, |c| c.count);
    // 候補の並び。押すと今の条件に重ね、選んである候補をもう 1 度押すと外す。
    // `show` が、候補の字を決める（値そのものか、状態の字）。
    let group = |title,
                 counts: Vec<Count>,
                 field: fn(&mut View) -> &mut Option<String>,
                 show: fn(&str) -> String| FilterGroup {
        title,
        choices: counts
            .into_iter()
            .map(|c| {
                let mut probe = view.clone();
                let active = field(&mut probe).as_deref() == Some(c.value.as_str());
                let label = show(&c.value);
                let next = (!active).then_some(c.value);
                Choice {
                    label,
                    note: String::new(),
                    count: c.count,
                    href: view.with(|v| *field(v) = next),
                    active,
                }
            })
            .collect(),
    };
    // 状態の候補は、決まった順（まだ届いていない・届いた・渡せない・期限切れ・cmux で運んだ）。件数が 0 の候補は出さない。
    let mut states = facets.states;
    states.sort_by_key(|c| STATES.iter().position(|(name, _)| *name == c.value));
    let messages = Messages {
        waiting,
        // 操作の帯の釦は、ほかの絞り込みを外して、まだ届いていない物だけに絞る。
        counter: Counter {
            label: "まだ届いていない",
            title: "まだ届いていないメッセージだけに絞る",
            count: waiting,
            href: View {
                state: Some("waiting".to_owned()),
                ..View::default()
            }
            .href(),
            active: view.state.as_deref() == Some("waiting"),
        },
        reload_href: view.with(|_| {}),
        reset_href: View::default().href(),
        filtered: [&view.project, &view.key, &view.to, &view.from, &view.state]
            .iter()
            .any(|v| v.is_some()),
        filters: vec![
            // 状態の候補は、値でなく画面の字で出す。
            group(
                "状態",
                states,
                |v| &mut v.state,
                |s| state_view(s).map_or(s, |(_, label)| label).to_owned(),
            ),
            group("受け手", facets.to, |v| &mut v.to, str::to_owned),
            group("送り手", facets.from, |v| &mut v.from, str::to_owned),
            group("Key", facets.keys, |v| &mut v.key, str::to_owned),
            group(
                "project",
                facets.projects,
                |v| &mut v.project,
                str::to_owned,
            ),
        ],
        rows: found.rows.into_iter().map(row).collect(),
        more_href,
    };
    messages.render().map_err(|e| Failure::data("ui", &e))
}

/// 長い本文（字か行が多い）なら、頭として見せる字。
fn head(text: &str) -> Option<String> {
    let long = text.chars().count() > TEXT_CHARS || text.lines().count() > TEXT_LINES;
    long.then(|| {
        text.chars()
            .take(HEAD_CHARS)
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect()
    })
}

fn row(r: MessageRow) -> Row {
    let (state, state_label) = state_view(&r.state).unwrap_or(("", ""));
    Row {
        head: head(&r.text),
        time: short_time(&r.sent_at),
        delivered: r.delivered_at.map(|at| {
            let time = short_time(&at);
            (at, time)
        }),
        again: r.deliveries.map_or(0, |n| n - 1).max(0),
        state,
        state_label,
        project: r.project,
        id: r.message_id,
        key: r.key.unwrap_or_default(),
        heading: r.heading,
        from_name: r.from_name,
        to_name: r.to_name,
        text: r.text,
        sent_at: r.sent_at,
        via: r.via.unwrap_or_default(),
        updated_at: r.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_carry_the_conditions_and_drop_empty_ones() {
        let view = View {
            to: Some("a b?c&d#e/f".into()),
            state: Some("waiting".into()),
            at: Some("6d65".into()),
            ..View::default()
        };
        assert_eq!(
            view.href(),
            "/messages?to=a%20b%3Fc%26d%23e%2Ff&state=waiting&at=6d65"
        );
        assert_eq!(view.with(|v| v.to = None), "/messages?state=waiting");
        assert_eq!(View::default().href(), "/messages");
    }

    #[test]
    fn only_long_texts_get_a_head() {
        assert_eq!(head("短い\n2 行目"), None);
        assert_eq!(head(&"あ".repeat(200)), None);
        assert_eq!(
            head(&"あ".repeat(201)).map(|h| h.chars().count()),
            Some(120)
        );
        assert_eq!(head("1\n2\n3\n4\n5"), None);
        assert_eq!(head("1\n2\n3\n4\n5\n6"), Some("1 2 3 4 5 6".to_owned()));
    }
}

$version: "2"

// pagetray（ページの置き場）の API。決まりは docs/src/next.md。共通の形（ai.common）は castlist の release の tag で固めて取った
// model/castlist/common.smithy（ci/gen.sh が版と digest を見る）。
// 例（@httpRequestTests・@httpResponseTests）の id は、castlist の定義と 1 つの model に入って重なれないので、頭に Pagetray を付ける。
namespace ai.pagetray

use ai.common#ActorRef
use ai.common#ActorSummary
use ai.common#Conflict
use ai.common#DateTime
use ai.common#Forbidden
use ai.common#InternalError
use ai.common#PayloadTooLarge
use ai.common#Unavailable
use aws.protocols#restJson1
use smithy.framework#ValidationException
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
@title("pagetray")
service Pagetray {
    version: "1.0"
    operations: [
        GetHealth
        CreatePage
    ]

    // 操作の前の守りの層（server の生成した骨の外）が返す誤り。どの操作の答えにもなり得る。
    errors: [
        Forbidden
        PayloadTooLarge
        Unavailable
    ]
}

/// 動いているかと、pagetray の版。
@readonly
@http(method: "GET", uri: "/healthz")
@httpRequestTests([
    {
        id: "PagetrayGetHealthRequest"
        documentation: "本文も引数も無い"
        protocol: restJson1
        method: "GET"
        uri: "/healthz"
    }
])
@httpResponseTests([
    {
        id: "PagetrayGetHealthResponse"
        documentation: "状態と版を返す"
        protocol: restJson1
        code: 200
        headers: { "Content-Type": "application/json" }
        body: """
            {"status":"ok","version":"1.0.0"}"""
        bodyMediaType: "application/json"
        params: { status: "ok", version: "1.0.0" }
    }
])
operation GetHealth {
    input := {}

    output := {
        /// いつも `ok`。
        @required
        status: String

        /// pagetray の版。
        @required
        version: String
    }
}

/// ページを 1 枚置く。本文は HTML をそのまま（`Content-Type: text/html`）。答えに本文は入らない。
/// 番号（`n`）を書かなければ、同じ project・key・kind の置いた番号と消した番号の最大 + 1（初めは 1）。
/// 書けば（取り込み）その番号で置き、置いた・消した番号なら 409（何も替えない）。
/// 題は本文の初めの `<title>`（無ければ空）。
/// 付随する id（`actorIssuer`・`actorRef`）を書けば、送り手を castlist に聞き（書く前に。1 秒・送り直さない）、答えと一緒に残す。
/// castlist に登録の無い送り手は 400、castlist に聞けなければ 503（どちらも何も残さない）。castlist の無い pagetray は、聞かずに付随する id だけを残す。
@http(method: "POST", uri: "/api/projects/{project}/keys/{key}/kinds/{kind}/pages", code: 201)
@httpRequestTests([
    {
        id: "PagetrayCreatePageRequest"
        documentation: "道に project・key・kind、引数に役割と workspace、本文は HTML"
        protocol: restJson1
        method: "POST"
        uri: "/api/projects/ai/keys/AI-36/kinds/plan/pages"
        queryParams: ["role=tpm", "workspace=workspace:1"]
        headers: { "Content-Type": "text/html" }
        body: "<title>計画</title><p>本文</p>"
        bodyMediaType: "text/html"
        params: {
            project: "ai"
            key: "AI-36"
            kind: "plan"
            role: "tpm"
            workspace: "workspace:1"
            body: "<title>計画</title><p>本文</p>"
        }
    }
    {
        id: "PagetrayCreatePageImportRequest"
        documentation: "取り込みは、番号（数の字）と置いた時刻を書く"
        protocol: restJson1
        method: "POST"
        uri: "/api/projects/ai/keys/AI-36/kinds/plan/pages"
        queryParams: ["role=tpm", "n=7", "createdAt=2026-11-24T00%3A00%3A00Z"]
        headers: { "Content-Type": "text/html" }
        body: "<p>前の置き場の物</p>"
        bodyMediaType: "text/html"
        params: {
            project: "ai"
            key: "AI-36"
            kind: "plan"
            role: "tpm"
            n: "7"
            createdAt: 1795478400
            body: "<p>前の置き場の物</p>"
        }
    }
    {
        id: "PagetrayCreatePageSenderRequest"
        documentation: "送り手の付随する id は、出した物と参照の組（参照の値に : があってよい）"
        protocol: restJson1
        method: "POST"
        uri: "/api/projects/ai/keys/AI-36/kinds/plan/pages"
        queryParams: ["role=tpm", "actorIssuer=tpm", "actorRef=cmux.workspace.ref%3Aworkspace%3A3"]
        headers: { "Content-Type": "text/html" }
        body: "<p>本文</p>"
        bodyMediaType: "text/html"
        params: {
            project: "ai"
            key: "AI-36"
            kind: "plan"
            role: "tpm"
            actorIssuer: "tpm"
            actorRef: "cmux.workspace.ref:workspace:3"
            body: "<p>本文</p>"
        }
    }
])
@httpResponseTests([
    {
        id: "PagetrayCreatePageCreated"
        documentation: "置けば 201 で、ページの記録（本文は無い）と、要求の Host で組んだ URL を返す"
        protocol: restJson1
        code: 201
        headers: {
            "Content-Type": "application/json"
            "Location": "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
        }
        body: """
            {"project":"ai","key":"AI-36","kind":"plan","n":"1","title":"計画","role":"tpm","workspace":"workspace:1","createdAt":"2026-11-24T00:00:00Z","size":42,"path":"/ai/AI-36/plan-1.html","url":"http://127.0.0.1:8801/ai/AI-36/plan-1.html"}"""
        bodyMediaType: "application/json"
        params: {
            project: "ai"
            key: "AI-36"
            kind: "plan"
            n: "1"
            title: "計画"
            role: "tpm"
            workspace: "workspace:1"
            createdAt: 1795478400
            size: 42
            path: "/ai/AI-36/plan-1.html"
            url: "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
            location: "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
        }
    }
    {
        id: "PagetrayCreatePageCreatedWithSender"
        documentation: "付随する id を書いて置けば、送り手（castlist の答えと、答えた時刻）も返す"
        protocol: restJson1
        code: 201
        headers: {
            "Content-Type": "application/json"
            "Location": "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
        }
        body: """
            {"project":"ai","key":"AI-36","kind":"plan","n":"1","title":"","role":"tpm","workspace":"","createdAt":"2026-11-24T00:00:00Z","size":1,"path":"/ai/AI-36/plan-1.html","sender":{"issuer":"tpm","ref":"claude.session:s-1","actor":{"id":"019abcde-0000-7000-8000-000000000001","name":"se.1","kind":"ai"},"resolvedAt":"2026-11-24T00:00:00Z"},"url":"http://127.0.0.1:8801/ai/AI-36/plan-1.html"}"""
        bodyMediaType: "application/json"
        params: {
            project: "ai"
            key: "AI-36"
            kind: "plan"
            n: "1"
            title: ""
            role: "tpm"
            workspace: ""
            createdAt: 1795478400
            size: 1
            path: "/ai/AI-36/plan-1.html"
            sender: {
                issuer: "tpm"
                ref: "claude.session:s-1"
                actor: { id: "019abcde-0000-7000-8000-000000000001", name: "se.1", kind: "ai" }
                resolvedAt: 1795478400
            }
            url: "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
            location: "http://127.0.0.1:8801/ai/AI-36/plan-1.html"
        }
    }
])
operation CreatePage {
    input := {
        /// 状態の folder の名（URL の 1 段目）。`api`・`.`・`..` は置けない（server が 400）。
        @required
        @httpLabel
        project: Segment

        /// Key（課題の名）。`.`・`..` は置けない（server が 400）。
        @required
        @httpLabel
        key: Segment

        /// 種類（brief・confirm・plan …。名の一覧は持たない）。
        @required
        @httpLabel
        kind: Name

        /// 置いた担当の役割。
        @required
        @httpQuery("role")
        role: Name

        /// 置いた担当の workspace の ID（無ければ空）。
        @httpQuery("workspace")
        workspace: Workspace

        /// 取り込みの時だけ: 番号。無ければ server が決める。
        @httpQuery("n")
        n: PageNumber

        /// 取り込みの時だけ: 置いた時刻（UTC・ミリ秒まで）。無ければ pagetray の今。
        @httpQuery("createdAt")
        createdAt: DateTime

        /// 付随する id を出した物（`actorRef` と組。片方だけは 400）。
        @httpQuery("actorIssuer")
        actorIssuer: Issuer

        /// 付随する id（`種類:値`。`actorIssuer` と組。片方だけは 400）。
        @httpQuery("actorRef")
        actorRef: ActorRef

        /// ページの HTML（空は 400）。UTF-8 でなくても受ける（題と検索の字は、読めない byte を置き換えて取る）。
        @required
        @httpPayload
        body: Html
    }

    output := with [PageFields] {
        /// 要求の `Host` で組んだ、ページの URL。
        @required
        url: String

        /// `url` と同じ。
        @required
        @httpHeader("Location")
        location: String
    }

    errors: [
        ValidationException
        Conflict
        Unavailable
        InternalError
    ]
}

/// ページの記録の欄（本文は入らない）。
@mixin
structure PageFields {
    @required
    project: Segment

    @required
    key: Segment

    @required
    kind: Name

    @required
    n: PageNumber

    /// 本文の初めの `<title>`（文字参照を戻し、空白を 1 つにまとめる）。無ければ空。
    @required
    title: String

    @required
    role: Name

    @required
    workspace: Workspace

    @required
    createdAt: DateTime

    /// 本文の byte の数。
    @required
    size: Long

    /// この server の中の path（`/<project>/<key>/<kind>-<n>.html`）。
    @required
    path: String

    /// 送り手。付随する id を書いて置いた時だけ。
    sender: Sender
}

/// 送り手の記録: 置いた時の付随する id と、castlist の答え（置いた時の物。後から書き換えない）。
structure Sender {
    /// 付随する id を出した物。
    @required
    issuer: Issuer

    /// 付随する id。
    @required
    ref: ActorRef

    /// castlist が答えたアクター。castlist の無い pagetray が置いた物には無い。
    actor: ActorSummary

    /// castlist が答えた時刻（答えの `asOf`）。`actor` と一緒にだけある。
    resolvedAt: DateTime
}

/// URL の 1 段になる字（project・key）: 英数字と `.`・`_`・`-` で 255 字まで。
@pattern("^[A-Za-z0-9._-]{1,255}$")
string Segment

/// 種類と役割の名: 英小文字で始まり、英小文字・数字・`-` で 32 字まで。
@pattern("^[a-z][a-z0-9-]{0,31}$")
string Name

/// workspace の ID: 英数字と `:`・`.`・`_`・`-` で 64 字まで（空でもよい）。
@pattern("^[A-Za-z0-9:._-]{0,64}$")
string Workspace

/// ページの番号（種類ごと）。id なので数の字にする: 頭に 0 の無い 1 以上の数（18 桁まで）。
@pattern("^[1-9][0-9]{0,17}$")
string PageNumber

/// 付随する id を出した物: 英小文字・数字・`-`・`.` で 64 字まで（castlist の出し手の名と同じ形）。
@pattern("^[a-z0-9.-]{1,64}$")
string Issuer

/// ページの HTML。
@mediaType("text/html")
blob Html

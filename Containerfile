# pagetray の image。build の段で静的な binary を作り、動かす段は空の image に binary 1 つだけを置く。
# 1.0.0 から、入れるのは新しい server（AI-36 の作り直し。crates/server の pagetray-next）。binary は image の中で
# /pagetray-server の名にする（0.8.x の image と同じ path・同じ ENTRYPOINT・同じ環境変数なので、ci/check-image.sh と
# ci/image-contents.txt もそのまま使える）。0.8.x の DB を移す時は、同じ binary に migrate の引数を付けて起こす
# （docs/src/release.md の「切り替え」）。Containerfile.next は同じ中身で、段 9 で消す。
# build の段の Rust の版は rust-toolchain.toml と合わせる（合わないと、build の中で別の版を取りに行く）。
FROM docker.io/library/rust:1.100.0-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# 作るのは新しい server だけ。新しい server の SQL は動く時に組む形だけなので、.sqlx/ も SQLX_OFFLINE も要らない
# （今の server の src/ は、workspace の頭の crate なので build の中に要るが、作らず、image にも入らない。段 9 で消す）。
RUN cargo build --release --locked -p pagetray-next && mkdir /data

FROM scratch
COPY --from=build /src/target/release/pagetray-next /pagetray-server
# DB の置き場。動かす時に volume を付ける。
COPY --from=build /data /data
ENV PAGETRAY_DB=/data/pages.db PAGETRAY_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/pagetray-server"]

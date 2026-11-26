# pagetray の image。build の段で静的な binary を作り、動かす段は空の image に binary 1 つだけを置く。
# 入れるのは server（crates/server の pagetray-server）。image の中の path は /pagetray-server（ci/check-image.sh と
# ci/image-contents.txt が見る）。0.8.x の DB を移すのは v1.0.0 の image の migrate（docs/src/release.md の「切り替え」）。
# build の段の Rust の版は rust-toolchain.toml と合わせる（合わないと、build の中で別の版を取りに行く）。
FROM docker.io/library/rust:1.100.0-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# 作るのは server だけ（CLI は workspace の member なので build の中に要るが、作らず、image にも入らない）。
RUN cargo build --release --locked -p pagetray-server && mkdir /data

FROM scratch
COPY --from=build /src/target/release/pagetray-server /pagetray-server
# DB の置き場。動かす時に volume を付ける。
COPY --from=build /data /data
ENV PAGETRAY_DB=/data/pages.db PAGETRAY_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/pagetray-server"]

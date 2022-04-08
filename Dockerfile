# syntax=docker/dockerfile:1.4

FROM rust:1.59-bullseye AS rust
FROM gcr.io/distroless/cc-debian11 AS debian
FROM joseluisq/static-web-server:2.7 AS static-web-server

FROM rust AS base
RUN rustup target add wasm32-unknown-unknown
RUN <<-eot
    wget -qO /tmp/cargo-make.zip https://github.com/sagiegurari/cargo-make/releases/download/0.35.10/cargo-make-v0.35.10-x86_64-unknown-linux-musl.zip
    unzip -qqjd /tmp/cargo-make /tmp/cargo-make.zip
    install -Dt /usr/local/cargo/bin /tmp/cargo-make/cargo-make
    rm -rf /tmp/cargo-make /tmp/cargo-make.zip
eot

FROM base AS build
WORKDIR /usr/src/sigma
COPY ./ ./
ARG profile
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/sigma/.cache \
    --mount=type=cache,target=/usr/src/sigma/target \
    cargo make --profile ${profile} build

FROM debian AS sigma-controller
COPY --from=build /usr/src/sigma/build/bin/sigma-controller /
ENTRYPOINT ["/sigma-controller"]

FROM debian AS sigma-server
COPY --from=build /usr/src/sigma/build/bin/sigma-server /
ENTRYPOINT ["/sigma-server"]

FROM static-web-server AS sigma-web-server
COPY apps/sigma/assets/ /public/
COPY --from=build \
    /usr/src/sigma/build/bin/sigma.wasm \
    /usr/src/sigma/build/bin/sigma.js \
    /public/

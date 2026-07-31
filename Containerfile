FROM docker.io/rust:1.97.1-alpine3.24 AS build
ARG TARGETARCH
WORKDIR /build
COPY src /build/src
COPY Cargo.toml /build/Cargo.toml
RUN apk add --no-cache musl-dev
RUN \
  case "${TARGETARCH}" in \
    "amd64") RUSTARCH="x86_64" ;; \
    "arm64") RUSTARCH="aarch64" ;; \
  esac; \
  rustup target add ${RUSTARCH}-unknown-linux-musl; \
  cargo build --release --target ${RUSTARCH}-unknown-linux-musl; \
  cp /build/target/${RUSTARCH}-unknown-linux-musl/release/sentinel /build/sentinel

FROM quay.io/hummingbird/core-runtime:2.43
WORKDIR /app
COPY --from=build /build/sentinel /app/sentinel

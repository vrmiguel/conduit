FROM rust:1.85-alpine as builder

RUN apk update && apk add --no-cache musl-dev openssl-dev gcc git openssl-libs-static linux-headers build-base

ENV RUSTFLAGS="-C target-feature=+crt-static"
ENV PKG_CONFIG_ALLOW_CROSS=1
ENV OPENSSL_STATIC=true

WORKDIR /usr/src
COPY . .

RUN ln -s /usr/bin/x86_64-linux-musl-gcc /usr/bin/musl-gcc
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine

COPY --from=builder /usr/src/target/x86_64-unknown-linux-musl/release/conduit .

CMD ["./conduit"]

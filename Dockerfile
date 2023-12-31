FROM rust:1-alpine as builder

ENV OPENSSL_STATIC=yes
ENV OPENSSL_LIB_DIR=/usr/lib/
ENV OPENSSL_INCLUDE_DIR=/usr/include/

RUN set -x && apk add --no-cache musl-dev openssl-dev openssl-libs-static
WORKDIR /app
COPY . .
RUN cargo install --target=x86_64-unknown-linux-musl --path .

FROM alpine as runner

WORKDIR /app

COPY --from=builder /usr/local/cargo/bin/server_orchestrator /usr/local/bin/server_orchestrator
COPY ./static static
COPY ./templates templates

EXPOSE 8000

ENV RUST_LOG=warn

CMD ["server_orchestrator"]
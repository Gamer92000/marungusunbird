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

COPY --from=builder /usr/local/cargo/bin/marungu_sunbird /usr/local/bin/marungu_sunbird
COPY ./static static
COPY ./templates templates

EXPOSE 8000

ENV LOG_LEVEL=info,rocket::server=warn,ts3_query_api::protocol=debug,rocket_dyn_templates=off,rocket::shield=off,rocket::launch=off

# /health returns 503 while the query connection is down, so the container is
# marked unhealthy when the TeamSpeak connection is dead, not just when the web
# server stops responding. busybox wget fails on HTTP error statuses.
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:${BIND_PORT:-8000}/health" || exit 1

CMD ["marungu_sunbird"]
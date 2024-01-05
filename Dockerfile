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

CMD ["marungu_sunbird"]
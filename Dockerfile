# One static-ish binary on musl, then an image with nothing else in it.
FROM rust:1-alpine AS build

RUN apk add --no-cache musl-dev
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM alpine:3.22

# rustls verifies the Gryzzly certificate against the system store.
RUN apk add --no-cache ca-certificates && \
    adduser -S -u 65532 -H -D gryzzly

COPY --from=build /src/target/release/mcp-gryzzly /usr/local/bin/mcp-gryzzly

USER 65532
# stdio by default; pass --http 0.0.0.0:8080 (with GRYZZLY_HTTP_TOKEN) to serve HTTP.
ENTRYPOINT ["/usr/local/bin/mcp-gryzzly"]

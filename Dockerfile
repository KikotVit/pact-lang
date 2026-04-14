FROM rust:1.88 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/pact /usr/local/bin/pact
WORKDIR /app
ENTRYPOINT ["pact"]

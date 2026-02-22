FROM rust:1.85-bookworm as builder

WORKDIR /build

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

RUN cargo build --release --bin oclaws

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/oclaws /app/

RUN mkdir -p /app/data /app/config

ENV RUST_LOG=info

EXPOSE 8080 8081

ENTRYPOINT ["/app/oclaws"]
CMD ["start"]

# The build and runtime images must be the same Debian release: the binary links
# the build image's glibc dynamically, and a newer builder than runtime fails
# with "GLIBC_x.y not found". `rust:1` alone tracks the newest Debian.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
# Build dependencies first so source edits don't invalidate this layer.
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/time-to-bot /usr/local/bin/time-to-bot
# config/ is mounted at runtime, see docker-compose.yml
CMD ["time-to-bot"]

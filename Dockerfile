FROM rust AS builder
WORKDIR /app

RUN rustup target add x86_64-unknown-linux-musl
RUN apt update && apt install -y musl-tools musl-dev
RUN update-ca-certificates

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "10001" \
    "app"


COPY ./ .

RUN cargo build --target x86_64-unknown-linux-musl --release



FROM scratch
WORKDIR /app

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/time-to-bot ./

USER app:app
CMD ["/app/time-to-bot"]

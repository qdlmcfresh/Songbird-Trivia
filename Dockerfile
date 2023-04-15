FROM rust:latest AS builder
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev libssl-dev libssl-dev ffmpeg youtube-dl pkg-config
RUN cargo install sqlx-cli

WORKDIR /songbird
COPY ./ .
ENV DATABASE_URL="sqlite:db/database.sqlite"
RUN mkdir db
RUN sqlx database setup
RUN cargo build --release

FROM debian:bullseye-slim
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev libssl-dev libssl-dev ffmpeg youtube-dl pkg-config openssl ca-certificates

COPY --from=builder /songbird/target/release/songbird-trivia /usr/local/bin/songbird-trivia
COPY --from=builder /songbird/migrations /project/migrations
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY --from=builder /songbird/run.sh /usr/local/bin/run.sh
WORKDIR /project
CMD /usr/local/bin/run.sh
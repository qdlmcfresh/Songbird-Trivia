FROM rust:latest AS builder
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev libssl-dev libssl-dev ffmpeg wget pkg-config
RUN wget https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux -O /usr/bin/yt-dlp
RUN chmod a+rx /usr/bin/yt-dlp
RUN cargo install sqlx-cli

WORKDIR /songbird
COPY ./ .
ENV DATABASE_URL="sqlite:db/database.sqlite"
RUN mkdir db
RUN sqlx database setup
RUN cargo build --release

FROM debian:bookworm-slim
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev libssl-dev ffmpeg pkg-config openssl ca-certificates

COPY --from=builder /songbird/target/release/songbird-trivia /usr/local/bin/songbird-trivia
COPY --from=builder /songbird/migrations /project/migrations
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY --from=builder /songbird/run.sh /usr/local/bin/run.sh
COPY --from=builder /usr/bin/yt-dlp /usr/local/bin/yt-dlp

WORKDIR /project
CMD /usr/local/bin/run.sh

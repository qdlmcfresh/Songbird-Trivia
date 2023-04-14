FROM mcr.microsoft.com/devcontainers/rust:0-1-bullseye
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev ffmpeg youtube-dl
RUN cargo install sqlx-cli
RUN chmod -R 775 /usr/local/cargo
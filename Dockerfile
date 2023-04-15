FROM rust:slim-bullseye
RUN export DEBIAN_FRONTEND=noninteractive
RUN apt update && apt upgrade -y
RUN apt install -y --no-install-recommends libopus-dev libssl-dev libssl-dev ffmpeg youtube-dl pkg-config
RUN cargo install sqlx-cli
COPY . /project
WORKDIR /project
ENV DATABASE_URL="sqlite:db/database.sqlite"
RUN mkdir db
RUN sqlx database setup
RUN cargo install --path .
RUN cargo clean
RUN rm -rf '$HOME/.cargo/cache'
RUN rm -rf target
CMD /project/run.sh
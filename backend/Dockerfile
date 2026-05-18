FROM rust:slim-bookworm
RUN apt-get update && apt-get install -y libpq-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
WORKDIR /app/backend
RUN cargo install cargo-watch
CMD ["cargo", "watch", "-x", "run"]

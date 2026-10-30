# The pin moves with rust-version by hand (OI-6); its trixie python3 is the 3.13 that tools/parity needs.
FROM rust:1.98.1
COPY --from=ghcr.io/astral-sh/uv:0.11.14 /uv /uvx /usr/local/bin/
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/*

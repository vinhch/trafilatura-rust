# The pin moves with rust-version by hand (OI-6); the CI fuzz job installs the same two things natively.
FROM rust:1.98.1
RUN rustup target add x86_64-unknown-linux-gnuasan \
 && cargo install cargo-fuzz --version 0.13.2 --locked

FROM docker.io/rust:1.97.1 AS build
WORKDIR /build
COPY Cargo.toml /build/Cargo.toml
COPY src /build/src
RUN cargo build --release

FROM quay.io/fedora/fedora-minimal:44
WORKDIR /app
COPY --from=build /build/target/release/mail-proxy /app/mail-proxy
ENTRYPOINT [ "/app/mail-proxy" ]

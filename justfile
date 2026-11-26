alias u := upgrade
alias f := fix
alias c := check
alias c0 := container-prune
alias cu := container-up
alias clu := container-local-up
alias ctu := container-tag-up
alias cdw := container-down

# command list
default:
    @just --list --unsorted

# cargo lib upgrade
upgrade:
    cargo upgrade --incompatible

# cargo sort && lint && fmt
fix:
    cargo sort
    cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features
    cargo fmt

# cargo sort && lint && fmt && test
check:
    cargo sort --check
    CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets --all-features
    cargo fmt --check
    cargo test --all-features

# container prune
container-prune:
    docker system prune --all --force --volumes

# container network && build && up && _prune
container-up:
    docker network create traefik || true
    VERSION=${VERSION:-dev} docker compose build --pull --no-cache
    VERSION=${VERSION:-dev} docker compose up --detach --force-recreate
    @just container-prune

# container _up local
container-local-up:
    @COMPOSE_FILE=compose.yaml:compose.local.yaml just container-up

# git pull && checkout; container _up
container-tag-up tag:
    git pull
    git checkout {{ tag }}
    @VERSION={{ tag }} just container-up
    git checkout -

# container down
container-down:
    docker compose down
    @just container-prune

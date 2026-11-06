# Masthead developer tasks. `just --list` shows them all.

default:
    @just --list

# Debug build of everything
build:
    cargo build --workspace

# Release binaries. The daemon builds on its own so the GUI crates' feature
# choices for shared dependencies don't leak into it.
release:
    cargo build --release -p masthead-daemon -p masthead-bar
    cargo build --release -p masthead-window -p masthead-cosmic

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    typos

# Resting resource budget (DURATION seconds, default 600)
soak duration="600":
    cargo build --release -p masthead-daemon -p masthead-bar
    DURATION={{duration}} scripts/soak.sh

# Install for the current user into ~/.local
install: release
    scripts/install.sh

uninstall:
    scripts/install.sh --uninstall

deb: release
    scripts/package-deb.sh dist

# Zip of the GNOME extension for extensions.gnome.org
extension:
    make -C shell/gnome pack

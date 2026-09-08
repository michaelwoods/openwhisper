.PHONY: all build install test status gui clean

CARGO_HOME ?= $(HOME)/.cache/puccinialin/cargo
CARGO_PATH := $(HOME)/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$(CARGO_HOME)/bin:$(PATH)
PKG_CONFIG_PATH ?= $(HOME)/.local/lib/pkgconfig:$(HOME)/.local/share/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig

all: build

build:
	@echo "==> Building OpenWhisper in release mode..."
	export CARGO_HOME="$(CARGO_HOME)"; export PATH="$(CARGO_PATH)"; export PKG_CONFIG_PATH="$(PKG_CONFIG_PATH)"; cargo build --release

install:
	@./scripts/install.sh

test:
	@echo "==> Running OpenWhisper test suite..."
	export CARGO_HOME="$(CARGO_HOME)"; export PATH="$(CARGO_PATH)"; export PKG_CONFIG_PATH="$(PKG_CONFIG_PATH)"; cargo test

status:
	@XDG_RUNTIME_DIR=/run/user/$$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$$(id -u)/bus systemctl --user status openwhisper.service --no-pager || true
	@openwhisper status

gui:
	@echo "==> Launching OpenWhisper Settings GUI..."
	export CARGO_HOME="$(CARGO_HOME)"; export PATH="$(CARGO_PATH)"; export PKG_CONFIG_PATH="$(PKG_CONFIG_PATH)"; cargo run -- config-gui

clean:
	export CARGO_HOME="$(CARGO_HOME)"; export PATH="$(CARGO_PATH)"; export PKG_CONFIG_PATH="$(PKG_CONFIG_PATH)"; cargo clean

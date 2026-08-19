CARGO ?= cargo
PREFIX ?= ~/.local
BINDIR = $(DESTDIR)$(PREFIX)/bin

.PHONY: all check build install test

all: build

## Format, lint, and test everything.
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test

build:
	$(CARGO) build --release

## Install the release binary into $(BINDIR).
install: build
	install -d "$(BINDIR)"
	install -m 0755 target/release/clining "$(BINDIR)/clining"
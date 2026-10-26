CARGO ?= cargo
PREFIX ?= ~/.local
BINDIR = $(DESTDIR)$(PREFIX)/bin
VERSION ?= $(shell $(CARGO) metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "clining") | .version')
TARGET_TRIPLE ?= $(shell rustc -vV | sed -n 's/^host: //p')
DIST_DIR = dist
DIST_ASSET = $(DIST_DIR)/clining-v$(VERSION)-$(TARGET_TRIPLE).tar.gz

.PHONY: all check build install dist print-dist-asset test

all: build

## Format, lint, and test everything.
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test

build:
	$(CARGO) build --release

## Package the release binary into $(DIST_DIR).
dist: build
	mkdir -p "$(DIST_DIR)"
	tar -czf "$(DIST_ASSET)" -C target/release clining

## Print the release asset path without building it.
print-dist-asset:
	@echo "$(DIST_ASSET)"

## Install the release binary into $(BINDIR).
install: build
	install -d "$(BINDIR)"
	install -m 0755 target/release/clining "$(BINDIR)/clining"

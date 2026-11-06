# Copyright 2026 Kitefold Labs
# SPDX-License-Identifier: Apache-2.0

UUID := masthead@kitefoldlabs.com
SOURCES := client.js draw.js format.js popover.js
ICONS := $(wildcard icons/*-symbolic.svg)
ZIP := $(UUID).shell-extension.zip
DEST := $(or $(XDG_DATA_HOME),$(HOME)/.local/share)/gnome-shell/extensions/$(UUID)

.PHONY: pack install uninstall check clean

# Build the zip that extensions.gnome.org and `gnome-extensions install` accept.
pack: $(ZIP)

$(ZIP): extension.js metadata.json stylesheet.css $(SOURCES) $(ICONS)
	gnome-extensions pack --force \
	  $(foreach f,$(SOURCES),--extra-source=$(f)) \
	  --extra-source=icons .

# Install for the current user. On Wayland, log out and back in, then
# `gnome-extensions enable $(UUID)`.
install: pack
	gnome-extensions install --force $(ZIP)

uninstall:
	gnome-extensions uninstall $(UUID)

# Syntax-check every module (needs node).
check:
	@for f in extension.js $(SOURCES); do node --check --input-type=module < $$f || exit 1; done
	@echo "syntax ok"

clean:
	rm -f $(ZIP)

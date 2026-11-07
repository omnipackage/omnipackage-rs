CC ?= cc
INSTALL_ROOT ?=

all: omnipackage-macos-hello

omnipackage-macos-hello: main.c version.h
	$(CC) -O2 -o omnipackage-macos-hello main.c

install: omnipackage-macos-hello
	install -d -m755 $(INSTALL_ROOT)/usr/bin/
	install -m755 omnipackage-macos-hello $(INSTALL_ROOT)/usr/bin/

clean:
	rm -f omnipackage-macos-hello

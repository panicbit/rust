-include ../tools.mk

# Apparently older versions of GCC segfault if -g is passed...
CC := $(CC:-g=)

all:
	$(RUSTC) foo.rs -C lto
	$(CC) bar.c $(TMPDIR)/libfoo.a \
		$(call OUT_EXE,bar) \
		$(EXTRACFLAGS) $(EXTRACXXFLAGS)
	$(call RUN,bar)

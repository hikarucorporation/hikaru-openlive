# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Makefile de Hikaru OpenLive.
#
# Dos cosas: un atajo para los comandos de `cargo` que se usan todos los
# días, y la instalación de la PÁGINA DE MANUAL del binario `hikaru_openlive`
# (la que hace que `man hikaru_openlive` funcione en vez de decir "No manual
# entry for hikaru_openlive").
#
# `make help` lista los objetivos.

CARGO ?= cargo
BIN   ?= hikaru_openlive

# --- La página de manual --------------------------------------------------
#
# El archivo CANÓNICO vive en el crate (`crates/hikaru_cli/man/`): es el que
# viaja con el crate y el que copia el `build.rs` a `$OUT_DIR/man/` para el
# empaquetado. `docs/man/` es el ESPEJO que muestra la documentación.
MAN_NAME    := hikaru_openlive
MAN_SRC     := crates/hikaru_cli/man/$(MAN_NAME).1
MAN_DOC_SRC := docs/man/$(MAN_NAME).1
MAN_PACK    := target/man/$(MAN_NAME).1.gz

# Dónde se instala. `DESTDIR` y `PREFIX` son los de siempre (los usa un
# empaquetador para armar un .deb o un paquete de FreeBSD).
PREFIX ?= /usr/local
DESTDIR ?=
MANDIR ?= $(PREFIX)/share/man
MAN1DIR    := $(DESTDIR)$(MANDIR)/man1

# La instalación de usuario: no hace falta root y `man` la encuentra si
# `$HOME/.local/share/man` está en el MANPATH (en la mayoría de las
# distribuciones ya lo está).
USER_MAN1DIR ?= $(HOME)/.local/share/man/man1

.PHONY: help all build release test run \
        man man-render man-check man-sync man-pack \
        man-install man-install-user man-uninstall clean

# --- La ayuda --------------------------------------------------------------

help:
	@echo "Hikaru OpenLive — objetivos disponibles:"
	@echo ""
	@echo "  build             Compila el binario $(BIN) (debug)"
	@echo "  release           Compila el binario $(BIN) (release)"
	@echo "  test              Corre los tests del crate de la consola"
	@echo "  run               Levanta la GUI (o la consola, según los args)"
	@echo ""
	@echo "  man               Muestra la página de manual"
	@echo "  man-render        La renderiza en texto plano (sin paginador)"
	@echo "  man-check         La verifica con groff (sin avisos)"
	@echo "  man-sync          Copia la página canónica a docs/man/"
	@echo "  man-pack          Empaqueta la página comprimida (target/man/)"
	@echo "  man-install       La instala en $(MAN1DIR) (hace falta root)"
	@echo "  man-install-user  La instala en $(USER_MAN1DIR) (sin root)"
	@echo "  man-uninstall     La saca de los dos directorios"
	@echo "  clean             Borra target/"

# --- El build --------------------------------------------------------------

all: build

build:
	$(CARGO) build --bin $(BIN)

release:
	$(CARGO) build --release --bin $(BIN)

test:
	$(CARGO) test -p hikaru_cli

run:
	$(CARGO) run --bin $(BIN) -- $(ARGS)

# --- La página de manual ---------------------------------------------------

man: man-render

# La renderiza sin paginador: sirve para ver qué va a salir en la terminal.
man-render:
	@if command -v man >/dev/null 2>&1; then \
		MANPAGER=cat MANWIDTH=80 man -l $(MAN_SRC); \
	else \
		groff -K utf8 -man -Tutf8 $(MAN_SRC) | cat; \
	fi

# La verifica: groff se queja de los `.TH` mal formados, de los saltos de
# paginador y de los caracteres inválidos.
man-check:
	@if command -v groff >/dev/null 2>&1; then \
		groff -K utf8 -man -Tutf8 -ww -z $(MAN_SRC) && \
		echo "man: la página no tiene avisos de groff"; \
	else \
		echo "man: groff no está instalado: no se puede verificar la página"; \
	fi

# Sincroniza el espejo de la documentación con el archivo canónico.
man-sync:
	@mkdir -p $(dir $(MAN_DOC_SRC))
	@cp $(MAN_SRC) $(MAN_DOC_SRC)
	@echo "man: $(MAN_DOC_SRC) sincronizado con $(MAN_SRC)"

# El paquete comprimido: es lo que se instala en /usr/share/man (el `man` de
# la mayoría de las distribuciones lee los `.1.gz`).
man-pack:
	@mkdir -p $(dir $(MAN_PACK))
	@gzip -9 -c $(MAN_SRC) > $(MAN_PACK)
	@echo "man: $(MAN_PACK)"

# La instalación en el sistema (hace falta root o sudo).
man-install:
	@mkdir -p $(MAN1DIR)
	@cp $(MAN_SRC) $(MAN1DIR)/$(MAN_NAME).1
	@chmod 644 $(MAN1DIR)/$(MAN_NAME).1
	@echo "man: instalada en $(MAN1DIR)/$(MAN_NAME).1"

# La instalación en el directorio del usuario (no hace falta root).
man-install-user:
	@mkdir -p $(USER_MAN1DIR)
	@cp $(MAN_SRC) $(USER_MAN1DIR)/$(MAN_NAME).1
	@chmod 644 $(USER_MAN1DIR)/$(MAN_NAME).1
	@echo "man: instalada en $(USER_MAN1DIR)/$(MAN_NAME).1"
	@echo "man: para verla, '$(HOME)/.local/share/man' tiene que estar en el MANPATH"

man-uninstall:
	@rm -f $(MAN1DIR)/$(MAN_NAME).1 $(USER_MAN1DIR)/$(MAN_NAME).1
	@echo "man: desinstalada"

clean:
	$(CARGO) clean

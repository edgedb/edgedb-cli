.PHONY: all

ROOT = $(dir $(realpath $(firstword $(MAKEFILE_LIST))))

all: workflows/nightly.yml

workflows/%.yml: workflows.src/%.tpl.yml workflows.src/%.targets.yml
	$(ROOT)/workflows.src/render.py $* $*.targets.yml

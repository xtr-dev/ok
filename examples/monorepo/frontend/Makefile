.PHONY: build test

node_modules: package-lock.json
	npm ci
	touch node_modules

build: node_modules
	npm run build

test: node_modules
	npm test

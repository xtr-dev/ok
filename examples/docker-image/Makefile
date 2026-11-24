# Build, push and run the API container image.
IMAGE   ?= ghcr.io/example/api
VERSION := $(shell git describe --tags --always --dirty)
SHA     := $(shell git rev-parse --short HEAD)
PORT    ?= 8080

.PHONY: image push serve

image:
	docker build --build-arg VERSION=$(VERSION) -t $(IMAGE):$(SHA) -t $(IMAGE):$(VERSION) .

push: image
	@if echo "$(VERSION)" | grep -q -- '-dirty$$'; then \
	  echo "refusing to push a dirty build ($(VERSION))"; exit 1; \
	fi
	docker push $(IMAGE):$(SHA)
	docker push $(IMAGE):$(VERSION)

serve: image
	docker run --rm -p $(PORT):8080 --env-file .env $(IMAGE):$(SHA)

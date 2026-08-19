.PHONY: build test vet windows validate

build:
	go build -tags migrated_fynedo -o bin/spotlight ./cmd/spotlight

windows:
	CGO_ENABLED=1 GOOS=windows GOARCH=amd64 CC=x86_64-w64-mingw32-gcc \
		go build -tags migrated_fynedo -ldflags "-H windowsgui" -o bin/spotlight.exe ./cmd/spotlight

test:
	go test ./...

vet:
	go vet ./...

validate:
	openspec validate spotlight-launcher
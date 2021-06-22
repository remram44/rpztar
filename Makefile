.PHONY: all builder build test

all: builder build testimg

builder: builder.Dockerfile
	docker build -t rsstatic-build - <builder.Dockerfile

build: rpztar

rpztar: target/x86_64-unknown-linux-musl/release/rpztar
	strip --strip-unneeded target/x86_64-unknown-linux-musl/release/rpztar -o rpztar

target/x86_64-unknown-linux-musl/release/rpztar: builder
	docker run -t --rm -v $(shell pwd):/src -v $(shell pwd)/.docker-cargo-registry-cache:/root/.cargo/registry rsstatic-build cargo build --target x86_64-unknown-linux-musl --release

testimg: build
	docker build -t rsstatic-test -f test.Dockerfile .

test: testimg
	docker run -ti --rm -v $(shell pwd)/test.tar.gz:/test.tar.gz -v $(shell pwd)/test.list:/test.list rsstatic-test /rpztar test.tar.gz test.list

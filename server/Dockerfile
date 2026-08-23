# build stage
FROM golang:1.27 as builder

ENV CGO_ENABLED=0

RUN apt-get -qq update && \
    apt-get install -yqq upx

WORKDIR /build
COPY . .

ARG BUILD_TIME=unknown
ARG GITHASH=unknown
ARG BUILD_TAG=dev

RUN go build \
    -ldflags "-X main.buildStamp=${BUILD_TIME} -X main.gitHash=${GITHASH} -X main.buildTag=${BUILD_TAG}" \
    -o app
RUN strip ./app
RUN upx -q -9 ./app

# ---
FROM scratch

# COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/app .

# EXPOSE 8080

ENTRYPOINT ["./app"]

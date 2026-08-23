#!/bin/bash

GOARCH=arm64 GOOS=linux go build
scp vraptor_monitor_gauge vraptor@vraptor.local:~/
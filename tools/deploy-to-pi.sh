#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail
set -o xtrace

readonly TARGET_HOST=twixelwall@twixelwall
readonly TARGET_PORT=2222
readonly TARGET_PATH=/home/twixelwall/bin/twixelwall-bot
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/twixelwall-bot

readonly DOCKER_LABEL=rust-xcompile-rpi

docker build -t ${DOCKER_LABEL} tools/rust-xcompile-docker/
docker run --rm --user $(id -u):$(id -g) -v $(pwd):/usr/src/twixelwall-bot -v ${HOME}/cargopi:/usr/local/cargo/registry -w /usr/src/twixelwall-bot ${DOCKER_LABEL} ./tools/compile-rpi-binary.sh
rsync -e "ssh -p ${TARGET_PORT}" ${SOURCE_PATH} ${TARGET_HOST}:${TARGET_PATH}

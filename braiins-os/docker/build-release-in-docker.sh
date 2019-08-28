#!/bin/bash

set -e

MONOREPO_DIR=/src
BOS_DIR=braiins-os
RELEASE_BUILD_DIR=$MONOREPO_DIR/$BOS_DIR

DOCKER_SSH_AUTH_SOCK=/ssh-agent
USER_NAME=build
HOST_ROOT_DIR=$PWD/..

if [ $# -eq 0 ]; then
    echo "Warning: Missing build release parameters!"
    echo "Running only braiins OS build environment..."
else
    ARGS="./build-release.sh $@"
fi

docker run -it --rm \
    -v $HOME/.ssh/known_hosts:/home/$USER_NAME/.ssh/known_hosts:ro \
    -v $SSH_AUTH_SOCK:$DOCKER_SSH_AUTH_SOCK -e SSH_AUTH_SOCK=$DOCKER_SSH_AUTH_SOCK \
    -v $HOST_ROOT_DIR:$MONOREPO_DIR -w $RELEASE_BUILD_DIR \
    -e RELEASE_BUILD_DIR=$RELEASE_BUILD_DIR \
    $USER/bos-builder $ARGS

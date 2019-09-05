#!/bin/bash

# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

set -e

DOCKERFILE_DIR=$(dirname "$0")
USER_UID=$(id -u)
USER_GID=$(id -g)

docker_dir=$(mktemp -d)
cp "./$DOCKERFILE_DIR/Dockerfile" "$docker_dir"
cp "./$DOCKERFILE_DIR/bashrc" "$docker_dir"
cp "./requirements.txt" "$docker_dir"

md5sum "./requirements.txt" > "$docker_dir/requirements.md5"

docker build --build-arg LOC_UID=$USER_UID --build-arg LOC_GID=$USER_GID \
    -t $USER/bos-builder "$docker_dir"

rm -fr "$docker_dir"

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

# mounted inside dev container for git-ssh to work
DOCKER_SSH_AUTH_SOCK=/ssh_auth.sock

# NOTE: $CI_REGISTRY_IMAGE need to be set to docker registry

# increment tag here and in ci config to enforce gitlab runners to fetch new images
TAG=5

# prepare image
build:
	test -n "$(CI_REGISTRY_IMAGE)"
	docker build -t $(CI_REGISTRY_IMAGE)/rust:$(TAG) .

# push image to gitlab
push: build
	test -n "$(CI_REGISTRY_IMAGE)"
	docker push $(CI_REGISTRY_IMAGE)/rust:$(TAG)

# spawn container for testing.
# once inside you need to install git and clone git@gitlab.ii.zone:x/braiins.git
enter: build
	test -n "$(CI_REGISTRY_IMAGE)"
	docker run --rm -ti --pid=host \
	-v $(HOME)/.ssh/known_hosts:$(HOME)/.ssh/known_hosts:ro \
	-v $(SSH_AUTH_SOCK):$(DOCKER_SSH_AUTH_SOCK) \
	-e SSH_AUTH_SOCK=$(DOCKER_SSH_AUTH_SOCK) \
	$(CI_REGISTRY_IMAGE)/rust:$(TAG) /bin/bash

.PHONY: build push enter

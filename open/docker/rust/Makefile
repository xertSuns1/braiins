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

NAME=rust
# increment tag here and in ci config to enforce gitlab runners to fetch new images
TAG=10
# place for clean git checkout
# note that this prevents seeing/using (yet) uncomitted changes in image
# we use stable path to be able to clean up properly in case of failed build
# this is obviously not conductive of concurent builds, if there is ever need of one
TMPTREE=/tmp/docker-build-$(NAME)-$(TAG)

# prepare image
build:
	test -n "$(CI_REGISTRY_IMAGE)"
	# clean up if previous build failed, in a way compatible with older git versions
	rm -fr $(TMPTREE)
	git worktree prune
	git worktree add $(TMPTREE) origin/bos-devel
	# production image
	docker build --no-cache -f Dockerfile -t $(CI_REGISTRY_IMAGE)/$(NAME):$(TAG) $(TMPTREE)
	# dev image with sources layer
	docker build -f Dockerfile --target=rust-with-sources -t $(CI_REGISTRY_IMAGE)/$(NAME)-src:$(TAG) $(TMPTREE)
	rm -fr $(TMPTREE)
	git worktree prune

# push image to gitlab
push: build
	test -n "$(CI_REGISTRY_IMAGE)"
	docker push $(CI_REGISTRY_IMAGE)/$(NAME):$(TAG)

# spawn container for testing.
# clean bos-devel branch is at /src
enter: build
	test -n "$(CI_REGISTRY_IMAGE)"
	docker run --rm -ti --pid=host \
	-v $(HOME)/.ssh/known_hosts:$(HOME)/.ssh/known_hosts:ro \
	-v $(SSH_AUTH_SOCK):$(DOCKER_SSH_AUTH_SOCK) \
	-e SSH_AUTH_SOCK=$(DOCKER_SSH_AUTH_SOCK) \
	$(CI_REGISTRY_IMAGE)/$(NAME)-src:$(TAG) /bin/bash

.PHONY: build push enter


# How to set up gitlab countinuous integration to run basic checks

* Enable container registry ( https://docs.gitlab.com/ee/user/packages/container_registry/index.html )
* Set up some runners
* Set a path to registry in Makefile (GITLAB_HOST) and run `make push`
* Use the image in pipelines defined in `.gitlab-ci.yml`, prefixed with $CI_REGISTRY_IMAGE.

# Running in local container

If pipeline is failing on you, it is far easier to debug interactively in local container and
push tweaked image/commit tested pipeline config.

1. `export CI_REGISTRY_IMAGE=…` to point to docker registry (host/path, not image name)
2. `make enter` will fire up local docker container
3. `apt-get update && apt-get install -y git`
4. `git clone …` code to work with
5. Do whatever, preferably copy commands directly from .gitlab-ci.yml
…

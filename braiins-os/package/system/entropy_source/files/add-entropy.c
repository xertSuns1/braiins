/*
 * Copyright (C) 2020  Braiins Systems s.r.o.
 *
 * This file is part of Braiins Open-Source Initiative (BOSI).
 *
 * BOSI is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * Please, keep in mind that we may also license BOSI or any part thereof
 * under a proprietary license. For more information on the terms and conditions
 * of such proprietary license or if you have any other questions, please
 * contact us at opensource@braiins.com.
 */

/* Insert entropy from stdin to /dev/random */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <linux/random.h>
#include <assert.h>

#define RANDOM_DEV "/dev/random"

void
pexit(const char *msg)
{
	perror(msg);
	exit(1);
}

int
get_entropy_count(int fd)
{
	int entropy = -1;
	int ret;

	ret = ioctl(fd, RNDGETENTCNT, &entropy);
	if (ret < 0)
		pexit("get_entropy_count");
	assert(entropy != -1);
	return entropy;
}

void
add_entropy(int fd, uint8_t *buf, int len)
{
	int ret;
	struct rand_pool_info *rp;

	rp = calloc(sizeof(*rp) + len, 1);
	assert(rp);
	memcpy(rp->buf, buf, len);
	rp->entropy_count = len * 8;
	rp->buf_size = len;

	ret = ioctl(fd, RNDADDENTROPY, rp);
	if (ret < 0)
		pexit("add_entropy");

	printf("added %d bytes of entropy\n", len);
}

int
main(int argc, char *argv[])
{
	int random_fd, ret;
	uint8_t buf[128];

	random_fd = open(RANDOM_DEV, O_RDONLY);
	if (random_fd < 0)
		pexit(RANDOM_DEV);

	printf("input_entropy = %d\n", get_entropy_count(random_fd));
	while ((ret = read(0, buf, sizeof(buf))) > 0) {
		add_entropy(random_fd, buf, ret);
	}
	printf("output_entropy = %d\n", get_entropy_count(random_fd));

	return 0;
}

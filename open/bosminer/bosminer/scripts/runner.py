#!/usr/bin/env python3

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

import os
import sys
import argparse
import toml
import string

from fabric import Connection

TARGET_DIR = '/tmp'
CONFIG_FILE = 'Test.toml'
ARG_HOSTNAME = '--hostname'
DEFAULT_USER = 'root'
CONFIG_PATHS = ['.', '..']


class RunnerError(Exception):
    pass


def is_test_harness(path):
    # each test ends with 16 chars long hexadecimal number
    suffix = (path.rsplit('-', 1) + [None])[0:2][1]
    return suffix and len(suffix) == 16 and set(suffix).issubset(string.hexdigits)


def main(argv):
    parser = argparse.ArgumentParser()

    parser.add_argument('test',
                        help='path to executable with the test')
    parser.add_argument('--user',
                        help='name of pool worker')
    parser.add_argument(ARG_HOSTNAME,
                        help='ip address or hostname of remote bOS device with ssh server')

    # parse command line arguments
    (args, extra_args) = parser.parse_known_args(argv)

    cfg_user = None
    cfg_hostname = None

    # construct all config file locations
    cfg_locations = [os.path.join(dir, CONFIG_FILE) for dir in CONFIG_PATHS]
    cfg_path = next((path for path in cfg_locations if os.path.isfile(path)), None)

    if cfg_path is not None:
        # try to get default configuration from configuration file
        config = toml.load(cfg_path)
        remote = config.get('remote')
        if remote:
            cfg_user = remote.get('user')
            cfg_hostname = remote.get('hostname')

    # get remote settings
    user = args.user or cfg_user or DEFAULT_USER
    hostname = args.hostname or cfg_hostname

    if not hostname:
        print("Missing remote hostname which can be specified by '{}' argument or in '{}'"
              .format(ARG_HOSTNAME, CONFIG_FILE))
        raise RunnerError

    test = args.test
    test_name = os.path.basename(test)

    # disable parallelism in tests
    remote_argv = ['--test-threads', '1'] if is_test_harness(test) else []
    remote_argv += extra_args

    try:
        with Connection('{}@{}'.format(user, hostname)) as c:
            c.put(test, TARGET_DIR)
            result = c.run('{}/{} {}'.format(TARGET_DIR, test_name, ' '.join(remote_argv)), pty=True, warn=True)
    except Exception as e:
        print('{}'.format(e))
        raise RunnerError

    if result.failed:
        raise RunnerError


if __name__ == "__main__":
    # execute only if run as a script
    try:
        main(sys.argv[1:])
    except RunnerError:
        sys.exit(1)

#!/usr/bin/env python3

import os
import sys
import argparse
import toml

from fabric import Connection

TARGET_DIR = '/tmp'
CONFIG_FILE = 'Test.toml'
ARG_HOSTNAME = '--hostname'
DEFAULT_USER = 'root'


class RunnerError(Exception):
    pass


def main(argv):
    parser = argparse.ArgumentParser()

    parser.add_argument('test',
                        help='path to executable with the test')
    parser.add_argument('filter', nargs='?',
                        help='run only tests whose names contain the filter')
    parser.add_argument('--user',
                        help='name of pool worker')
    parser.add_argument(ARG_HOSTNAME,
                        help='ip address or hostname of remote bOS device with ssh server')

    # parse command line arguments
    args = parser.parse_args(argv)

    cfg_user = None
    cfg_hostname = None

    if os.path.isfile(CONFIG_FILE):
        # try to get default configuration from configuration file
        config = toml.load(CONFIG_FILE)
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
    remote_argv = ['--test-threads', '1']
    if args.filter:
        remote_argv.append(args.filter)

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

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

from itertools import chain
from collections import OrderedDict


class Package:
    # feeds index constants
    FEEDS_ATTR_PACKAGE = 'Package'
    FEEDS_ATTR_FILENAME = 'Filename'
    FEEDS_ATTR_VERSION = 'Version'
    FEEDS_ATTR_REQUIRE = 'Require'
    FEEDS_EXCLUDED_ATTRIBUTES = ['Source', 'Maintainer']

    @property
    def name(self):
        return self._attributes.get(self.FEEDS_ATTR_PACKAGE)

    @property
    def filename(self):
        return self._attributes.get(self.FEEDS_ATTR_FILENAME)

    @property
    def version(self):
        return self._attributes.get(self.FEEDS_ATTR_VERSION)

    @property
    def require(self):
        return self._attributes.get(self.FEEDS_ATTR_REQUIRE)

    @require.setter
    def require(self, require):
        self._attributes[self.FEEDS_ATTR_REQUIRE] = require

    def __init__(self, attributes):
        self._attributes = attributes

    def __hash__(self):
        return hash((self.name, self.version))

    def __eq__(self, other):
        if not isinstance(other, type(self)):
            return NotImplemented
        return self.name == other.name and self.version == other.version

    def __lt__(self, other):
        return self.filename < other.filename

    def __iter__(self):
        for attribute, value in self._attributes.items():
            if attribute not in self.FEEDS_EXCLUDED_ATTRIBUTES:
                yield attribute, value


class Packages:
    """
    Class for parsing LEDE feeds index with packages
    """
    def __init__(self, path, input=None):
        """
        Initialize parser with path to feeds index file

        :param path:
            File path to feeds index file.
        """
        self._path = path
        self._input = input

    def __enter__(self):
        """
        Open feeds index file

        :return:
            Feeds index file parser.
        """
        self._input = open(self._path, 'r')
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """
        Close previously opend feeds index file
        """
        self._input.close()

    def _get_package_record(self, stream):
        """

        :param stream:
        :return:
        """
        attributes = OrderedDict()
        attribute = None
        value = None
        for line in stream:
            if not len(line) or line[0] == '\n':
                # end of record
                break
            if not line[0].isspace():
                # found new package attribute
                if attribute:
                    # store previous attribute
                    attributes[attribute] = value
                    if attribute == Package.FEEDS_ATTR_VERSION and not attributes.get(Package.FEEDS_ATTR_REQUIRE):
                        attributes[Package.FEEDS_ATTR_REQUIRE] = None
                # attribute has format 'name: value\n'
                attribute, value = line.split(': ', 1)
                # remove newline
                value = value.rstrip()
            else:
                # when newline starts with space then previous attribute value continues
                value = '{}\n{}'.format(value, line.rstrip())
        if attribute:
            # store previous attribute
            attributes[attribute] = value
        if Package.FEEDS_ATTR_REQUIRE not in attributes:
            attributes[Package.FEEDS_ATTR_REQUIRE] = None
        return Package(attributes)

    def __iter__(self):
        """
        Iterate through all package records in feeds index file

        :return:
            Ordered dictionary with attribute records for one package.
        """
        while True:
            # find first attribute
            for line in self._input:
                if len(line) and not line[0].isspace():
                    break
            else:
                # no more data so break outer cycle
                break
            # read the whole record
            yield self._get_package_record(chain((line,), self._input))

#!/usr/bin/env sh

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

if [ ! -L "/sys/class/gpio/gpio943" ]; then
	echo 943 > /sys/class/gpio/export
	echo out > /sys/class/gpio/gpio943/direction
	echo 944 > /sys/class/gpio/export
	echo out > /sys/class/gpio/gpio944/direction
	echo 945 > /sys/class/gpio/export
	echo out > /sys/class/gpio/gpio945/direction
	echo 953 > /sys/class/gpio/export
	echo in > /sys/class/gpio/gpio953/direction
	echo 957 > /sys/class/gpio/export
	echo in > /sys/class/gpio/gpio957/direction
fi

bmminer --fixed-freq --no-pre-heat --api-listen --default-config /etc/bmminer.conf

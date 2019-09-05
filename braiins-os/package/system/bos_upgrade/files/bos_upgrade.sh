#!/bin/sh

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

# redirect STDOUT and STDERR to /dev/kmsg
exec 1<&- 2<&- 1>/dev/kmsg 2>&1

set -e

FIRMWARE_DIR="/tmp/firmware"
UPGRADE_SCRIPT="./stage2.sh"

echo "Start braiins/LEDE firmware upgrade process..."

# try to set LEDs to signal recovery mode
echo timer > "/sys/class/leds/Green LED/trigger"
echo nand-disk > "/sys/class/leds/Red LED/trigger"

FIRMWARE_OFFSET=$(fw_printenv -n stage2_off 2> /dev/null)
FIRMWARE_SIZE=$(fw_printenv -n stage2_size 2> /dev/null)
FIRMWARE_MTD=/dev/mtd$(fw_printenv -n stage2_mtd 2> /dev/null)

# get stage2 firmware images from NAND
mkdir -p "$FIRMWARE_DIR"
cd "$FIRMWARE_DIR"

nanddump -s ${FIRMWARE_OFFSET} -l ${FIRMWARE_SIZE} ${FIRMWARE_MTD} \
| tar zx

# rather check error in script
set +e

if /bin/sh "$UPGRADE_SCRIPT" ; then
	echo "Upgrade has been successful!"

	# reboot system
	echo "Restarting system..."
	sync
	reboot
else
    echo "Upgrade failed"
fi

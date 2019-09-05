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

echo "System is running the bOSminer preview!"

green_led="/sys/class/leds/Green LED"
red_led="/sys/class/leds/Red LED"

# after successful boot, turn off the red LED and green LED let turned on
echo default-on > "$green_led/trigger"
echo none > "$red_led/trigger"

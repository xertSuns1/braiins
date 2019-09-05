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

gpio_write() {
	local p=/sys/class/gpio/gpio$1/value 
	if [ -f "$p" ]; then 
		echo $2 > $p
		return 0
	else
		return 1
	fi
}

chain=0
echo "RESET=0"
for pin in 855 857 859 861 863 865; do
	let chain=chain+1
	if  gpio_write $pin 0; then
		echo "chain $chain present"
	else
		echo "chain $chain not present"
	fi
done
sleep 1
echo "START_EN=0"
for pin in 854 856 858 860 862 864; do
	gpio_write $pin 0
done
sleep 1
echo "POWER=0"
for pin in 872 873 874 875 876 877; do
	gpio_write $pin 0 && sleep 1
done
echo "LED=1"
for pin in 881 882 883 884 885 886; do
	gpio_write $pin 1
done

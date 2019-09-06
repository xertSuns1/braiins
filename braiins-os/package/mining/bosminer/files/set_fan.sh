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

set -e

PWM=$1

if [ "$PWM" = "" ]; then
	echo "usage: $0 <pwm_in_percent>" 1>&2
	echo "0 means fan off, 100 fan full on" 1>&2
	exit 1
fi
if ! [ "$PWM" -ge 0 -a "$PWM" -le 100 ]; then
	echo "fan pwm value out of range ($PWM is not within 0..100)" 1>&2
	exit 1
fi

FAN_PWM_DEV=/sys/class/pwm/pwmchip0
FAN_PERIOD=40000
FAN_DUTY_CYCLE=$((($FAN_PERIOD*(100-$PWM))/100))

cd "$FAN_PWM_DEV"
if ! [ -d pwm0 ]; then
	echo 0 > export
fi
cd pwm0
echo $FAN_PERIOD > period
echo $FAN_DUTY_CYCLE > duty_cycle
echo 1 > enable

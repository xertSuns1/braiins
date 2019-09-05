####################################################################################################
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
####################################################################################################

####################################################################################################
# Fan procedures
####################################################################################################
set FAN_A 0x42800000
set FAN_B 0x42810000
set FAN_C 0x42820000

set DUTY_MAX 1997

proc fan_init {base_addr} {
    # set frequency to 25kHz
    mwr [expr $base_addr + 0x04] 1998
    # set duty cycle to 100%
    mwr [expr $base_addr + 0x14] 0
    # enable timers and PWM generations
    mwr [expr $base_addr + 0x00] 0x206
    mwr [expr $base_addr + 0x10] 0x606
}

proc fan_duty {base_addr percent} {
    global DUTY_MAX
    mwr [expr $base_addr + 0x14] [expr $DUTY_MAX * (100 - $percent) / 100]
}

# test of PWM module for fan - configure core and set different speed of fan
proc test_fan {name base_addr} {
    fan_init $base_addr

    puts -nonewline "Test of $name, speed set to 100%"
    flush stdout
    exec sleep 2

    # set duty cycle to 50%
    fan_duty $base_addr 50
    puts -nonewline "..50%"
    flush stdout
    exec sleep 2

    # set duty cycle to 25%
    fan_duty $base_addr 25
    puts -nonewline "..25%"
    flush stdout
    exec sleep 2

    # set duty cycle to 0%
    fan_duty $base_addr 0
    puts -nonewline "..0%"
    flush stdout
    exec sleep 2

    # set duty cycle to 100%
    fan_duty $base_addr 100
    puts "..100%"
}

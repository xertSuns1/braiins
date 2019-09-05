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
# Check input arguments
####################################################################################################
# check number of arguments
if {$argc == 1} {
    set board [lindex $argv 0]
} else {
    puts "Wrong number of TCL arguments! Expected 1 argument, get $argc"
    puts "List of arguments: $argv"
    exit 1
}

# check name of the board
if { !(($board == "G9") || ($board == "G19") || ($board == "G29")) } {
    puts "Unknown board: $board"
    puts "Only supported boards are G9, G19 and G29!"
    exit 1
}

puts "Board name: $board"

# Project directory
set projdir "./build_$board"

####################################################################################################
# Control board initialization
####################################################################################################
connect arm hw
fpga -f ${projdir}/results/system.bit
source ${projdir}/results/system/ps7_init.tcl
ps7_init
ps7_post_config

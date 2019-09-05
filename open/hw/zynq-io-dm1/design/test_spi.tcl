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
# SPI procedures
####################################################################################################
# test of SPI module - configure core and send one byte
proc test_spi {name base_addr data} {
    puts "Test of $name"
    # reset of SPI core
    mwr [expr $base_addr + 0x40] 0x0A
    # enable SPI core
    mwr [expr $base_addr + 0x60] 0x086
    # select CS (set to 0)
    mwr [expr $base_addr + 0x70] 0x0
    # sent data (1 byte)
    mwr [expr $base_addr + 0x68] $data
    # deselect CS (set to 1)
    mwr [expr $base_addr + 0x70] 0x1
}

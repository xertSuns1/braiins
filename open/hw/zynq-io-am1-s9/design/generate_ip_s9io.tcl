####################################################################################################
# Copyright (c) 2018 Braiins Systems s.r.o.
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.
####################################################################################################

####################################################################################################
# Generate IP core s9io
####################################################################################################
set ip_name "s9io"
set ip_library "ip"
set ip_version "0.1"
set ip_vendor "braiins.cz"
set src_path "src/${ip_name}_${ip_version}/hdl"
set ip_repo "${projdir}/ip_repo"

# Set list of VHDL files in compilation order
set s9io_files [ list \
    "${src_path}/crc5_serial.vhd" \
    "${src_path}/crc5_resp_serial.vhd" \
    "${src_path}/crc16_serial.vhd" \
    "${src_path}/fifo_block.vhd" \
    "${src_path}/fifo_block_thr.vhd" \
    "${src_path}/fifo_distr.vhd" \
    "${src_path}/mod_divider.vhd" \
    "${src_path}/uart_rx.vhd" \
    "${src_path}/uart_tx.vhd" \
    "${src_path}/uart.vhd" \
    "${src_path}/s9io_core.vhd" \
    "${src_path}/s9io_v0_1_S00_AXI.vhd" \
    "${src_path}/s9io_v0_1.vhd" \
]

timestamp "Generating IP core ${ip_name} ..."

####################################################################################################
# set VHDL as target language for files generation
set_property target_language VHDL [current_project]

set ip_id "${ip_vendor}:${ip_library}:${ip_name}:${ip_version}"
set ip_repo_path "${ip_repo}/${ip_name}_${ip_version}"

# remove directory if exists
if [file exists $ip_repo_path] {
    file delete -force $ip_repo_path
}

####################################################################################################
# Create new IP peripheral core
####################################################################################################
create_peripheral ${ip_vendor} ${ip_library} ${ip_name} ${ip_version} -dir $ip_repo

set ip_core [ipx::find_open_core $ip_id]

add_peripheral_interface S00_AXI -interface_mode slave -axi_type lite $ip_core
set_property VALUE 16 [ipx::get_bus_parameters WIZ_NUM_REG -of_objects [ipx::get_bus_interfaces S00_AXI -of_objects $ip_core]]
generate_peripheral -driver -bfm_example_design -debug_hw_example_design $ip_core
write_peripheral $ip_core

# set_property ip_repo_paths {} [current_project]

####################################################################################################
# Open IP core for edit
####################################################################################################
set ipx_xml ${ip_repo_path}/component.xml
set ipx_project ${ip_name}_${ip_version}
set project_dir ${ip_name}_${ip_version}

# ipx::open_core ${ip_repo_path}/component.xml
ipx::edit_ip_in_project -upgrade true -name $ipx_project -directory ${projdir}/system.tmp/${ipx_project} $ipx_xml

# set additional information about IP core
set_property description "S9 Board Interface IP core" [ipx::current_core]
set_property company_url "http://www.braiins.cz" [ipx::current_core]

set vhdl_synth_group [ipx::get_file_groups xilinx_vhdlsynthesis -of_objects [ipx::current_core]]
set vhdl_sim_group [ipx::get_file_groups xilinx_vhdlbehavioralsimulation -of_objects [ipx::current_core]]

# add generated file into file list
lappend s9io_hdl_files "hdl/s9io_version.vhd"

# copy source files into IP core directory
foreach FILE $s9io_files {
    file copy -force $FILE ${ip_repo_path}/hdl
    lappend s9io_hdl_files hdl/[file tail $FILE]
}


####################################################################################################
# Generate VHDL file with unix timestamp
####################################################################################################
# name of the VHDL file
set filename "${ip_repo_path}/hdl/s9io_version.vhd"

# open the file for writing
set fd [open $filename "w"]

puts $fd [string repeat "-" 100]
puts $fd "-- Copyright (c) 2018 Braiins Systems s.r.o."
puts $fd "--"
puts $fd "-- Permission is hereby granted, free of charge, to any person obtaining a copy"
puts $fd "-- of this software and associated documentation files (the \"Software\"), to deal"
puts $fd "-- in the Software without restriction, including without limitation the rights"
puts $fd "-- to use, copy, modify, merge, publish, distribute, sublicense, and/or sell"
puts $fd "-- copies of the Software, and to permit persons to whom the Software is"
puts $fd "-- furnished to do so, subject to the following conditions:"
puts $fd "--"
puts $fd "-- The above copyright notice and this permission notice shall be included in all"
puts $fd "-- copies or substantial portions of the Software."
puts $fd "--"
puts $fd "-- THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR"
puts $fd "-- IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,"
puts $fd "-- FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE"
puts $fd "-- AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER"
puts $fd "-- LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,"
puts $fd "-- OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE"
puts $fd "-- SOFTWARE."
puts $fd [string repeat "-" 100]
puts $fd "-- Project Name:   S9 Board Interface IP"
puts $fd "-- Description:    Version of IP core as unix timestamp"
puts $fd "--"
puts $fd "-- Engineer:       Marian Pristach"
puts $fd "-- Revision:       1.0.0 (${date_time})"
puts $fd "-- Comments:       This file is generated during synthesis process - do not modify manually!"
puts $fd [string repeat "-" 100]
puts $fd "library ieee;"
puts $fd "use ieee.std_logic_1164.all;"
puts $fd "use ieee.numeric_std.all;"
puts $fd ""
puts $fd "entity s9io_version is"
puts $fd "    port ("
puts $fd "        timestamp : out std_logic_vector(31 downto 0)"
puts $fd "    );"
puts $fd "end s9io_version;"
puts $fd ""
puts $fd "architecture rtl of s9io_version is"
puts $fd ""
puts $fd "begin"
puts $fd ""
puts $fd "  timestamp <= std_logic_vector(to_unsigned(${build_id}, 32));"
puts $fd ""
puts $fd "end rtl;"

# close the file
close $fd


####################################################################################################
# Add source files into IP core, update and save IP core
####################################################################################################
foreach FILE $s9io_hdl_files {
    ipx::remove_file $FILE $vhdl_synth_group
    ipx::add_file $FILE $vhdl_synth_group
    set_property type vhdlSource [ipx::get_files $FILE -of_objects $vhdl_synth_group]
    set_property library_name xil_defaultlib [ipx::get_files $FILE -of_objects $vhdl_synth_group]
}

# Copy list of synthesis group into simulation group
ipx::copy_contents_from $vhdl_synth_group $vhdl_sim_group

# Update parameters and ports according to RTL sources
ipx::merge_project_changes hdl_parameters [ipx::current_core]
ipx::merge_project_changes ports [ipx::current_core]

ipx::create_xgui_files [ipx::current_core]
ipx::update_checksums [ipx::current_core]
ipx::save_core [ipx::current_core]
close_project

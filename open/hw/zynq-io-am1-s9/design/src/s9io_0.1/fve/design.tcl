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
# Procedure to generate verification design
proc create_ipi_design {design_name} {
    create_bd_design $design_name
    open_bd_design $design_name

    # Create Clock and Reset Ports
    set ACLK [create_bd_port -dir I -type clk ACLK]
    set_property -dict [list CONFIG.FREQ_HZ {50000000} CONFIG.PHASE {0.000} CONFIG.CLK_DOMAIN "${design_name}_ACLK"] $ACLK
    set ARESETN [create_bd_port -dir I -type rst ARESETN]
    set_property -dict [list CONFIG.POLARITY {ACTIVE_LOW}] $ARESETN
    set_property CONFIG.ASSOCIATED_RESET ARESETN $ACLK

    # Create instance: s9io_0, and set properties
    set s9io_0 [create_bd_cell -type ip -vlnv braiins.cz:ip:s9io:0.1 s9io_0]

    # Create instance: master_0, and set properties
    set master_0 [create_bd_cell -type ip -vlnv  xilinx.com:ip:axi_vip master_0]
    set_property -dict [list CONFIG.PROTOCOL {AXI4LITE} CONFIG.INTERFACE_MODE {MASTER}] $master_0

    # Create interface connections
    connect_bd_intf_net [get_bd_intf_pins master_0/M_AXI] [get_bd_intf_pins s9io_0/S00_AXI]

    # Create port connections
    connect_bd_net -net aclk_net [get_bd_ports ACLK] [get_bd_pins master_0/ACLK] [get_bd_pins s9io_0/S00_AXI_ACLK]
    connect_bd_net -net aresetn_net [get_bd_ports ARESETN] [get_bd_pins master_0/ARESETN] [get_bd_pins s9io_0/S00_AXI_ARESETN]

    # Create external ports for rest of DUT pins
    make_bd_pins_external [get_bd_cells s9io_0]

    # Auto assign address
    assign_bd_address
}

####################################################################################################
timestamp "Generating verification environment for IP core s9io ..."

# name of verification design
set design_name "s9io_bfm"

# set name of fileset
set fileset "sim_s9io"

# suppress warning from AXI protocol checker:
#  The "System Verilog Assertion" is not supported yet for simulation. The statement will be ignored.
set_msg_config -id {XSIM 43-4127} -suppress

####################################################################################################
# create new fileset
create_fileset -simset $fileset

# generate verification design
create_ipi_design $design_name
validate_bd_design

set wrapper_file [make_wrapper -files [get_files ${design_name}.bd] -top -force]
import_files -force -norecurse $wrapper_file

set_property target_simulator XSim [current_project]
set_property -name {xsim.simulate.runtime} -value {100ms} -objects [get_filesets $fileset]
set_property SOURCE_SET sources_1 [get_filesets $fileset]

# import verification files
import_files -fileset $fileset -norecurse -force "src/s9io_0.1/fve/s9io_tb.sv"
import_files -fileset $fileset -norecurse -force "src/s9io_0.1/fve/s9io_pkg.sv"
import_files -fileset $fileset -norecurse -force "src/s9io_0.1/fve/bfm_uart.sv"

set_property top s9io_v0_1_tb [get_filesets $fileset]
set_property top_lib {} [get_filesets $fileset]
set_property top_file {} [get_filesets $fileset]

####################################################################################################
timestamp "Launching simulation ..."
launch_simulation -simset $fileset -mode behavioral
close_sim

####################################################################################################
# check result of simulation
set ok 0
set fd [open "${projdir}/system.sim/${fileset}/behav/xsim/simulate.log" "r"]

# find simulation result
while { [gets $fd line] != -1 } {
    if { $line == "Simulation finished: PASSED" } {
        set ok 1
    }
}

# close file
close $fd

# check result of simulation
if { $ok != 1 } {
    puts "ERROR: \[s9io\]: Verification failed, check simulation report"
#     exit 1
}

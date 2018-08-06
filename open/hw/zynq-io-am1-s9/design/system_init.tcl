###########################################################
# CREATE PROJECT
###########################################################
create_project -force $design $projdir -part $partname
set_property target_language Verilog [current_project]

if {[info exists board_part]} {
    set_property board_part $board_part [current_project]
}

###########################################################
# Create Report/Results Directory
###########################################################
set report_dir  $projdir/reports
set results_dir $projdir/results
if ![file exists $report_dir]  {file mkdir $report_dir}
if ![file exists $results_dir] {file mkdir $results_dir}

###########################################################
# Add IP Repositories to search path
###########################################################

set other_repos [get_property ip_repo_paths [current_project]]
set_property  ip_repo_paths  "$ip_repos $other_repos" [current_project]

update_ip_catalog

###########################################################
# CREATE BLOCK DESIGN (GUI/TCL COMBO)
###########################################################

create_bd_design "system"

puts "Source system.tcl ..."
source "./system.tcl"


###########################################################
# BLOCK DESIGN PATCH FOR S9
###########################################################
# enable change of peripheral divisors
set_property -dict [list CONFIG.PCW_OVERRIDE_BASIC_CLOCK {1}] [get_bd_cells processing_system7_0]

### Clock configuration
# DCI_CLK_CTRL (0xF8000128) - not required
# set_property -dict [list CONFIG.PCW_DCI_PERIPHERAL_DIVISOR0 {35} CONFIG.PCW_DCI_PERIPHERAL_DIVISOR1 {3}] [get_bd_cells processing_system7_0]
# SDIO_CLK_CTRL (0xF8000150)
set_property -dict [list CONFIG.PCW_SDIO_PERIPHERAL_DIVISOR0 {40}] [get_bd_cells processing_system7_0]
# UART_CLK_CTRL (0xF8000154) - no change of 0xe0001018
# set_property -dict [list CONFIG.PCW_UART_PERIPHERAL_DIVISOR0 {20}] [get_bd_cells processing_system7_0]

# FPGA0_CLK_CTRL (0xF8000170) - not required
# set_property -dict [list CONFIG.PCW_FCLK0_PERIPHERAL_DIVISOR0 {20} CONFIG.PCW_FCLK0_PERIPHERAL_DIVISOR1 {1}] [get_bd_cells processing_system7_0]

### DDR memory configuration
set_property -dict [list CONFIG.PCW_UIPARAM_DDR_MEMORY_TYPE {DDR 3 (Low Voltage)}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_UIPARAM_DDR_PARTNO {MT41K256M16 RE-15E}] [get_bd_cells processing_system7_0]
# DDRIOB_DDR_CTRL (0xF8000B6C)
set_property -dict [list CONFIG.PCW_UIPARAM_DDR_USE_INTERNAL_VREF {1}] [get_bd_cells processing_system7_0]

# disable USB0
set_property -dict [list CONFIG.PCW_USB0_PERIPHERAL_ENABLE {0}] [get_bd_cells processing_system7_0]

# SD0 pin swap
set_property -dict [list CONFIG.PCW_SD0_GRP_CD_IO {MIO 46}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_SD0_GRP_WP_ENABLE {1} CONFIG.PCW_SD0_GRP_WP_IO {MIO 50}] [get_bd_cells processing_system7_0]

### MIO configuration
set_property -dict [list CONFIG.PCW_MIO_0_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_1_PULLUP {disabled} CONFIG.PCW_MIO_1_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_2_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_3_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_4_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_5_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_6_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_9_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_10_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_11_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_12_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_13_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_14_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_15_PULLUP {disabled}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_16_PULLUP {disabled} CONFIG.PCW_MIO_16_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_17_PULLUP {disabled} CONFIG.PCW_MIO_17_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_18_PULLUP {disabled} CONFIG.PCW_MIO_18_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_19_PULLUP {disabled} CONFIG.PCW_MIO_19_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_20_PULLUP {disabled} CONFIG.PCW_MIO_20_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_21_PULLUP {disabled} CONFIG.PCW_MIO_21_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_22_PULLUP {disabled} CONFIG.PCW_MIO_22_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_23_PULLUP {disabled} CONFIG.PCW_MIO_23_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_24_PULLUP {disabled} CONFIG.PCW_MIO_24_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_25_PULLUP {disabled} CONFIG.PCW_MIO_25_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_26_PULLUP {disabled} CONFIG.PCW_MIO_26_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_27_PULLUP {disabled} CONFIG.PCW_MIO_27_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_28_PULLUP {disabled} CONFIG.PCW_MIO_28_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_29_PULLUP {disabled} CONFIG.PCW_MIO_29_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_30_PULLUP {disabled} CONFIG.PCW_MIO_30_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_31_PULLUP {disabled} CONFIG.PCW_MIO_31_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_32_PULLUP {disabled} CONFIG.PCW_MIO_32_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_33_PULLUP {disabled} CONFIG.PCW_MIO_33_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_34_PULLUP {disabled} CONFIG.PCW_MIO_34_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_35_PULLUP {disabled} CONFIG.PCW_MIO_35_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_36_PULLUP {disabled} CONFIG.PCW_MIO_36_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_37_PULLUP {disabled} CONFIG.PCW_MIO_37_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_38_PULLUP {disabled} CONFIG.PCW_MIO_38_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_39_PULLUP {disabled} CONFIG.PCW_MIO_39_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_40_PULLUP {disabled} CONFIG.PCW_MIO_40_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_41_PULLUP {disabled} CONFIG.PCW_MIO_41_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_42_PULLUP {disabled} CONFIG.PCW_MIO_42_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_43_PULLUP {disabled} CONFIG.PCW_MIO_43_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_44_PULLUP {disabled} CONFIG.PCW_MIO_44_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_45_PULLUP {disabled} CONFIG.PCW_MIO_45_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_46_PULLUP {disabled} CONFIG.PCW_MIO_46_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_47_PULLUP {disabled} CONFIG.PCW_MIO_47_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_48_PULLUP {disabled} CONFIG.PCW_MIO_48_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_49_PULLUP {disabled} CONFIG.PCW_MIO_49_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_50_PULLUP {disabled} CONFIG.PCW_MIO_50_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_51_PULLUP {disabled} CONFIG.PCW_MIO_51_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_52_PULLUP {disabled} CONFIG.PCW_MIO_52_SLEW {slow}] [get_bd_cells processing_system7_0]
set_property -dict [list CONFIG.PCW_MIO_53_PULLUP {disabled} CONFIG.PCW_MIO_53_SLEW {slow}] [get_bd_cells processing_system7_0]

###########################################################
# END OF BLOCK DESIGN PATCH FOR S9
###########################################################


make_wrapper -files [get_files $projdir/${design}.srcs/sources_1/bd/system/system.bd] -top

###########################################################
# ADD FILES
###########################################################

#HDL
if {[string equal [get_filesets -quiet sources_1] ""]} {
    create_fileset -srcset sources_1
}
set top_wrapper $projdir/${design}.srcs/sources_1/bd/system/hdl/system_wrapper.v
add_files -norecurse -fileset [get_filesets sources_1] $top_wrapper

if {[llength $hdl_files] != 0} {
    add_files -norecurse -fileset [get_filesets sources_1] $hdl_files
}

#CONSTRAINTS
if {[string equal [get_filesets -quiet constrs_1] ""]} {
  create_fileset -constrset constrs_1
}
if {[llength $constraints_files] != 0} {
    add_files -norecurse -fileset [get_filesets constrs_1] $constraints_files
}

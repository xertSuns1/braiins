####################################################################################################
# Pin assignment for GPIO 0 - inputs
####################################################################################################
set_property -dict { PACKAGE_PIN Y18 IOSTANDARD LVCMOS33 } [get_ports { gpio_0_tri_i[0]  }];  # S9: J8_5, PLUG0


####################################################################################################
# Pin assignment for GPIO 1 - outputs
####################################################################################################
# LEDs
set_property -dict { PACKAGE_PIN M19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[0]  }];  # S9: D5, LED
set_property -dict { PACKAGE_PIN M17 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[1]  }];  # S9: D6, LED
set_property -dict { PACKAGE_PIN F16 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[2]  }];  # S9: D7, LED
set_property -dict { PACKAGE_PIN L19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[3]  }];  # S9: D8, LED

# connector J8
set_property -dict { PACKAGE_PIN Y19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[4]  }];  # S9: J8_15, RST

####################################################################################################
# Pin assignment for PWM
####################################################################################################
set_property -dict { PACKAGE_PIN J18 IOSTANDARD LVCMOS33 } [get_ports { pwm0  }];  # S9: FAN1_4 ... FAN6_4, PWM


####################################################################################################
# Pin assignment for I2C
####################################################################################################
set_property -dict { PACKAGE_PIN W18 IOSTANDARD LVCMOS33 } [get_ports { iic_0_scl_io }];  # S9: Jx_4, TSCL
set_property -dict { PACKAGE_PIN W19 IOSTANDARD LVCMOS33 } [get_ports { iic_0_sda_io }];  # S9: Jx_3, TSDA


####################################################################################################
# Pin assignment for UARTs
####################################################################################################
# connector J8
set_property -dict { PACKAGE_PIN W16 IOSTANDARD LVCMOS33 } [get_ports { rxd_0 }];  # S9: J8_12, RX
set_property -dict { PACKAGE_PIN V16 IOSTANDARD LVCMOS33 } [get_ports { txd_0 }];  # S9: J8_11, TX



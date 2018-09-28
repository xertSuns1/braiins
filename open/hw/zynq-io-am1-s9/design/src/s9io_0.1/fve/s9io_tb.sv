/***************************************************************************************************
 * Copyright (c) 2018 Braiins Systems s.r.o.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 ***************************************************************************************************
 * Project Name:   S9 Board Interface IP
 * Description:    Testbench for s9io IP core
 *
 * Engineer:       Marian Pristach
 * Revision:       1.0.0 (22.09.2018)
 *
 * Comments:
 **************************************************************************************************/

`timescale 1ns / 1ps

import axi_vip_pkg::*;
import s9io_pkg::*;
import s9io_bfm_master_0_0_pkg::*;

module s9io_v0_1_tb();

    // Simulation parameters
    parameter VERBOSE_LEVEL = 0;

    // instance of AXI master BFM agent
    s9io_bfm_master_0_0_mst_t mst_agent;

    // counter of errors
    integer err_counter = 0;

    // local signals
    bit clock;
    bit reset;

    logic uart_rx;
    logic uart_tx;
    logic irq_work_tx;
    logic irq_work_rx;
    logic irq_cmd_rx;

    // ---------------------------------------------------------------------------------------------
    // instance of DUT
    s9io_bfm_wrapper DUT (
        .ACLK(clock),
        .ARESETN(reset),
        .rxd_0(uart_rx),
        .txd_0(uart_tx),
        .irq_work_tx_0(irq_work_tx),
        .irq_work_rx_0(irq_work_rx),
        .irq_cmd_rx_0(irq_cmd_rx)
    );

    // instance of UART BFM
    uart_bfm i_uart (
        .rx(uart_tx),
        .tx(uart_rx)
    );

    // ---------------------------------------------------------------------------------------------
    initial begin
        automatic xil_axi_uint mst_agent_verbosity = 0;

        mst_agent = new("master vip agent",DUT.s9io_bfm_i.master_0.inst.IF);
        mst_agent.vif_proxy.set_dummy_drive_type(XIL_AXI_VIF_DRIVE_NONE);
        mst_agent.set_agent_tag("Master VIP");
        mst_agent.set_verbosity(mst_agent_verbosity);
        mst_agent.start_master();
        $timeformat (-9, 0, "ns", 1);
    end

    // ---------------------------------------------------------------------------------------------
    // clock and reset generation
    always #10 clock <= ~clock;

    initial begin
        reset <= 1'b0;
        #(3*CLK_PERIOD);
        reset <= 1'b1;
    end

    // ---------------------------------------------------------------------------------------------
    initial begin
        automatic int rdata = 0;
        automatic int tmp = 0;

        // wait for reset is finished
        @(posedge reset);

        $display("############################################################");
        $display("Starting simulation");
        $display("############################################################");

        axi_read(BUILD_ID, rdata);
        $display("Build ID: %d", rdata);

        // configure IP core
        $display("Configuring IP core: work time 1us, UART baudrate 3.125M");
        axi_write(BAUD_REG, 0);           // @50MHz -> 3.125 MBd
        axi_write(WORK_TIME, 50);         // @50MHz -> 1us
        axi_write(CTRL_REG, CTRL_ENABLE); // enable IP core, 1 midstate

        // checking status
        axi_read(STAT_REG, rdata);
        tmp = STAT_WORK_TX_EMPTY | STAT_WORK_RX_EMPTY | STAT_CMD_TX_EMPTY | STAT_CMD_RX_EMPTY |
            STAT_IRQ_PEND_WORK_TX;
        compare_data(tmp, rdata, "STAT_REG");

        $display("############################################################");

        // -----------------------------------------------------------------------------------------
        // testcase 1 - test of send commands (5 and 9 bytes)
        tc_send_cmd5();
        tc_send_cmd9();

        // testcase 2 - test of work send (1 and 4 midstates)
        tc_send_work_midstate1();
        tc_send_work_midstate4();

        // testcase 3 - test of receive of command and work response
        tc_cmd_response();
        tc_work_response();

        // testcase 4 - test of FIFOs (reset and flags)
        tc_fifo_cmd_rx();
        tc_fifo_cmd_tx();
        tc_fifo_work_rx();
        tc_fifo_work_tx();

        // testcase 5 - test of IRQs
        tc_irq_cmd_rx();
        tc_irq_work_rx();
        tc_irq_work_tx();

        // testcase 6 - test of last job ID
        tc_job_id_1();
        tc_job_id_2();
        tc_job_id_3();
        tc_job_id_4();

        // Testcase 7 - test of IP core reset by enable flag
        tc_ip_core_reset_1();
        tc_ip_core_reset_2();
        tc_ip_core_reset_3();
        tc_ip_core_reset_4();

        // testcase 8 - test of error counter register
        tc_error_counter();

        // -----------------------------------------------------------------------------------------
        // final report
        $display("############################################################");
        if (err_counter == 0) begin
            $display("Simulation finished: PASSED");
        end else begin
            $display("Simulation finished: FAILED");
            $display("Number of errors: %0d", err_counter);
        end
        $display("############################################################");

        #1;
        $finish;
    end

    // simulation timeout check
    initial begin
        #5ms;
        $display("############################################################");
        $display("Simulation timeout");
        $display("############################################################");
        $finish;
    end


    // *********************************************************************************************
    //                                     TESTCASES
    // *********************************************************************************************

    // ---------------------------------------------------------------------------------------------
    // Testcase 1: Test of send commands
    // ---------------------------------------------------------------------------------------------
    // send 5 bytes commands
    task tc_send_cmd5();
        // Tx FIFO data:
        // - check_asic_reg(CHIP_ADDRESS) -> read_asic_register(chain, 1, 0, CHIP_ADDRESS)
        // - software_set_address
        // - software_set_address -> 64x set_address(i, 0, chip_addr) // chip_addr = 0, 4, 8, 12
        static logic[31:0] fifo_data1[$] = {32'h00000554};
        static logic[31:0] fifo_data2[$] = {32'h00000555};
        static logic[31:0] fifo_data3[$] = {32'h00000541};
        static logic[31:0] fifo_data4[$] = {32'h00040541};
        static logic[31:0] fifo_data5[$] = {32'h00080541};
        static logic[31:0] fifo_data6[$] = {32'h000c0541};
        static logic[31:0] fifo_data7[$] = {32'h00f40541};
        static logic[31:0] fifo_data8[$] = {32'h00f80541};
        static logic[31:0] fifo_data9[$] = {32'h00fc0541};

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {8'h54, 8'h05, 8'h00, 8'h00, 8'h19};
        static logic[7:0] uart_data2[$] = {8'h55, 8'h05, 8'h00, 8'h00, 8'h10};
        static logic[7:0] uart_data3[$] = {8'h41, 8'h05, 8'h00, 8'h00, 8'h15};
        static logic[7:0] uart_data4[$] = {8'h41, 8'h05, 8'h04, 8'h00, 8'h0a};
        static logic[7:0] uart_data5[$] = {8'h41, 8'h05, 8'h08, 8'h00, 8'h0e};
        static logic[7:0] uart_data6[$] = {8'h41, 8'h05, 8'h0c, 8'h00, 8'h11};
        static logic[7:0] uart_data7[$] = {8'h41, 8'h05, 8'hf4, 8'h00, 8'h10};
        static logic[7:0] uart_data8[$] = {8'h41, 8'h05, 8'hf8, 8'h00, 8'h14};
        static logic[7:0] uart_data9[$] = {8'h41, 8'h05, 8'hfc, 8'h00, 8'h0b};

        $display("Testcase 1a: send 5 bytes commands");
        fifo_write_cmd(fifo_data1);
        uart_read_and_compare(uart_data1);

        fifo_write_cmd(fifo_data2);
        uart_read_and_compare(uart_data2);

        fifo_write_cmd(fifo_data3);
        uart_read_and_compare(uart_data3);

        fifo_write_cmd(fifo_data4);
        uart_read_and_compare(uart_data4);

        fifo_write_cmd(fifo_data5);
        uart_read_and_compare(uart_data5);

        fifo_write_cmd(fifo_data6);
        uart_read_and_compare(uart_data6);

        fifo_write_cmd(fifo_data7);
        uart_read_and_compare(uart_data7);

        fifo_write_cmd(fifo_data8);
        uart_read_and_compare(uart_data8);

        fifo_write_cmd(fifo_data9);
        uart_read_and_compare(uart_data9);
    endtask


    // ---------------------------------------------------------------------------------------------
    // send 9 bytes commands
    task tc_send_cmd9();
        // Tx FIFO data:
        // - set_frequency(dev->frequency) -> set_frequency_with_addr_plldatai(pllindex, mode, addr, chain)
        // - open_core_one_chain(chainIndex, nullwork_enable) ->
        // -   BC_COMMAND_BUFFER_READY | BC_COMMAND_EN_CHAIN_ID | (chainIndex << 16) | (bc_command & 0xfff0ffff)
        // - set_asic_ticket_mask(63)
        // - set_hcnt(0)
        static logic[31:0] fifo_data1[$]  = {32'h0c000948, 32'h21026800};
        static logic[31:0] fifo_data2[$]  = {32'h0c040948, 32'h21026800};
        static logic[31:0] fifo_data3[$]  = {32'h0c080948, 32'h21026800};
        static logic[31:0] fifo_data4[$]  = {32'h0c0c0948, 32'h21026800};
        static logic[31:0] fifo_data5[$]  = {32'h0ce80948, 32'h21026800};
        static logic[31:0] fifo_data6[$]  = {32'h0cec0948, 32'h21026800};
        static logic[31:0] fifo_data7[$]  = {32'h0cf00948, 32'h21026800};
        static logic[31:0] fifo_data8[$]  = {32'h0cf40948, 32'h21026800};
        static logic[31:0] fifo_data9[$]  = {32'h1c000958, 32'h809a2040};
        static logic[31:0] fifo_data10[$] = {32'h18000958, 32'h3f000000};
        static logic[31:0] fifo_data11[$] = {32'h14000958, 32'h00000000};

        // reference data send out through UART
        static logic[7:0] uart_data1[$]  = {8'h48, 8'h09, 8'h00, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h02};
        static logic[7:0] uart_data2[$]  = {8'h48, 8'h09, 8'h04, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h19};
        static logic[7:0] uart_data3[$]  = {8'h48, 8'h09, 8'h08, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h11};
        static logic[7:0] uart_data4[$]  = {8'h48, 8'h09, 8'h0c, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h0a};
        static logic[7:0] uart_data5[$]  = {8'h48, 8'h09, 8'he8, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h03};
        static logic[7:0] uart_data6[$]  = {8'h48, 8'h09, 8'hec, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h18};
        static logic[7:0] uart_data7[$]  = {8'h48, 8'h09, 8'hf0, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h13};
        static logic[7:0] uart_data8[$]  = {8'h48, 8'h09, 8'hf4, 8'h0c, 8'h00, 8'h68, 8'h02, 8'h21, 8'h08};
        static logic[7:0] uart_data9[$]  = {8'h58, 8'h09, 8'h00, 8'h1c, 8'h40, 8'h20, 8'h9a, 8'h80, 8'h00};
        static logic[7:0] uart_data10[$] = {8'h58, 8'h09, 8'h00, 8'h18, 8'h00, 8'h00, 8'h00, 8'h3f, 8'h00};
        static logic[7:0] uart_data11[$] = {8'h58, 8'h09, 8'h00, 8'h14, 8'h00, 8'h00, 8'h00, 8'h00, 8'h0a};

        $display("Testcase 1b: send 9 bytes commands");
        fifo_write_cmd(fifo_data1);
        uart_read_and_compare(uart_data1);

        fifo_write_cmd(fifo_data2);
        uart_read_and_compare(uart_data2);

        fifo_write_cmd(fifo_data3);
        uart_read_and_compare(uart_data3);

        fifo_write_cmd(fifo_data4);
        uart_read_and_compare(uart_data4);

        fifo_write_cmd(fifo_data5);
        uart_read_and_compare(uart_data5);

        fifo_write_cmd(fifo_data6);
        uart_read_and_compare(uart_data6);

        fifo_write_cmd(fifo_data7);
        uart_read_and_compare(uart_data7);

        fifo_write_cmd(fifo_data8);
        uart_read_and_compare(uart_data8);

        fifo_write_cmd(fifo_data9);
        uart_read_and_compare(uart_data9);

        fifo_write_cmd(fifo_data10);
        uart_read_and_compare(uart_data10);

        fifo_write_cmd(fifo_data11);
        uart_read_and_compare(uart_data11);
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 2: Test of send works
    // ---------------------------------------------------------------------------------------------
    // send 1 midstate work
    task tc_send_work_midstate1();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00000000, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h00, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h5f, 8'hd3
        };

        $display("Testcase 2a: send 1 midstate work");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);
    endtask


    // ---------------------------------------------------------------------------------------------
    // send 4 midstates work
    task tc_send_work_midstate4();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00000031, 32'h17365a17, 32'h5b51c8e6, 32'h66014b9d, 32'h1df9f7a3, 32'hba9aca03,
            32'hc42b0a8c, 32'hd89fc91a, 32'h1046e72e, 32'h46a47e9a, 32'hf01c1b8e, 32'hebc3c539,
            32'he578935d, 32'hc6419d97, 32'h1ff8d327, 32'h7bf6698e, 32'hd757b9eb, 32'h980317d2,
            32'heafd359f, 32'h9544a768, 32'h0e1d09af, 32'hc9316c84, 32'h89bbde77, 32'hcb13866a,
            32'h805beaaa, 32'hffbbfdb1, 32'ha1b617a9, 32'ha81b497c, 32'h93c5272d, 32'hcd1b2770,
            32'h96ab3905, 32'h7bfafae3, 32'hf1004cdb, 32'hb08d4078, 32'hd82c00af, 32'he75b218b
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h96, 8'h31, 8'h04, 8'h00, 8'h00, 8'h00, 8'h00, 8'h17, 8'h5a, 8'h36, 8'h17,
            8'he6, 8'hc8, 8'h51, 8'h5b, 8'h9d, 8'h4b, 8'h01, 8'h66, 8'h1d, 8'hf9, 8'hf7, 8'ha3,
            8'hba, 8'h9a, 8'hca, 8'h03, 8'hc4, 8'h2b, 8'h0a, 8'h8c, 8'hd8, 8'h9f, 8'hc9, 8'h1a,
            8'h10, 8'h46, 8'he7, 8'h2e, 8'h46, 8'ha4, 8'h7e, 8'h9a, 8'hf0, 8'h1c, 8'h1b, 8'h8e,
            8'heb, 8'hc3, 8'hc5, 8'h39, 8'he5, 8'h78, 8'h93, 8'h5d, 8'hc6, 8'h41, 8'h9d, 8'h97,
            8'h1f, 8'hf8, 8'hd3, 8'h27, 8'h7b, 8'hf6, 8'h69, 8'h8e, 8'hd7, 8'h57, 8'hb9, 8'heb,
            8'h98, 8'h03, 8'h17, 8'hd2, 8'hea, 8'hfd, 8'h35, 8'h9f, 8'h95, 8'h44, 8'ha7, 8'h68,
            8'h0e, 8'h1d, 8'h09, 8'haf, 8'hc9, 8'h31, 8'h6c, 8'h84, 8'h89, 8'hbb, 8'hde, 8'h77,
            8'hcb, 8'h13, 8'h86, 8'h6a, 8'h80, 8'h5b, 8'hea, 8'haa, 8'hff, 8'hbb, 8'hfd, 8'hb1,
            8'ha1, 8'hb6, 8'h17, 8'ha9, 8'ha8, 8'h1b, 8'h49, 8'h7c, 8'h93, 8'hc5, 8'h27, 8'h2d,
            8'hcd, 8'h1b, 8'h27, 8'h70, 8'h96, 8'hab, 8'h39, 8'h05, 8'h7b, 8'hfa, 8'hfa, 8'he3,
            8'hf1, 8'h00, 8'h4c, 8'hdb, 8'hb0, 8'h8d, 8'h40, 8'h78, 8'hd8, 8'h2c, 8'h00, 8'haf,
            8'he7, 8'h5b, 8'h21, 8'h8b, 8'h37, 8'h0a
        };

        $display("Testcase 2b: send 4 midstates work");

        // set 4 midstates mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_4);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 3: Test of receive and check work and commands responses
    // ---------------------------------------------------------------------------------------------
    // receive of command response
    task tc_cmd_response();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h13, 8'h87, 8'h90, 8'h00, 8'h00, 8'h00, 8'h07};
        static logic[7:0] uart_data2[$] = {8'h13, 8'h87, 8'h90, 8'h04, 8'h00, 8'h00, 8'h10};
        static logic[7:0] uart_data3[$] = {8'h13, 8'h87, 8'h90, 8'h08, 8'h00, 8'h00, 8'h0c};
        static logic[7:0] uart_data4[$] = {8'h13, 8'h87, 8'h90, 8'h0c, 8'h00, 8'h00, 8'h1b};
        static logic[7:0] uart_data5[$] = {8'h13, 8'h87, 8'h90, 8'he8, 8'h00, 8'h00, 8'h16};
        static logic[7:0] uart_data6[$] = {8'h13, 8'h87, 8'h90, 8'hec, 8'h00, 8'h00, 8'h01};
        static logic[7:0] uart_data7[$] = {8'h13, 8'h87, 8'h90, 8'hf0, 8'h00, 8'h00, 8'h0b};
        static logic[7:0] uart_data8[$] = {8'h13, 8'h87, 8'h90, 8'hf4, 8'h00, 8'h00, 8'h1c};

        // reference Rx FIFO data
        static logic[31:0] fifo_data1[$] = {32'h00908713, 32'h07000000};
        static logic[31:0] fifo_data2[$] = {32'h04908713, 32'h10000000};
        static logic[31:0] fifo_data3[$] = {32'h08908713, 32'h0c000000};
        static logic[31:0] fifo_data4[$] = {32'h0c908713, 32'h1b000000};
        static logic[31:0] fifo_data5[$] = {32'he8908713, 32'h16000000};
        static logic[31:0] fifo_data6[$] = {32'hec908713, 32'h01000000};
        static logic[31:0] fifo_data7[$] = {32'hf0908713, 32'h0b000000};
        static logic[31:0] fifo_data8[$] = {32'hf4908713, 32'h1c000000};

        $display("Testcase 3a: command response");
        uart_send_data(uart_data1);
        fifo_read_and_compare_cmd(fifo_data1);

        uart_send_data(uart_data2);
        fifo_read_and_compare_cmd(fifo_data2);

        uart_send_data(uart_data3);
        fifo_read_and_compare_cmd(fifo_data3);

        uart_send_data(uart_data4);
        fifo_read_and_compare_cmd(fifo_data4);

        uart_send_data(uart_data5);
        fifo_read_and_compare_cmd(fifo_data5);

        uart_send_data(uart_data6);
        fifo_read_and_compare_cmd(fifo_data6);

        uart_send_data(uart_data7);
        fifo_read_and_compare_cmd(fifo_data7);

        uart_send_data(uart_data8);
        fifo_read_and_compare_cmd(fifo_data8);
    endtask


    // ---------------------------------------------------------------------------------------------
    // receive of work response
    task tc_work_response();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h03, 8'h98};
        static logic[7:0] uart_data2[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h04, 8'h9e};
        static logic[7:0] uart_data3[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h05, 8'h93};
        static logic[7:0] uart_data4[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h06, 8'h84};
        static logic[7:0] uart_data5[$] = {8'he1, 8'h6b, 8'hf8, 8'h09, 8'h01, 8'h6f, 8'h9c};
        static logic[7:0] uart_data6[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h70, 8'h80};
        static logic[7:0] uart_data7[$] = {8'he1, 8'h6b, 8'hf8, 8'h09, 8'h01, 8'h70, 8'h93};
        static logic[7:0] uart_data8[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h71, 8'h8d};
        static logic[7:0] uart_data9[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h12, 8'h99};

        // reference Rx FIFO data - the real value depends on the last work ID
        static logic[31:0] fifo_data1[$] = {32'h83ea0372, 32'h98000300};
        static logic[31:0] fifo_data2[$] = {32'h83ea0372, 32'h9e000400};
        static logic[31:0] fifo_data3[$] = {32'h83ea0372, 32'h93000500};
        static logic[31:0] fifo_data4[$] = {32'h83ea0372, 32'h84000600};
        static logic[31:0] fifo_data5[$] = {32'h09f86be1, 32'h9c006f01};
        static logic[31:0] fifo_data6[$] = {32'h83ea0372, 32'h80007000};
        static logic[31:0] fifo_data7[$] = {32'h09f86be1, 32'h93007001};
        static logic[31:0] fifo_data8[$] = {32'h83ea0372, 32'h8d007100};
        static logic[31:0] fifo_data9[$] = {32'h083c0648, 32'h99001200};

        $display("Testcase 3b: work response");

        // initialization of job ID to max. value
        init_job_id();

        uart_send_data(uart_data1);
        fifo_read_and_compare_work(fifo_data1);

        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);

        uart_send_data(uart_data3);
        fifo_read_and_compare_work(fifo_data3);

        uart_send_data(uart_data4);
        fifo_read_and_compare_work(fifo_data4);

        uart_send_data(uart_data5);
        fifo_read_and_compare_work(fifo_data5);

        uart_send_data(uart_data6);
        fifo_read_and_compare_work(fifo_data6);

        uart_send_data(uart_data7);
        fifo_read_and_compare_work(fifo_data7);

        uart_send_data(uart_data8);
        fifo_read_and_compare_work(fifo_data8);
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 4: Test of FIFO reset/flags
    // ---------------------------------------------------------------------------------------------
    // test of FIFOs reset and flags, command RX FIFO
    task tc_fifo_cmd_rx();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h13, 8'h87, 8'h90, 8'hf4, 8'h00, 8'h00, 8'h1c};

        $display("Testcase 4a: FIFO reset/flags, command RX FIFO");

        // check if FIFO is empty
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        uart_send_data(uart_data1);

        // check if FIFO is not empty
        check_status(STAT_CMD_RX_EMPTY, 1'b0, "FIFO is empty after write");

        // reset of FIFO
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_RST_CMD_RX_FIFO);

        // wait for work time
        #1us;

        // check if FIFO is empty
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "FIFO reset");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of FIFOs reset and flags, command TX FIFO
    task tc_fifo_cmd_tx();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {32'h00000554};
        static logic[31:0] fifo_data2[$] = {32'h00000555};

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {8'h54, 8'h05, 8'h00, 8'h00, 8'h19};

        $display("Testcase 4b: FIFO reset/flags, command TX FIFO");

        // check if FIFO is empty
        check_status(STAT_CMD_TX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        fifo_write_cmd(fifo_data1);
        fifo_write_cmd(fifo_data2);

        // wait for work time
        #1us;

        // check if FIFO is not empty
        check_status(STAT_CMD_TX_EMPTY, 1'b0, "FIFO is empty after write");

        // reset of FIFO
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_RST_CMD_TX_FIFO);

        // wait for work time
        #1us;

        // check if FIFO is empty
        check_status(STAT_CMD_TX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "FIFO reset");

        // first command is sent but the second is deleted, so we can read the first
        uart_read_and_compare(uart_data1);
    endtask


    // ---------------------------------------------------------------------------------------------
    // test of FIFOs reset and flags, work response RX FIFO
    task tc_fifo_work_rx();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h12, 8'h99};

        $display("Testcase 4c: FIFO reset/flags, work RX FIFO");

        // check if FIFO is empty
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        uart_send_data(uart_data1);

        // check if FIFO is not empty
        check_status(STAT_WORK_RX_EMPTY, 1'b0, "FIFO is empty after write");

        // reset of FIFO
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_RST_WORK_RX_FIFO);

        // wait for work time
        #1us;

        // check if FIFO is empty
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "FIFO reset");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of FIFOs reset and flags, work TX FIFO
    task tc_fifo_work_tx();
        // data are not complete - missing 5 words
        static logic[31:0] fifo_data1[$] = {
            32'h00000000, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000
        };

        $display("Testcase 4d: FIFO reset/flags, work TX FIFO");

        // check if FIFO is empty
        check_status(STAT_WORK_TX_EMPTY, 1'b1, "FIFO is not empty");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        // send data
        fifo_write_work(fifo_data1);

        // check if FIFO is not empty
        check_status(STAT_WORK_TX_EMPTY, 1'b0, "FIFO is empty after write");

        // reset of FIFO
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_RST_WORK_TX_FIFO);

        // wait for work time
        #1us;

        // check if FIFO is empty
        check_status(STAT_WORK_TX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "FIFO reset");
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 5: Test of IRQs and status flags
    // ---------------------------------------------------------------------------------------------
    // test of IRQs - command RX
    task tc_irq_cmd_rx();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h13, 8'h87, 8'h90, 8'hf4, 8'h00, 8'h00, 8'h1c};
        // reference Rx FIFO data
        static logic[31:0] fifo_data1[$] = {32'hf4908713, 32'h1c000000};

        $display("Testcase 5a: IRQ RX command response");

        // check IRQ ports and flags
        check_irq(3'b000, 3'b001, "initial state");

        // send data through UART
        uart_send_data(uart_data1);

        // check IRQ ports and flags - IRQ should be zero because IRQ is disabled
        check_irq(3'b000, 3'b101, "IRQ is disabled");

        // read data
        fifo_read_and_compare_cmd(fifo_data1);

        // enable IRQ
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_IRQ_EN_CMD_RX);

         // check IRQ ports and flags
        check_irq(3'b000, 3'b001, "no data has been send yet");

        // send data through UART
        uart_send_data(uart_data1);

        // check IRQ ports and flags - should be one because IRQ is enabled
        check_irq(3'b100, 3'b101, "data has been send");

        // read data
        fifo_read_and_compare_cmd(fifo_data1);

        // check IRQ ports and flags
        check_irq(3'b000, 3'b001, "all data already read");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of IRQs - work RX
    task tc_irq_work_rx();
        // data send through UART
        static logic[7:0] uart_data1[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h12, 8'h99};
        // reference Rx FIFO data - the real value depends on the last work ID !!!
        static logic[31:0] fifo_data1[$] = {32'h083c0648, 32'h99001200};

        $display("Testcase 5b: IRQ RX work response");

        // initialization of job ID to max. value
        init_job_id();

        // check IRQ ports
        check_irq(3'b000, 3'b001, "initial state");

        // send data through UART
        uart_send_data(uart_data1);

        // check IRQ ports and flags - IRQ should be zero because IRQ is disabled
        check_irq(3'b000, 3'b011, "IRQ is disabled");

        // read data
        fifo_read_and_compare_work(fifo_data1);

        // enable IRQ
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_IRQ_EN_WORK_RX);

         // check IRQ ports
        check_irq(3'b000, 3'b001, "no data has been send yet");

        // send data through UART
        uart_send_data(uart_data1);

        // check IRQ ports - should be one because IRQ is enabled
        check_irq(3'b010, 3'b011, "data has been send");

        // read data
        fifo_read_and_compare_work(fifo_data1);

         // check IRQ ports
        check_irq(3'b000, 3'b001, "all data already read");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of IRQs - work TX
    task tc_irq_work_tx();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00000000, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h00, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h5f, 8'hd3
        };

        // temporary unpacked array
        static logic[31:0] tmp[$];

        $display("Testcase 5c: IRQ TX work");

        // check IRQ ports
        check_irq(3'b000, 3'b001, "initial state");

        // set 1 midstate mode, IRQ enabled
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1 | CTRL_IRQ_EN_WORK_TX);
        // set IRQ threshold in words
        axi_write(IRQ_FIFO_THR, 6);

        // check IRQ ports
        check_irq(3'b001, 3'b001, "IRQ enabled");

        // send first part of data
        fifo_write_work(fifo_data1[0:4]);

        // check IRQ ports
        check_irq(3'b001, 3'b001, "not enough data in FIFO");

        // send one word to switch IRQ threshold
        tmp.push_back(fifo_data1[5]);
        fifo_write_work(tmp);

        // check IRQ ports
        check_irq(3'b000, 3'b000, "enough data in FIFO");

        // send rest of data
        fifo_write_work(fifo_data1[6:fifo_data1.size-1]);

        // check IRQ ports
        check_irq(3'b000, 3'b000, "all data are in FIFO");

        // wait until send is completed + check data
        uart_read_and_compare(uart_data1);

        // check IRQ ports
        check_irq(3'b001, 3'b001, "FIFO is again empty");
    endtask

    // ---------------------------------------------------------------------------------------------
    // Testcase 6: Test of last job ID
    // ---------------------------------------------------------------------------------------------
    // response has the same job ID as last work
    task tc_job_id_1();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00000000, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h00, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h5f, 8'hd3
        };

        // response send through UART
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h00, 8'h8d};

        // reference Rx FIFO data
        static logic[31:0] fifo_data2[$] = {32'h083c0648, 32'h8d000000};

        $display("Testcase 6a: job ID, response with same ID");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);

        // send work response
        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);
    endtask

    // ---------------------------------------------------------------------------------------------
    // response has the same job ID as last work, full range
    task tc_job_id_2();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h0000cdef, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h6f, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'ha7
        };

        // response send through UART
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h6f, 8'h8d};

        // reference Rx FIFO data
        static logic[31:0] fifo_data2[$] = {32'h083c0648, 32'h8dcdef00};

        $display("Testcase 6b: job ID, response with same ID, full range");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);

        // send work response
        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);
    endtask

    // ---------------------------------------------------------------------------------------------
    // response has smaller job ID then last work
    task tc_job_id_3();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00001234, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h34, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h4f, 8'hc8
        };

        // response send through UART
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h30, 8'h9f};

        // reference Rx FIFO data
        static logic[31:0] fifo_data2[$] = {32'h083c0648, 32'h9f123000};

        $display("Testcase 6c: job ID, response with smaller ID then last work");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);

        // send work response
        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);
    endtask

    // ---------------------------------------------------------------------------------------------
    // response has higher job ID then last work (should not happen in real HW)
    task tc_job_id_4();
        // Tx FIFO data
        static logic[31:0] fifo_data1[$] = {
            32'h00001234, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h34, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h4f, 8'hc8
        };

        // response send through UART
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h40, 8'h90};

        // reference Rx FIFO data
        static logic[31:0] fifo_data2[$] = {32'h083c0648, 32'h9011c000};

        $display("Testcase 6c: job ID, response with higher ID then last work");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);

        // send work response
        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 7: Test of reset of IP core by enable flag
    // ---------------------------------------------------------------------------------------------
    // test of IP core reset, command RX FIFO
    task tc_ip_core_reset_1();
        // data send through UART - not complete!
        static logic[7:0] uart_data1[$] = {8'h13, 8'h87, 8'h90, 8'h00, 8'h00, 8'h00};
        // data send through UART - complete
        static logic[7:0] uart_data2[$] = {8'h13, 8'h87, 8'h90, 8'hf4, 8'h00, 8'h00, 8'h1c};

        // reference Rx FIFO data
        static logic[31:0] fifo_data2[$] = {32'hf4908713, 32'h1c000000};

        automatic int rdata = 0;

        $display("Testcase 7a: IP core reset, command RX FIFO");

        // check if FIFO is empty
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        uart_send_data(uart_data1);

        // check if FIFO is still empty
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "FIFO is not empty after partial write");

        // reset of IP core
        axi_write(CTRL_REG, 32'h0);
        axi_write(CTRL_REG, CTRL_ENABLE);

        // check if FIFO is empty
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "FIFO is not empty after IP core reset");

        // check IRQ flags
        check_irq_flags(3'b001, "IP core reset");

        // send and check new data
        uart_send_data(uart_data2);
        fifo_read_and_compare_cmd(fifo_data2);

        // check error counter - should be zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of IP core reset, command TX FIFO
    task tc_ip_core_reset_2();
        // Tx FIFO data - not complete!
        static logic[31:0] fifo_data1[$] = {32'h0c000948};
        // complete
        static logic[31:0] fifo_data2[$] = {32'h14000958, 32'h00000000};

        // reference data send out through UART
        static logic[7:0] uart_data2[$] = {8'h58, 8'h09, 8'h00, 8'h14, 8'h00, 8'h00, 8'h00, 8'h00, 8'h0a};

        automatic int rdata = 0;

        $display("Testcase 7b: IP core reset, command TX FIFO");

        // check if FIFO is empty
        check_status(STAT_CMD_TX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        fifo_write_cmd(fifo_data1);

        // check if FIFO is still empty
        check_status(STAT_CMD_TX_EMPTY, 1'b0, "FIFO is empty after partial write");

        // reset of IP core
        axi_write(CTRL_REG, 32'h0);
        axi_write(CTRL_REG, CTRL_ENABLE);

        // check if FIFO is empty
        check_status(STAT_CMD_TX_EMPTY, 1'b1, "FIFO is not empty after IP core reset");

        // check IRQ flags
        check_irq_flags(3'b001, "IP core reset");

        // send and check new data
        fifo_write_cmd(fifo_data2);
        uart_read_and_compare(uart_data2);

        // check error counter - should be zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of IP core reset, work response RX FIFO
    task tc_ip_core_reset_3();
        // data send through UART - not complete
        static logic[7:0] uart_data1[$] = {8'h72, 8'h03, 8'hea, 8'h83, 8'h00, 8'h03};
        // complete
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h12, 8'h99};

        // reference Rx FIFO data - the real value depends on the last work ID
        static logic[31:0] fifo_data2[$] = {32'h083c0648, 32'h99001200};

        automatic int rdata = 0;

        $display("Testcase 7c: IP core reset, work RX FIFO");

        // initialization of job ID to max. value
        init_job_id();

        // check if FIFO is empty
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "FIFO is not empty");

        // send data
        uart_send_data(uart_data1);

        // check if FIFO is still empty
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "FIFO is empty after partial write");

        // reset of IP core
        axi_write(CTRL_REG, 32'h0);
        axi_write(CTRL_REG, CTRL_ENABLE);

        // check if FIFO is empty
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "IP core reset");

        // send and check new data
        uart_send_data(uart_data2);
        fifo_read_and_compare_work(fifo_data2);

        // check error counter - should be zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");
    endtask

    // ---------------------------------------------------------------------------------------------
    // test of IP core reset, work TX FIFO
    task tc_ip_core_reset_4();
        // data are not complete - missing 5 words
        static logic[31:0] fifo_data1[$] = {
            32'h00001234, 32'h11111111, 32'h22222222, 32'h33333333, 32'h44444444, 32'h55555555
        };

        // Tx FIFO data - complete
        static logic[31:0] fifo_data2[$] = {
            32'h00000000, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data2[$] = {
            8'h21, 8'h36, 8'h00, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h5f, 8'hd3
        };

        automatic int rdata = 0;

        $display("Testcase 7d: IP core reset, work TX FIFO");

        // check if FIFO is empty
        check_status(STAT_WORK_TX_EMPTY, 1'b1, "FIFO is not empty");

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        // send data
        fifo_write_work(fifo_data1);

        // check if FIFO is not empty
        check_status(STAT_WORK_TX_EMPTY, 1'b0, "FIFO is empty after write");

        // reset of IP core
        axi_write(CTRL_REG, 32'h0);
        axi_write(CTRL_REG, CTRL_ENABLE);

        // check if FIFO is empty
        check_status(STAT_WORK_TX_EMPTY, 1'b1, "FIFO is not empty after reset");

        // check IRQ flags
        check_irq_flags(3'b001, "IP core reset");

        // send and check new data
        fifo_write_work(fifo_data2);
        uart_read_and_compare(uart_data2);

        // check error counter - should be zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");
    endtask


    // ---------------------------------------------------------------------------------------------
    // Testcase 8: Test error counter register
    // ---------------------------------------------------------------------------------------------
    task tc_error_counter();
        // data send through UART - command response, wrong CRC (should be 8'h1c)
        static logic[7:0] uart_data1[$] = {8'h13, 8'h87, 8'h90, 8'hf4, 8'h00, 8'h00, 8'h1d};

        // data send through UART - work response, wrong CRC (should be 8'h99)
        static logic[7:0] uart_data2[$] = {8'h48, 8'h06, 8'h3c, 8'h08, 8'h00, 8'h12, 8'h98};

        automatic int rdata = 0;

        $display("Testcase 8a: error counter register");

        // check if register is zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");

        // clear error counter register
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_ERR_CNT_CLEAR);

        // send corrupted command response
        uart_send_data(uart_data1);

        // check if counter is incremented and FIFO is empty
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h1, rdata, "ERR_COUNTER");
        check_status(STAT_CMD_RX_EMPTY, 1'b1, "RX command FIFO is not empty");

        // send corrupted work response
        uart_send_data(uart_data2);

        // check if counter is incremented and FIFO is empty
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h2, rdata, "ERR_COUNTER");
        check_status(STAT_WORK_RX_EMPTY, 1'b1, "RX work FIFO is not empty");

        // clear error counter register
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_ERR_CNT_CLEAR);

        // check if register is zero
        axi_read(ERR_COUNTER, rdata);
        compare_data(32'h0, rdata, "ERR_COUNTER");
    endtask


    // ---------------------------------------------------------------------------------------------
    //                               Auxiliary Functions
    // ---------------------------------------------------------------------------------------------
    // AXI write word
    task axi_read(input logic[31:0] addr, output logic[31:0] rdata);
        automatic xil_axi_prot_t protectionType = 3'b000;
        automatic xil_axi_resp_t bresp;

        mst_agent.AXI4LITE_READ_BURST(addr, protectionType, rdata, bresp);

        if (VERBOSE_LEVEL > 0) begin
            $display("  %t: AXI read: addr 0x%h, data 0x%h, bresp %d", $time, addr, rdata, bresp);
        end

        if (bresp != 0) begin
            $display("  %t: ERROR: AXI read failed: addr 0x%h, data 0x%h, bresp %d", $time, addr, rdata, bresp);
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    // AXI write word
    task axi_write(input logic[31:0] addr, input logic[31:0] wdata);
        automatic xil_axi_prot_t protectionType = 3'b000;
        automatic xil_axi_resp_t bresp;

        mst_agent.AXI4LITE_WRITE_BURST(addr, protectionType, wdata, bresp);

        if (VERBOSE_LEVEL > 0) begin
            $display("  %t: AXI write: addr 0x%h, data 0x%h, bresp %d", $time, addr, wdata, bresp);
        end

        if (bresp != 0) begin
            $display("  %t: ERROR: AXI write failed: addr 0x%h, data 0x%h, bresp %d", $time, addr, wdata, bresp);
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task compare_data(input logic[31:0] expected, input logic[31:0] actual, string msg);
        if (expected === 'hx || actual === 'hx) begin
            $display("  %t: ERROR: %s: compare data cannot be performed - expected or actual data are 'x'", $time, msg);
            err_counter++;
        end else if (actual != expected) begin
            $display("  %t: ERROR: %s: data mismatch, expected = 0x%h, get = 0x%h", $time, msg, expected, actual);
            err_counter++;
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task uart_send_data(input logic[7:0] array[$]);
        for (int i = 0; i < array.size; i++) begin
            i_uart.send_frame_tx(array[i]);
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task uart_read_and_compare(input logic[7:0] expected[$]);
        automatic logic[7:0] rdata = 0;

        for (int i = 0; i < expected.size; i++) begin
            // wait for trigger
            @(i_uart.ev_uart_rx.triggered);
            rdata = i_uart.rcv_data;
            compare_data(expected[i], rdata, "UART");
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task fifo_write_cmd(input logic[31:0] array[$]);
        for (int i = 0; i < array.size; i++) begin
            axi_write(CMD_TX_FIFO, array[i]);
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task fifo_write_work(input logic[31:0] array[$]);
        for (int i = 0; i < array.size; i++) begin
            axi_write(WORK_TX_FIFO, array[i]);
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task fifo_read_and_compare_cmd(input logic[31:0] expected[$]);
        automatic logic[31:0] rdata = 0;

        for (int i = 0; i < expected.size; i++) begin
            axi_read(CMD_RX_FIFO, rdata);
            if (VERBOSE_LEVEL > 0) begin
                $display("  %t: read cmd RX FIFO: 0x%h", $time, rdata);
            end
            compare_data(expected[i], rdata, "CMD_RX_FIFO");
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    task fifo_read_and_compare_work(input logic[31:0] expected[$]);
        automatic logic[31:0] rdata = 0;

        for (int i = 0; i < expected.size; i++) begin
            axi_read(WORK_RX_FIFO, rdata);
            if (VERBOSE_LEVEL > 0) begin
                $display("  %t: read work RX FIFO: 0x%h", $time, rdata);
            end
            compare_data(expected[i], rdata, "WORK_RX_FIFO");
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    // check of flag in status register
    task check_status(input logic[31:0] flag, bit expected, input string err_msg);
        static logic[31:0] rdata = 0;

        // read status register
        axi_read(STAT_REG, rdata);

        // check flag
        if (((rdata & flag) != 0) != expected) begin
            $display("  %t: ERROR: %s", $time, err_msg);
            err_counter++;
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    // check value of interrupt ports
    // expected value is concatenation of {irq_cmd_rx, irq_work_rx, irq_work_tx}
    task check_irq(logic[2:0] exp_ports, logic[2:0] exp_flags, string msg);
        if (irq_cmd_rx !== exp_ports[2]) begin
            $display("  %t: ERROR: irq_cmd_rx should be %d (%s)", $time, exp_ports[2], msg);
            err_counter++;
        end

        if (irq_work_rx !== exp_ports[1]) begin
            $display("  %t: ERROR: irq_work_rx should be %d (%s)", $time, exp_ports[1], msg);
            err_counter++;
        end

        if (irq_work_tx !== exp_ports[0]) begin
            $display("  %t: ERROR: irq_work_tx should be %d (%s)", $time, exp_ports[0], msg);
            err_counter++;
        end

        // check status flags
        check_irq_flags(exp_flags, msg);
    endtask

    // ---------------------------------------------------------------------------------------------
    // check interrupt flags in status register
    // expected value is concatenation of {irq_cmd_rx, irq_work_rx, irq_work_tx}
    task check_irq_flags(logic[2:0] exp_flags, string msg);
        static logic[31:0] rdata = 0;

        // read status register
        axi_read(STAT_REG, rdata);

        if (((rdata & STAT_IRQ_PEND_CMD_RX) != 0) != exp_flags[2]) begin
            $display("  %t: ERROR: IRQ_PEND_CMD_RX flag should be %d (%s)", $time, exp_flags[2], msg);
            err_counter++;
        end

        if (((rdata & STAT_IRQ_PEND_WORK_RX) != 0) != exp_flags[1]) begin
            $display("  %t: ERROR: IRQ_PEND_WORK_RX flag should be %d (%s)", $time, exp_flags[1], msg);
            err_counter++;
        end

        if (((rdata & STAT_IRQ_PEND_WORK_TX) != 0) != exp_flags[0]) begin
            $display("  %t: ERROR: IRQ_PEND_WORK_TX flag should be %d (%s)", $time, exp_flags[0], msg);
            err_counter++;
        end
    endtask

    // ---------------------------------------------------------------------------------------------
    // initialization of job ID to max. value
    task init_job_id();
        // Tx FIFO data - to set correct job ID
        static logic[31:0] fifo_data1[$] = {
            32'h0000007f, 32'hffffffff, 32'hffffffff, 32'hffffffff, 32'h00000000, 32'h00000000,
            32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000, 32'h00000000
        };

        // reference data send out through UART
        static logic[7:0] uart_data1[$] = {
            8'h21, 8'h36, 8'h7f, 8'h01, 8'h00, 8'h00, 8'h00, 8'h00, 8'hff, 8'hff, 8'hff, 8'hff,
            8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'hff, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00, 8'h00,
            8'h00, 8'h00, 8'h00, 8'h00, 8'h37, 8'he6
        };

        // set 1 midstate mode
        axi_write(CTRL_REG, CTRL_ENABLE | CTRL_MIDSTATE_1);

        fifo_write_work(fifo_data1);
        uart_read_and_compare(uart_data1);
    endtask

endmodule

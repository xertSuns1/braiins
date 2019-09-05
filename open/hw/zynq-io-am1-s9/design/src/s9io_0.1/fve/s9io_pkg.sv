/***************************************************************************************************
 * Copyright (C) 2019  Braiins Systems s.r.o.
 *
 * This file is part of Braiins Open-Source Initiative (BOSI).
 *
 * BOSI is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * Please, keep in mind that we may also license BOSI or any part thereof
 * under a proprietary license. For more information on the terms and conditions
 * of such proprietary license or if you have any other questions, please
 * contact us at opensource@braiins.com.
 ***************************************************************************************************
 * Project Name:   S9 Board Interface IP
 * Description:    Package with s9io IP core parameters
 *
 * Engineer:       Marian Pristach
 * Revision:       1.0.0 (26.09.2018)
 *
 * Comments:
 **************************************************************************************************/

`timescale 1ns / 1ps

package s9io_pkg;

    // ---------------------------------------------------------------------------------------------
    // clock parameter
    time CLK_PERIOD = 20ns;

    // ---------------------------------------------------------------------------------------------
    // Definition of IP core AXI interface
    // ---------------------------------------------------------------------------------------------
    // Command Interface Receive FIFO, read-only
    parameter CMD_RX_FIFO  = 32'h00;
    // Command Interface Transmit FIFO, write-only
    parameter CMD_TX_FIFO = 32'h04;
    // Work Interface Receive FIFO, read-only
    parameter WORK_RX_FIFO = 32'h08;
    // Work Interface Transmit FIFO, write-only
    parameter WORK_TX_FIFO = 32'h0C;
    // Control Register, read/write
    parameter CTRL_REG = 32'h10;
    // Status Register, read-only
    parameter STAT_REG = 32'h14;
    // Baudrate Divisor, read/write
    parameter BAUD_REG = 32'h18;
    // Work Time Delay, read/write
    parameter WORK_TIME = 32'h1C;
    // Threshold for Work Transmit FIFO IRQ (32b words), read/write
    parameter IRQ_FIFO_THR = 32'h20;
    // Counter of dropped frames (CRC mismatch, ...), read-only
    parameter ERR_COUNTER = 32'h30;
    // Last Work ID send to ASICs, read-only
    parameter LAST_WORK_ID = 32'h34;
    // Build ID (Unix timestamp), read-only
    parameter BUILD_ID = 32'h3C;

    // ---------------------------------------------------------------------------------------------
    // Control Register
    // ---------------------------------------------------------------------------------------------
    // Enable, read/write
    parameter CTRL_ENABLE = 32'h8000;
    // Number of midstates per work, encoded as log2 of values: 1, 2, 4, read/write
    parameter CTRL_MIDSTATE_1 = 32'h0000;
    parameter CTRL_MIDSTATE_2 = 32'h2000;
    parameter CTRL_MIDSTATE_4 = 32'h4000;
    // Enable interrupt for Work Receive FIFO, read/write
    parameter CTRL_IRQ_EN_WORK_RX = 32'h1000;
    // Enable interrupt for Work Transmit FIFO, read/write
    parameter CTRL_IRQ_EN_WORK_TX = 32'h0800;
    // Enable interrupt for Command Receive FIFO, read/write
    parameter CTRL_IRQ_EN_CMD_RX = 32'h0400;
    // Clear error counter, write-only
    parameter CTRL_ERR_CNT_CLEAR = 32'h0010;
    // Reset/clear Work Transmit FIFO, write-only
    parameter CTRL_RST_WORK_TX_FIFO = 32'h0008;
    // Reset/clear Work Receive FIFO, write-only
    parameter CTRL_RST_WORK_RX_FIFO = 32'h0004;
    // Reset/clear Command Transmit FIFO, write-only
    parameter CTRL_RST_CMD_TX_FIFO = 32'h0002;
    // Reset/clear Command Receive FIFO, write-only
    parameter CTRL_RST_CMD_RX_FIFO = 32'h0001;

    // ---------------------------------------------------------------------------------------------
    // Status Register - read-only
    // ---------------------------------------------------------------------------------------------
    // Interrupt pending for Work Receive FIFO
    parameter STAT_IRQ_PEND_WORK_RX = 32'h1000;
    // Interrupt pending for Work Transmit FIFO
    parameter STAT_IRQ_PEND_WORK_TX = 32'h0800;
    // Interrupt pending for Command Receive FIFO
    parameter STAT_IRQ_PEND_CMD_RX = 32'h0400;
    // Work Interface Transmit FIFO Full
    parameter STAT_WORK_TX_FULL = 32'h0080;
    // Work Interface Transmit FIFO Empty
    parameter STAT_WORK_TX_EMPTY = 32'h0040;
    // Work Interface Receive FIFO Full
    parameter STAT_WORK_RX_FULL = 32'h0020;
    // Work Interface Receive FIFO Empty
    parameter STAT_WORK_RX_EMPTY = 32'h0010;
    // Command Interface Transmit FIFO Full
    parameter STAT_CMD_TX_FULL = 32'h0008;
    // Command Interface Transmit FIFO Empty
    parameter STAT_CMD_TX_EMPTY = 32'h0004;
    // Command Interface Receive FIFO Full
    parameter STAT_CMD_RX_FULL = 32'h0002;
    // Command Interface Receive FIFO Empty
    parameter STAT_CMD_RX_EMPTY = 32'h0001;

endpackage : s9io_pkg

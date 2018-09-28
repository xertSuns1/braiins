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
 * Description:    UART Bus Functional Model
 *
 * Engineer:       Marian Pristach
 * Revision:       1.0.0 (23.09.2018)
 *
 * Comments:       Buadrate fixed to 3.125M
 **************************************************************************************************/

`timescale 1ns / 1ps

module uart_bfm(input logic rx, output logic tx);

    // Simulation parameters
    parameter VERBOSE_LEVEL = 0;
    time UART_PERIOD = 320ns;

    // local signals and registers
    int drive_tx = 0;
    logic tx_int = 1'b1;
    logic[7:0] rcv_data;
    event ev_uart_rx;

    task send_frame_tx(logic[7:0] tx_data);
        drive_tx = 1;
        tx_int = 1'b0;
        # UART_PERIOD;

        for (int i = 0; i < 8; i++) begin
            tx_int = tx_data [i];
            # UART_PERIOD;
        end

        tx_int = 1'b1;      // stop bit
        # UART_PERIOD;
        drive_tx = 0;
    endtask

    assign tx = drive_tx ? tx_int : 1'b1;

    // receiver
    always begin
        @(negedge(rx));     // wait for start bit
        #(UART_PERIOD/2);

        if (rx != 1'b0) begin
            $display(" UART monitor: ERROR: Start bit is too short!");
        end

        for (int cnt = 0; cnt < 8; ++cnt) begin
            #(UART_PERIOD)
            rcv_data[cnt]= rx;
        end

        #(UART_PERIOD);

        if (rx != 1'b1) begin
            $display(" UART monitor: ERROR: Missing stop bit!");
        end else begin
            if (VERBOSE_LEVEL > 0) begin
                $display(" UART monitor: received data 0x%h", rcv_data);
            end
            -> ev_uart_rx;
        end
    end

endmodule
